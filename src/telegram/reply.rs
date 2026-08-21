//! Outgoing message helpers.
//!
//! Every user-visible reply goes through here so two production concerns are
//! handled in exactly one place: Telegram's hard 4096-character limit (exceeding
//! it fails the whole send, so an unbounded list silently produced no reply at
//! all), and the authorization guards that must run before any handler body.

use crate::db::repos::users::AuthUser;
use crate::db::Db;
use crate::error::AppError;
use crate::telegram::auth::{self, Authorization};
use teloxide::prelude::*;
use teloxide::types::ChatId;

/// Telegram rejects `sendMessage` payloads longer than this, counted in UTF-16 code
/// units rather than characters or bytes.
pub const MAX_MESSAGE_LEN: usize = 4096;

/// Split at a slightly lower bound so a chunk plus any framing still fits.
const CHUNK_LEN: usize = 3900;

/// Length of `text` in the units Telegram actually counts.
///
/// Counting characters would under-count: every emoji and every character outside the
/// basic multilingual plane is two UTF-16 units, so a message of emoji that looks
/// half the limit long is rejected outright — and a rejected send means a missed
/// reply, or a missed alert.
fn width(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Splits `text` into pieces Telegram will accept, preferring line breaks and never
/// splitting inside a character.
pub fn chunk_message(text: &str, limit: usize) -> Vec<String> {
    // Clamped rather than asserted: a bad limit must not be able to panic a
    // long-running daemon.
    let limit = limit.max(1);

    if width(text) <= limit {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    let mut flush = |current: &mut String, current_width: &mut usize| {
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
            *current_width = 0;
        }
    };

    for line in text.split_inclusive('\n') {
        let line_width = width(line);

        // A single line longer than the limit has to be split mid-line.
        if line_width > limit {
            flush(&mut current, &mut current_width);

            for ch in line.chars() {
                let ch_width = ch.len_utf16();
                if current_width + ch_width > limit {
                    flush(&mut current, &mut current_width);
                }
                current.push(ch);
                current_width += ch_width;
            }
            continue;
        }

        if current_width + line_width > limit {
            flush(&mut current, &mut current_width);
        }

        current.push_str(line);
        current_width += line_width;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Sends `text`, transparently splitting it across several messages when needed.
pub async fn send_text(
    bot: &Bot,
    chat_id: ChatId,
    text: impl AsRef<str>,
) -> Result<(), teloxide::RequestError> {
    for chunk in chunk_message(text.as_ref(), CHUNK_LEN) {
        bot.send_message(chat_id, chunk).await?;
    }

    Ok(())
}

/// Sends a reply and logs (rather than propagates) a delivery failure. Used on
/// error paths where there is nothing further to fall back to.
pub async fn try_send(bot: &Bot, chat_id: ChatId, text: impl AsRef<str>) {
    if let Err(err) = send_text(bot, chat_id, text).await {
        tracing::warn!(chat_id = chat_id.0, %err, "failed to deliver telegram reply");
    }
}

/// Reports a handler failure to the user without leaking internal detail, and logs
/// the full error for operators.
pub async fn report_error(bot: &Bot, chat_id: ChatId, context: &'static str, err: &AppError) {
    match err {
        // Expected, user-caused outcomes: log quietly.
        AppError::NotFound(_) | AppError::InvalidInput(_) | AppError::Conflict(_) => {
            tracing::info!(context, %err, "command rejected");
        }
        _ => tracing::error!(context, %err, "command failed"),
    }

    try_send(bot, chat_id, err.user_message()).await;
}

/// Terminates a handler, reporting a failure to the user exactly once.
///
/// Every command and flow step funnels its result through here. Handlers used to `?`
/// on repository calls, which returned the error to the dispatcher's error handler:
/// it was logged, and the user was left staring at a chat that never replied.
pub async fn finish(
    bot: &Bot,
    chat_id: ChatId,
    context: &'static str,
    outcome: crate::error::Result<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Err(err) = outcome {
        report_error(bot, chat_id, context, &err).await;
    }

    Ok(())
}

/// Resolves the sender and replies with the denial reason when access is refused.
/// Returns `None` when the handler must stop.
pub async fn require_user(bot: &Bot, db: &Db, msg: &Message) -> Option<AuthUser> {
    resolve(bot, db, msg, false).await
}

/// As [`require_user`], additionally requiring the admin role.
pub async fn require_admin(bot: &Bot, db: &Db, msg: &Message) -> Option<AuthUser> {
    resolve(bot, db, msg, true).await
}

/// Authorization check that does not reply, for use as a routing filter where a later
/// branch is responsible for the response.
pub async fn is_authorized(db: &Db, msg: &Message) -> bool {
    matches!(
        auth::authorize(db, msg).await,
        Ok(Authorization::Allowed(_))
    )
}

async fn resolve(bot: &Bot, db: &Db, msg: &Message, admin: bool) -> Option<AuthUser> {
    let decision = match auth::authorize(db, msg).await {
        Ok(decision) => decision,
        Err(err) => {
            tracing::error!(%err, "authorization lookup failed");
            try_send(
                bot,
                msg.chat.id,
                "Could not verify your access right now. Please try again shortly.",
            )
            .await;
            return None;
        }
    };

    let decision = if admin {
        decision.require_admin()
    } else {
        decision
    };

    match decision {
        Authorization::Allowed(user) => Some(user),
        Authorization::Denied(reason) => {
            try_send(bot, msg.chat.id, reason.user_message()).await;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_a_single_chunk() {
        assert_eq!(chunk_message("hello", 100), vec!["hello"]);
    }

    #[test]
    fn splits_on_line_boundaries() {
        let text = "aaaa\nbbbb\ncccc\n";
        let chunks = chunk_message(text, 10);
        assert_eq!(chunks, vec!["aaaa\nbbbb\n", "cccc\n"]);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn splits_lines_longer_than_the_limit() {
        let text = "x".repeat(25);
        let chunks = chunk_message(&text, 10);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.chars().count() <= 10));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn never_splits_inside_a_character() {
        // Multi-byte characters: a naive byte split would produce invalid UTF-8.
        let text = "é".repeat(25);
        let chunks = chunk_message(&text, 10);
        assert!(chunks.iter().all(|c| width(c) <= 10));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn measures_surrogate_pairs_the_way_telegram_does() {
        // Each of these is one `char` but two UTF-16 units. Counting characters would
        // build chunks twice as long as Telegram accepts, and the send would fail.
        let text = "\u{1f6a8}".repeat(20);
        assert_eq!(text.chars().count(), 20);
        assert_eq!(width(&text), 40);

        let chunks = chunk_message(&text, 10);
        assert!(chunks.iter().all(|c| width(c) <= 10), "{chunks:?}");
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn every_chunk_fits_the_telegram_limit() {
        let text = (0..500)
            .map(|i| format!("rule {i} \u{26a0}\u{fe0f} is enabled\n"))
            .collect::<String>();

        let chunks = chunk_message(&text, CHUNK_LEN);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(width(chunk) <= MAX_MESSAGE_LEN, "{}", width(chunk));
        }
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn a_zero_limit_does_not_panic() {
        assert_eq!(chunk_message("ab", 0).concat(), "ab");
    }
}
