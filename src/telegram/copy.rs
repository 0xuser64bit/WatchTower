//! Every multi-line message the bot sends.
//!
//! Collected here so the tone and structure of the copy can be reviewed as a whole,
//! and built with `concat!` of single-line literals so source indentation cannot leak
//! into a message.
//!
//! Screens and guided prompts are rendered with Telegram's **HTML** parse mode, so any
//! literal `<`, `>` or `&` in this file must be written as an entity (`&lt;`, `&gt;`,
//! `&amp;`). User-supplied values are escaped by the renderer, not here. A handful of
//! terse error reprompts are still sent as plain text (see [`crate::telegram::reply`])
//! and may use raw comparison symbols; those are called out where they live.

// ── Main menu & onboarding ────────────────────────────────────────────────────────

/// A short, human summary of what is being watched, e.g. `2 tokens and 1 wallet`.
fn watching(tokens: i64, wallets: i64) -> String {
    let mut parts = Vec::new();
    if tokens > 0 {
        parts.push(plural(tokens, "token", "tokens"));
    }
    if wallets > 0 {
        parts.push(plural(wallets, "wallet", "wallets"));
    }
    match parts.len() {
        0 => "nothing yet".to_string(),
        1 => parts.remove(0),
        _ => format!("{} and {}", parts[0], parts[1]),
    }
}

fn plural(count: i64, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

/// The main menu body, adapting to whether anything is set up yet.
pub fn main_menu(tokens: i64, wallets: i64, rules: i64) -> String {
    if tokens == 0 && wallets == 0 && rules == 0 {
        return concat!(
            "<b>WatchTower</b>\n",
            "Solana price &amp; balance alerts.\n",
            "\n",
            "Nothing is tracked yet. Add a token or wallet, then create an\n",
            "alert and I'll message you when it moves.\n",
            "\n",
            "Pick a section to begin."
        )
        .to_string();
    }

    format!(
        concat!(
            "<b>WatchTower</b>\n",
            "Solana price &amp; balance alerts.\n",
            "\n",
            "Watching <b>{0}</b>, with {1}.\n",
            "\n",
            "Pick a section."
        ),
        watching(tokens, wallets),
        plural(rules, "alert", "alerts")
    )
}

pub const NO_ADMINS_WARNING: &str = concat!(
    "⚠️ There are no active admins, so alerts have nowhere to go.\n",
    "Open 🛡 Admin to add one."
);

// ── Help ──────────────────────────────────────────────────────────────────────────

pub const HELP: &str = concat!(
    "<b>WatchTower</b> — price &amp; balance alerts for Solana.\n",
    "\n",
    "<b>How alerts work</b>\n",
    "An alert fires once when its condition becomes true, then stays\n",
    "quiet until it clears. \"price above $100\" pings you when it crosses\n",
    "$100 — not every minute after. When it drops back under $100 it\n",
    "re-arms for the next crossing.\n",
    "\n",
    "Percentage alerts measure from the last time they fired, so\n",
    "\"up 10%\" tells you about every 10% move.\n",
    "\n",
    "<b>What you can watch</b>\n",
    "🪙 a token's price, in USD\n",
    "👛 a wallet's balance, in SOL\n",
    "\n",
    "<b>Conditions</b>\n",
    "Above · Below · At or above · At or below · Up % · Down %\n",
    "\n",
    "<b>Alert states</b>\n",
    "🟢 armed — waiting for the condition to become true\n",
    "🔴 firing — condition is true; you've been notified\n",
    "⚪ disabled — paused, not being checked\n",
    "\n",
    "<b>Good to know</b>\n",
    "Solana only, and read-only — WatchTower never holds a key or moves\n",
    "funds. Alerts are sent to every admin.\n",
    "\n",
    "Use the buttons here, or the menu button by the message box."
);

// ── Admin ───────────────────────────────────────────────────────────────────────────

pub fn admin_panel(active_admins: i64) -> String {
    let warning = if active_admins == 0 {
        "\n\n⚠️ No active admins — alerts cannot be delivered."
    } else {
        ""
    };

    format!(
        concat!(
            "<b>🛡 Admin Panel</b>\n",
            "\n",
            "Active admins (who receive every alert): <b>{0}</b>\n",
            "\n",
            "Manage who can use WatchTower and who is alerted.{1}"
        ),
        active_admins, warning
    )
}

pub const USER_GONE: &str = "That user is not registered.";
pub const CANNOT_SELF: &str = "You can't do that to your own account. Ask another admin.";
pub const LAST_ADMIN: &str = concat!(
    "That's the last active admin. Add another admin first, otherwise\n",
    "nobody could manage the bot or receive alerts."
);

pub fn ask_admin_id() -> String {
    concat!(
        "<b>Add an admin</b>\n",
        "\n",
        "Send the person's numeric Telegram user ID. It's a number like\n",
        "123456789 — @userinfobot will tell them theirs.\n",
        "\n",
        "They'll be able to manage the bot and will receive every alert."
    )
    .to_string()
}

pub const BAD_ADMIN_ID: &str =
    "That isn't a Telegram user ID. Send a positive number, e.g. 123456789.";

pub fn confirm_admin(target: i64, already_known: bool) -> String {
    let note = if already_known {
        "\n\nThey're already known to the bot; this grants them admin."
    } else {
        ""
    };
    format!(
        concat!(
            "<b>Add admin</b>\n",
            "\n",
            "User <code>{0}</code> will become an admin.{1}"
        ),
        target, note
    )
}

// ── Empty states & stale references ──────────────────────────────────────────────────

pub const EMPTY_ALERTS: &str = concat!(
    "<b>🚨 Your Alerts</b>\n",
    "\n",
    "No alerts yet.\n",
    "\n",
    "Create one and I'll ping you when a price or balance crosses your\n",
    "line."
);

pub const ALERT_GONE: &str = "That alert no longer exists.";

pub const EMPTY_TOKENS: &str = concat!(
    "<b>🪙 Tracked Tokens</b>\n",
    "\n",
    "No tokens yet.\n",
    "\n",
    "Add a token by its mint address to watch its price in USD."
);

pub const TOKEN_GONE: &str = "That token is no longer tracked.";

pub const EMPTY_WALLETS: &str = concat!(
    "<b>👛 Tracked Wallets</b>\n",
    "\n",
    "No wallets yet.\n",
    "\n",
    "Add a wallet by its address to watch its SOL balance."
);

pub const WALLET_GONE: &str = "That wallet is no longer tracked.";

pub const EMPTY_HISTORY: &str = concat!(
    "<b>📜 Alert History</b>\n",
    "\n",
    "Nothing has fired yet.\n",
    "\n",
    "When an alert triggers, it will appear here."
);

// ── Fallbacks ───────────────────────────────────────────────────────────────────────

pub const PASTED_AN_ADDRESS: &str = concat!(
    "That looks like a Solana address. To watch it, open the menu and\n",
    "choose 🪙 Add Token or 👛 Add Wallet — or send /addtoken or /addwallet."
);

pub const NOT_A_COMMAND: &str =
    "I only take taps and commands. Send /menu to open the menu, or /help.";

pub const NOT_A_PRIVATE_CHAT: &str = concat!(
    "WatchTower only works in a direct message.\n",
    "Open a private chat with me and send /start."
);

// ── Adding a token ──────────────────────────────────────────────────────────────────

pub const ASK_MINT: &str = concat!(
    "<b>Add a token</b>\n",
    "\n",
    "Which token? Paste its mint address — 32-44 letters and numbers,\n",
    "shown on any Solana explorer or on CoinGecko.\n",
    "\n",
    "USDC, for example:\n",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
);

pub const BAD_ADDRESS: &str = concat!(
    "That doesn't look like a Solana address. They're 32-44 letters and\n",
    "numbers, with no 0, O, I or l. Paste just the address."
);

pub const NOT_PRICED: &str = concat!(
    "<b>No price found</b>\n",
    "\n",
    "I can't find a USD price for that mint, so a price alert on it could\n",
    "never fire — so I'm not adding it.\n",
    "\n",
    "Check the address, or try a token listed on CoinGecko."
);

pub fn ask_token_name(price_line: &str) -> String {
    format!(
        concat!(
            "{0}\n",
            "\n",
            "Give it a short name like USDC to keep your alerts readable,\n",
            "or tap Skip to just use the address."
        ),
        price_line
    )
}

pub fn ask_wallet_name(balance_line: &str) -> String {
    format!(
        concat!(
            "{0}\n",
            "\n",
            "Give it a short name like Treasury to keep your alerts\n",
            "readable, or tap Skip to just use the address."
        ),
        balance_line
    )
}

pub const ASK_ADDRESS: &str = concat!(
    "<b>Add a wallet</b>\n",
    "\n",
    "Paste the wallet address — 32-44 letters and numbers, the same\n",
    "thing you'd paste into an explorer.\n",
    "\n",
    "I only read the balance. I never need a key or seed phrase, and I\n",
    "can't move anything."
);

pub const ASK_SHORT_NAME: &str = "Send a short name as text, or tap Skip.";

pub fn token_saved(display: &str) -> String {
    format!(
        concat!(
            "✅ Tracking <b>{0}</b>.\n",
            "\n",
            "Create an alert to get pinged when its price moves."
        ),
        display
    )
}

pub fn wallet_saved(display: &str) -> String {
    format!(
        concat!(
            "✅ Tracking <b>{0}</b>.\n",
            "\n",
            "Create an alert to get pinged when its SOL balance moves."
        ),
        display
    )
}

pub const CANCELLED_NOTHING_ADDED: &str = "Cancelled — nothing was added.";

// ── Creating an alert ───────────────────────────────────────────────────────────────

pub const NOTHING_TO_ALERT_ON: &str = concat!(
    "<b>Create an alert</b>\n",
    "\n",
    "There's nothing to watch yet. Add a token (for its price) or a\n",
    "wallet (for its SOL balance) first, then come back."
);

pub const ASK_ALERT_KIND: &str = concat!(
    "<b>New alert — step 1 of 4</b>\n",
    "\n",
    "What should this alert watch?"
);

pub const BAD_ALERT_KIND: &str = "Tap Token or Wallet, or send the word `token` or `wallet`.";

pub const ASK_TARGET: &str = concat!(
    "<b>New alert — step 2 of 4</b>\n",
    "\n",
    "Which one? Tap it below."
);

pub const BAD_TARGET_NUMBER: &str = "Tap one of the buttons, or send its number.";

pub const ASK_OPERATOR: &str = concat!(
    "<b>New alert — step 3 of 4</b>\n",
    "\n",
    "When should I ping you?\n",
    "\n",
    "Above / Below fire when the value crosses a number you set.\n",
    "Up % / Down % fire on each move of that size from where it is now."
);

// Plain-text reprompt: sent without a parse mode, so raw symbols are fine here.
pub const BAD_OPERATOR: &str = "Tap a condition above, or send one of:  >  <  >=  <=  %up  %down";

pub const ASK_PERCENT: &str = concat!(
    "<b>New alert — step 4 of 4</b>\n",
    "\n",
    "How big a move? Send a percentage, like 10 for 10%.\n",
    "\n",
    "I take a reading now and measure from there, resetting after each\n",
    "alert so you hear about every move of that size."
);

pub fn ask_threshold(example: &str, unit: &str) -> String {
    format!(
        concat!(
            "<b>New alert — step 4 of 4</b>\n",
            "\n",
            "What value? Send a number on its own, like {0}.\n",
            "\n",
            "This one is in {1}."
        ),
        example, unit
    )
}

pub const BAD_THRESHOLD: &str =
    "Send a single positive number, like 1.5 or 250. No symbols, no units.";

pub const THRESHOLD_TOO_BIG: &str =
    "That's over 1000%, which is almost always a typo. Try a smaller number.";

pub fn ask_cooldown(default_seconds: i64) -> String {
    format!(
        concat!(
            "<b>One more thing</b>\n",
            "\n",
            "How long should I wait before this alert can fire again?\n",
            "\n",
            "Tap Use default ({0}s) — right for almost everyone — or send a\n",
            "number of seconds. It only matters if the value flickers across\n",
            "your line; the alert already stays quiet until the condition clears."
        ),
        default_seconds
    )
}

pub const BAD_COOLDOWN: &str =
    "Send a whole number of seconds between 0 and 86400, or tap Use default.";

pub fn confirm_alert(target: &str, condition: &str, cooldown_seconds: i64) -> String {
    format!(
        concat!(
            "<b>New Alert</b>\n",
            "\n",
            "{0}\n",
            "<b>Condition:</b> {1}\n",
            "<b>Then wait:</b> {2}s before repeating\n",
            "\n",
            "Create it?"
        ),
        target, condition, cooldown_seconds
    )
}

pub fn alert_saved(target: &str, condition: &str, poll_seconds: u64) -> String {
    format!(
        concat!(
            "✅ <b>Alert created</b>\n",
            "\n",
            "{0}\n",
            "<b>Condition:</b> {1}\n",
            "\n",
            "I check every {2}s and will message you when it happens."
        ),
        target, condition, poll_seconds
    )
}

pub const CANCELLED_NO_ALERT: &str = "Cancelled — no alert was created.";

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixed string, plus one rendering of every parameterised one.
    fn all_copy() -> Vec<(&'static str, String)> {
        vec![
            ("main_menu_empty", main_menu(0, 0, 0)),
            ("main_menu_full", main_menu(2, 1, 3)),
            ("no_admins", NO_ADMINS_WARNING.into()),
            ("help", HELP.into()),
            ("admin_panel", admin_panel(1)),
            ("admin_panel_zero", admin_panel(0)),
            ("ask_admin_id", ask_admin_id()),
            ("bad_admin_id", BAD_ADMIN_ID.into()),
            ("confirm_admin", confirm_admin(123456789, false)),
            ("user_gone", USER_GONE.into()),
            ("cannot_self", CANNOT_SELF.into()),
            ("last_admin", LAST_ADMIN.into()),
            ("empty_alerts", EMPTY_ALERTS.into()),
            ("alert_gone", ALERT_GONE.into()),
            ("empty_tokens", EMPTY_TOKENS.into()),
            ("token_gone", TOKEN_GONE.into()),
            ("empty_wallets", EMPTY_WALLETS.into()),
            ("wallet_gone", WALLET_GONE.into()),
            ("empty_history", EMPTY_HISTORY.into()),
            ("pasted_an_address", PASTED_AN_ADDRESS.into()),
            ("not_a_command", NOT_A_COMMAND.into()),
            ("not_private", NOT_A_PRIVATE_CHAT.into()),
            ("ask_mint", ASK_MINT.into()),
            ("bad_address", BAD_ADDRESS.into()),
            ("not_priced", NOT_PRICED.into()),
            ("ask_token_name", ask_token_name("Current price: 1 USD.")),
            (
                "ask_wallet_name",
                ask_wallet_name("Current balance: 2.5 SOL."),
            ),
            ("ask_address", ASK_ADDRESS.into()),
            ("ask_short_name", ASK_SHORT_NAME.into()),
            ("token_saved", token_saved("USDC")),
            ("wallet_saved", wallet_saved("Treasury")),
            ("cancelled_nothing_added", CANCELLED_NOTHING_ADDED.into()),
            ("nothing_to_alert_on", NOTHING_TO_ALERT_ON.into()),
            ("ask_alert_kind", ASK_ALERT_KIND.into()),
            ("bad_alert_kind", BAD_ALERT_KIND.into()),
            ("ask_target", ASK_TARGET.into()),
            ("bad_target_number", BAD_TARGET_NUMBER.into()),
            ("ask_operator", ASK_OPERATOR.into()),
            ("bad_operator", BAD_OPERATOR.into()),
            ("ask_percent", ASK_PERCENT.into()),
            ("ask_threshold", ask_threshold("1.5", "USD")),
            ("bad_threshold", BAD_THRESHOLD.into()),
            ("threshold_too_big", THRESHOLD_TOO_BIG.into()),
            ("ask_cooldown", ask_cooldown(300)),
            ("bad_cooldown", BAD_COOLDOWN.into()),
            (
                "confirm_alert",
                confirm_alert("🪙 <b>USDC</b>", "below $0.99", 300),
            ),
            (
                "alert_saved",
                alert_saved("🪙 <b>USDC</b>", "below $0.99", 60),
            ),
            ("cancelled_no_alert", CANCELLED_NO_ALERT.into()),
        ]
    }

    #[test]
    fn no_message_has_trailing_whitespace_or_blank_padding() {
        for (name, text) in all_copy() {
            assert!(!text.ends_with('\n'), "{name} ends with a newline");
            assert!(!text.starts_with(' '), "{name} starts with a space");
            for (n, line) in text.lines().enumerate() {
                assert_eq!(line.trim_end(), line, "{name} line {n} has trailing space");
            }
        }
    }

    #[test]
    fn every_message_fits_one_telegram_send() {
        for (name, text) in all_copy() {
            let chunks = crate::telegram::reply::chunk_message(&text, 3900);
            assert_eq!(chunks.len(), 1, "{name} needs splitting");
        }
    }

    #[test]
    fn no_message_is_wider_than_a_phone_screen() {
        // Telegram wraps, but a hand-aligned line that wraps reads badly. HTML tags do
        // not render, so they are discounted from the visible width.
        for (name, text) in all_copy() {
            for (n, line) in text.lines().enumerate() {
                let visible = strip_tags(line).chars().count();
                assert!(
                    visible <= 72,
                    "{name} line {n} is {visible} chars: {line:?}"
                );
            }
        }
    }

    /// Crude tag stripper for the width check: removes `<...>` spans.
    fn strip_tags(line: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for ch in line.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(ch),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn help_explains_behaviour_states_and_scope() {
        // The three states shown on the alert screens must be explained.
        for state in ["armed", "firing", "disabled"] {
            assert!(HELP.contains(state), "{state} is unexplained");
        }
        // The edge-trigger behaviour and the percentage nuance are the whole point.
        assert!(HELP.contains("crosses"), "edge behaviour not explained");
        assert!(HELP.contains("every 10% move"), "percentage nuance missing");
        // Scope, so nobody assumes multi-chain or custody.
        assert!(HELP.contains("Solana only"));
        assert!(HELP.contains("read-only"));
        // The six conditions are named in words rather than raw symbols.
        for word in [
            "Above",
            "Below",
            "At or above",
            "At or below",
            "Up %",
            "Down %",
        ] {
            assert!(HELP.contains(word), "{word} condition missing from help");
        }
    }

    #[test]
    fn html_copy_has_no_stray_unescaped_specials() {
        // A raw `&` not part of an entity, or a `<`/`>` not part of a tag, would break
        // the HTML send. Reprompts sent as plain text are exempt.
        let plain_reprompts = [
            "bad_operator",
            "bad_address",
            "bad_alert_kind",
            "bad_target_number",
            "bad_threshold",
            "threshold_too_big",
            "bad_cooldown",
            "ask_short_name",
            "bad_admin_id",
            "cancelled_nothing_added",
            "cancelled_no_alert",
            "not_a_command",
            "user_gone",
            "cannot_self",
            "alert_gone",
            "token_gone",
            "wallet_gone",
        ];

        for (name, text) in all_copy() {
            if plain_reprompts.contains(&name) {
                continue;
            }
            assert!(
                well_formed_html(&text),
                "{name} has unescaped HTML: {text:?}"
            );
        }
    }

    /// Checks that `<`/`>` only appear as balanced simple tags and every `&` starts a
    /// known entity. Good enough for the small, controlled tag vocabulary used here.
    fn well_formed_html(text: &str) -> bool {
        let mut depth = 0i32;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '<' => {
                    if depth != 0 {
                        return false;
                    }
                    depth = 1;
                }
                '>' => {
                    if depth != 1 {
                        return false;
                    }
                    depth = 0;
                }
                '&' => {
                    let rest: String = chars.clone().take(5).collect();
                    if !(rest.starts_with("amp;")
                        || rest.starts_with("lt;")
                        || rest.starts_with("gt;"))
                    {
                        return false;
                    }
                }
                _ => {}
            }
        }
        depth == 0
    }

    #[test]
    fn prompts_show_a_concrete_example_of_the_answer() {
        assert!(ASK_MINT.contains("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"));
        assert!(ask_threshold("1.5", "USD").contains("1.5"));
        assert!(ASK_PERCENT.contains("10"));
        assert!(ask_admin_id().contains("123456789"));
    }
}
