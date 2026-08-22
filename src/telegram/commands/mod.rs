//! Command handlers.

pub mod admin;
pub mod alerts;
pub mod status;
pub mod targets;

use crate::app_state::AppState;
use crate::db::repos::rules::RuleRepo;
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::users::{AuthUser, Role, UserRepo};
use crate::db::repos::wallets::WalletRepo;
use crate::error::Result;
use crate::telegram::flows::HandlerResult;
use crate::telegram::{copy, reply};
use teloxide::prelude::*;

/// What the user currently has set up, used to tailor `/start`.
struct Summary {
    tokens: i64,
    wallets: i64,
    rules: i64,
}

impl Summary {
    async fn load(state: &AppState) -> Result<Self> {
        Ok(Summary {
            tokens: TokenRepo::new(&state.db).count().await?,
            wallets: WalletRepo::new(&state.db).count().await?,
            rules: RuleRepo::new(&state.db).count_all().await?,
        })
    }

    fn is_empty(&self) -> bool {
        self.tokens == 0 && self.wallets == 0 && self.rules == 0
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();

        if self.tokens > 0 {
            parts.push(plural(self.tokens, "token", "tokens"));
        }
        if self.wallets > 0 {
            parts.push(plural(self.wallets, "wallet", "wallets"));
        }

        let watching = match parts.len() {
            0 => "nothing yet".to_string(),
            1 => parts.remove(0),
            _ => format!("{} and {}", parts[0], parts[1]),
        };

        if self.rules > 0 {
            format!("{watching} with {}", plural(self.rules, "alert", "alerts"))
        } else {
            format!("{watching}, with no alerts set up yet")
        }
    }
}

fn plural(count: i64, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

pub async fn start(state: AppState, msg: Message) -> HandlerResult {
    let Some(user) = reply::require_user(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let outcome = render_start(&state, &msg, user).await;
    reply::finish(&state.bot, msg.chat.id, "start", outcome).await
}

/// `/start` adapts to what the user already has.
///
/// A first-time user gets three steps, not a command list: twenty-odd commands in the
/// very first message is how someone closes the chat and never comes back. A returning
/// user gets a status line and the three commands they actually reach for.
async fn render_start(state: &AppState, msg: &Message, user: AuthUser) -> Result<()> {
    let summary = Summary::load(state).await?;

    let text = if summary.is_empty() {
        copy::quick_start(state.settings.poll_interval.as_secs())
    } else {
        copy::returning_welcome(&summary.describe())
    };

    reply::send_text(&state.bot, msg.chat.id, text).await?;

    // Surfaced only to admins, and only when it matters: an admin whose alerts have
    // nowhere to land needs to know now, not on their next /status.
    if user.role == Role::Admin && UserRepo::new(&state.db).count_active_admins().await? == 0 {
        reply::send_text(&state.bot, msg.chat.id, copy::NO_ADMINS_WARNING).await?;
    }

    Ok(())
}

pub async fn help(state: AppState, msg: Message) -> HandlerResult {
    let Some(user) = reply::require_user(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let mut text = copy::HELP.to_string();
    if user.role == Role::Admin {
        text.push_str(copy::HELP_ADMIN);
    }

    let outcome = reply::send_text(&state.bot, msg.chat.id, text)
        .await
        .map_err(Into::into);
    reply::finish(&state.bot, msg.chat.id, "help", outcome).await
}

/// Anything that is not a command and not part of an active flow.
pub async fn fallback(state: AppState, msg: Message) -> HandlerResult {
    // Authorize first: an unknown sender must not learn anything about the bot.
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    // A pasted address is the most likely thing someone sends without a command, so
    // name the two commands that take one instead of pointing at the manual.
    let looks_like_an_address = msg
        .text()
        .map(str::trim)
        .is_some_and(crate::providers::solana::is_valid_address);

    let text = if looks_like_an_address {
        copy::PASTED_AN_ADDRESS
    } else {
        copy::NOT_A_COMMAND
    };

    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

/// Refuses group, supergroup, and channel chats.
///
/// Replies only to an apparent command attempt: answering every message in a group the
/// bot happens to be in would be noise, and could trip Telegram's flood limits.
pub async fn non_private_chat(state: AppState, msg: Message) -> HandlerResult {
    if msg.text().is_some_and(|text| text.starts_with('/')) {
        tracing::info!(
            chat_id = msg.chat.id.0,
            "refused a command from a non-private chat"
        );
        reply::try_send(&state.bot, msg.chat.id, copy::NOT_A_PRIVATE_CHAT).await;
    }

    Ok(())
}

/// Parses a positive row id from a command argument, replying with usage on failure.
pub async fn parse_id(state: &AppState, msg: &Message, raw: &str, usage: &str) -> Option<i64> {
    match raw.trim().parse::<i64>() {
        Ok(id) if id > 0 => Some(id),
        _ => {
            reply::try_send(
                &state.bot,
                msg.chat.id,
                format!("{usage}\n\nThe number comes from the listing, e.g. /alerts."),
            )
            .await;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(tokens: i64, wallets: i64, rules: i64) -> Summary {
        Summary {
            tokens,
            wallets,
            rules,
        }
    }

    #[test]
    fn a_user_with_nothing_set_up_gets_the_quick_start() {
        assert!(summary(0, 0, 0).is_empty());
        assert!(!summary(1, 0, 0).is_empty());
        assert!(!summary(0, 0, 1).is_empty());
    }

    #[test]
    fn summary_reads_as_a_sentence() {
        assert_eq!(summary(1, 0, 1).describe(), "1 token with 1 alert");
        assert_eq!(
            summary(2, 3, 5).describe(),
            "2 tokens and 3 wallets with 5 alerts"
        );
        assert_eq!(
            summary(1, 0, 0).describe(),
            "1 token, with no alerts set up yet"
        );
        assert_eq!(summary(0, 1, 2).describe(), "1 wallet with 2 alerts");
    }
}
