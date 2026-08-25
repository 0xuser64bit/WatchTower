//! The presentation toolkit shared by every screen and guided step.
//!
//! Two ideas keep the interface feeling like a small application rather than a
//! command line:
//!
//! * A [`Surface`] is *where* a screen should appear — a fresh message, or an edit
//!   of the message a button lives on. Editing in place is what stops a tap-driven
//!   session from flooding the chat with a new bubble per action.
//! * A [`Screen`] is *what* to show — rendered text plus its inline keyboard. Screens
//!   are built once and can be sent or edited onto any surface.
//!
//! All screen text is HTML. Telegram's HTML mode needs only three characters escaped,
//! so [`esc`] is applied to every piece of user-supplied text (labels, symbols,
//! addresses) on the way in. A single missed escape fails the send, so dynamic values
//! must never reach a screen un-escaped.

use teloxide::prelude::*;
use teloxide::types::{
    CallbackQueryId, InlineKeyboardButton, InlineKeyboardMarkup, LinkPreviewOptions, MessageId,
    ParseMode,
};
use teloxide::ApiError;

/// Escapes the three characters Telegram's HTML parse mode treats specially.
///
/// Applied to every user-supplied value rendered into a screen. Addresses are
/// base58 and never contain these, but labels and symbols are free text.
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Wraps text as an inline `<code>` span: monospaced, and tap-to-copy on mobile —
/// exactly what a Solana address wants to be.
pub fn code(text: &str) -> String {
    format!("<code>{}</code>", esc(text))
}

/// A callback button. Data is kept short by callers; Telegram caps it at 64 bytes.
pub fn button(text: impl Into<String>, data: impl Into<String>) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(text, data)
}

/// The standard bottom navigation row: back to a specific screen, and home.
pub fn back_menu(back_data: impl Into<String>) -> Vec<InlineKeyboardButton> {
    vec![
        button("← Back", back_data),
        button("🏠 Menu", super::callback::MAIN),
    ]
}

/// A lone "home" row, for leaf screens with no meaningful parent.
pub fn menu_row() -> Vec<InlineKeyboardButton> {
    vec![button("🏠 Menu", super::callback::MAIN)]
}

/// A lone "cancel" row for a guided step. Cancelling always lands on the main menu.
pub fn cancel_row() -> Vec<InlineKeyboardButton> {
    vec![button("✕ Cancel", super::callback::CANCEL)]
}

/// A finished screen: body text (HTML) and the keyboard beneath it.
pub struct Screen {
    pub text: String,
    pub keyboard: InlineKeyboardMarkup,
}

impl Screen {
    pub fn new(text: impl Into<String>, rows: Vec<Vec<InlineKeyboardButton>>) -> Self {
        Self {
            text: text.into(),
            keyboard: InlineKeyboardMarkup::new(rows),
        }
    }
}

/// Where a screen should be delivered.
#[derive(Clone, Copy, Debug)]
pub enum Surface {
    /// Post a new message into the chat.
    New(ChatId),
    /// Replace an existing message in place, keeping the conversation compact.
    Edit(ChatId, MessageId),
}

impl Surface {
    pub fn chat(self) -> ChatId {
        match self {
            Surface::New(chat) | Surface::Edit(chat, _) => chat,
        }
    }
}

fn no_preview() -> LinkPreviewOptions {
    LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    }
}

/// Renders a screen onto a surface.
///
/// On an [`Surface::Edit`], an unchanged edit is not an error worth surfacing: it
/// happens whenever a user double-taps a button, so Telegram's "message is not
/// modified" is swallowed. Every other failure propagates so the handler boundary can
/// log it and tell the user once.
pub async fn render(bot: &Bot, surface: Surface, screen: Screen) -> crate::error::Result<()> {
    match surface {
        Surface::New(chat) => {
            bot.send_message(chat, screen.text)
                .parse_mode(ParseMode::Html)
                .link_preview_options(no_preview())
                .reply_markup(screen.keyboard)
                .await?;
        }
        Surface::Edit(chat, message_id) => {
            let result = bot
                .edit_message_text(chat, message_id, screen.text)
                .parse_mode(ParseMode::Html)
                .link_preview_options(no_preview())
                .reply_markup(screen.keyboard)
                .await;

            match result {
                Ok(_) => {}
                // A no-op edit (double tap on the same button) is not a real failure.
                Err(teloxide::RequestError::Api(ApiError::MessageNotModified)) => {}
                Err(err) => return Err(err.into()),
            }
        }
    }

    Ok(())
}

/// Clears the spinner on a tapped button. Best-effort: a callback that cannot be
/// acknowledged (too old, already answered) must not fail the handler.
pub async fn ack(bot: &Bot, id: CallbackQueryId) {
    if let Err(err) = bot.answer_callback_query(id).await {
        tracing::debug!(%err, "failed to answer callback query");
    }
}

/// Answers a callback with a small toast at the top of the chat. Used for outcomes
/// that do not warrant redrawing the screen ("Already up to date").
pub async fn toast(bot: &Bot, id: CallbackQueryId, text: &str) {
    if let Err(err) = bot.answer_callback_query(id).text(text).await {
        tracing::debug!(%err, "failed to answer callback query with toast");
    }
}

/// Answers a callback with a modal alert the user must dismiss. Reserved for refusals
/// and expired-button notices, where a fleeting toast is too easy to miss.
pub async fn alert(bot: &Bot, id: CallbackQueryId, text: &str) {
    if let Err(err) = bot
        .answer_callback_query(id)
        .text(text)
        .show_alert(true)
        .await
    {
        tracing::debug!(%err, "failed to answer callback query with alert");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_only_the_three_html_specials() {
        assert_eq!(esc("a & b < c > d"), "a &amp; b &lt; c &gt; d");
        // A label crafted to break out of the markup is neutralised.
        assert_eq!(esc("<b>oops</b>"), "&lt;b&gt;oops&lt;/b&gt;");
        // Plain base58 is untouched.
        assert_eq!(
            esc("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        );
    }

    #[test]
    fn code_span_escapes_its_contents() {
        assert_eq!(code("a<b"), "<code>a&lt;b</code>");
    }
}
