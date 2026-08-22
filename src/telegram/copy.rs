//! Every multi-line message the bot sends.
//!
//! Collected here for two reasons. The tone and structure of the copy can be reviewed
//! as a whole rather than hunted across handlers, and the strings are built with
//! `concat!` of single-line literals so source indentation cannot leak into a message
//! — a backslash line-continuation silently swallows the following line's leading
//! whitespace only when it is written exactly right, and getting it wrong ships
//! ragged output that no type checker will catch.
//!
//! Plain text only. Telegram's Markdown modes would need every user-supplied label and
//! base58 address escaped on every path, and one missed escape fails the send.

// ── Onboarding ──────────────────────────────────────────────────────────────────────

pub fn quick_start(poll_seconds: u64) -> String {
    format!(
        concat!(
            "ChainSentinel watches Solana and messages you when something moves.\n",
            "\n",
            "Three steps to your first alert:\n",
            "\n",
            "  1. /addtoken   add a token you care about\n",
            "  2. /addalert   tell me when to ping you\n",
            "  3. that's it   I check every {0} seconds from then on\n",
            "\n",
            "You only ever paste an address. I ask one short question at a\n",
            "time, and /cancel gets you out of anything.\n",
            "\n",
            "Want to see it work first? Track USDC:\n",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v\n",
            "\n",
            "Every command, with examples: /help"
        ),
        poll_seconds
    )
}

pub fn returning_welcome(watching: &str) -> String {
    format!(
        concat!(
            "ChainSentinel is watching {0}.\n",
            "\n",
            "Check in:\n",
            "  /alerts    your alerts, and whether any are firing\n",
            "  /status    is monitoring healthy right now\n",
            "  /history   what has fired recently\n",
            "\n",
            "Add more:\n",
            "  /addtoken   /addwallet   /addalert\n",
            "\n",
            "Everything, with examples: /help"
        ),
        watching
    )
}

pub const NO_ADMINS_WARNING: &str = concat!(
    "Heads up: there are no active admins, so alerts have nowhere to go.\n",
    "Use /addadmin to fix that."
);

pub const HELP: &str = concat!(
    "ChainSentinel - price and balance alerts for Solana.\n",
    "\n",
    "HOW ALERTS WORK\n",
    "  An alert fires when its condition becomes true, then goes quiet\n",
    "  until it clears. \"price > 100\" pings you once when it crosses 100,\n",
    "  not every minute while it stays there. When it falls back under\n",
    "  100 the alert re-arms, ready for the next crossing.\n",
    "\n",
    "SET SOMETHING UP\n",
    "  /addtoken    track a token, by mint address\n",
    "  /addwallet   track a wallet, by address\n",
    "  /addalert    create an alert on something you track\n",
    "\n",
    "  Each asks a few short questions; /cancel gets you out.\n",
    "  Track a token or wallet first - alerts attach to those.\n",
    "\n",
    "WHAT YOU CAN WATCH\n",
    "  A token's price in USD, and a wallet's SOL balance.\n",
    "\n",
    "  >       goes above       e.g.  > 250      price passes $250\n",
    "  <       drops below      e.g.  < 5        balance dips under 5 SOL\n",
    "  >=      at or above\n",
    "  <=      at or below\n",
    "  %up     rises by         e.g.  %up 10     every +10% move\n",
    "  %down   falls by         e.g.  %down 15   every -15% move\n",
    "\n",
    "  Percentage alerts measure from the last time they fired, so\n",
    "  \"%up 10\" tells you about every 10% move, not just the first.\n",
    "\n",
    "LOOK AT THINGS\n",
    "  /alerts     your alerts, their state and last reading\n",
    "  /tokens     tracked tokens\n",
    "  /wallets    tracked wallets\n",
    "  /history    alerts that have already fired\n",
    "  /status     is monitoring healthy, are the data feeds up\n",
    "\n",
    "CHANGE THINGS\n",
    "  /disablerule 3     pause alert 3, keep it\n",
    "  /enablerule 3      start it again, freshly armed\n",
    "  /deleterule 3      remove alert 3\n",
    "  /deletetoken 2     stop tracking token 2, and its alerts\n",
    "  /deletewallet 1    stop tracking wallet 1, and its alerts\n",
    "\n",
    "  The numbers come from /alerts, /tokens and /wallets.\n",
    "\n",
    "WHAT THE STATES MEAN\n",
    "  armed      condition is false; you'll be pinged when it turns true\n",
    "  firing     condition is true and you have been pinged\n",
    "  disabled   paused, not being checked\n",
    "\n",
    "GOOD TO KNOW\n",
    "  Solana only, and read-only - I never hold a key or move funds.\n",
    "  Alerts go to every admin. /status says if that list is empty."
);

pub const HELP_ADMIN: &str = concat!(
    "\n",
    "\n",
    "ADMIN\n",
    "  /admin                the admin panel\n",
    "  /listusers            who can use this bot\n",
    "  /addadmin 123456789   let someone in, as an admin\n",
    "  /demote 123456789     take admin away\n",
    "  /block 123456789      revoke all access\n",
    "  /unblock 123456789    restore it\n",
    "\n",
    "  Those are Telegram user IDs, not usernames - @userinfobot\n",
    "  will tell someone theirs.\n",
    "\n",
    "  You can't demote or block yourself, and the last admin can't be\n",
    "  removed: there would be no way back in."
);

pub const ADMIN_PANEL: &str = concat!(
    "Admin panel\n",
    "\n",
    "  /listusers            who can use this bot\n",
    "  /addadmin 123456789   let someone in, as an admin\n",
    "  /demote 123456789     take admin away\n",
    "  /block 123456789      revoke all access\n",
    "  /unblock 123456789    restore it\n",
    "\n",
    "Only registered, unblocked users can use the bot at all, and active\n",
    "admins are who every alert is sent to."
);

// ── Fallbacks ───────────────────────────────────────────────────────────────────────

pub const PASTED_AN_ADDRESS: &str = concat!(
    "That looks like a Solana address. To watch it:\n",
    "\n",
    "  /addtoken    if it's a token mint\n",
    "  /addwallet   if it's a wallet\n",
    "\n",
    "I'll ask for the address next."
);

pub const NOT_A_COMMAND: &str = "I only take commands. /help lists them, /start if you're new.";

pub const NOT_A_PRIVATE_CHAT: &str = concat!(
    "ChainSentinel only works in a direct message.\n",
    "Open a private chat with me and send /start."
);

// ── Adding a token ──────────────────────────────────────────────────────────────────

pub const ASK_MINT: &str = concat!(
    "Which token? Paste its mint address.\n",
    "\n",
    "It's 32-44 letters and numbers - you'll find it on the token's page\n",
    "on any Solana explorer, or on CoinGecko.\n",
    "\n",
    "USDC, for example:\n",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v\n",
    "\n",
    "/cancel to stop."
);

pub const BAD_ADDRESS: &str = concat!(
    "That doesn't look like a Solana address. They're 32-44 letters and\n",
    "numbers, with no 0, O, I or l in them.\n",
    "\n",
    "Paste just the address, nothing else. /cancel to stop."
);

pub const NOT_PRICED: &str = concat!(
    "I can't find a USD price for that mint, so a price alert on it could\n",
    "never fire. Not adding it.\n",
    "\n",
    "Double-check the address, or try a token that's listed on CoinGecko."
);

pub fn ask_token_name(price_line: &str) -> String {
    format!(
        concat!(
            "{0}\n",
            "\n",
            "What should I call it? A short name like USDC keeps your alerts\n",
            "readable.\n",
            "\n",
            "Send a name, or `skip` to just use the address."
        ),
        price_line
    )
}

pub fn ask_wallet_name(balance_line: &str) -> String {
    format!(
        concat!(
            "{0}\n",
            "\n",
            "What should I call it? Something like Treasury or Cold wallet\n",
            "keeps your alerts readable.\n",
            "\n",
            "Send a name, or `skip` to just use the address."
        ),
        balance_line
    )
}

pub const ASK_ADDRESS: &str = concat!(
    "Which wallet? Paste its address.\n",
    "\n",
    "32-44 letters and numbers, the same thing you'd paste into an\n",
    "explorer. I only read the balance - I never need a key or a seed\n",
    "phrase, and I can't move anything.\n",
    "\n",
    "/cancel to stop."
);

pub const ASK_SHORT_NAME: &str = "Send a short name as text, or `skip`.";

pub fn confirm_token(name: &str, mint: &str) -> String {
    format!(
        concat!(
            "Ready to track:\n",
            "\n",
            "  Name   {0}\n",
            "  Mint   {1}\n",
            "\n",
            "Send `yes` to save it, or /cancel."
        ),
        name, mint
    )
}

pub fn confirm_wallet(name: &str, address: &str) -> String {
    format!(
        concat!(
            "Ready to track:\n",
            "\n",
            "  Name      {0}\n",
            "  Address   {1}\n",
            "\n",
            "Send `yes` to save it, or /cancel."
        ),
        name, address
    )
}

pub fn token_saved(display: &str, id: i64) -> String {
    format!(
        concat!(
            "Tracking {0} as token {1}.\n",
            "\n",
            "Next: /addalert to get pinged when its price moves."
        ),
        display, id
    )
}

pub fn wallet_saved(display: &str, id: i64) -> String {
    format!(
        concat!(
            "Tracking {0} as wallet {1}.\n",
            "\n",
            "Next: /addalert to get pinged when its SOL balance changes."
        ),
        display, id
    )
}

pub const CANCELLED_NOTHING_ADDED: &str = "Cancelled - nothing was added.";

// ── Creating an alert ───────────────────────────────────────────────────────────────

pub const NOTHING_TO_ALERT_ON: &str = concat!(
    "Nothing to alert on yet. Add something first:\n",
    "\n",
    "  /addtoken    a token, to watch its price\n",
    "  /addwallet   a wallet, to watch its SOL balance\n",
    "\n",
    "Then come back to /addalert."
);

pub fn ask_alert_kind(tokens: i64, wallets: i64) -> String {
    format!(
        concat!(
            "What should this alert watch?\n",
            "\n",
            "  `token`    a token's price in USD   ({0} tracked)\n",
            "  `wallet`   a wallet's SOL balance   ({1} tracked)\n",
            "\n",
            "Send one of those two words. /cancel to stop."
        ),
        tokens, wallets
    )
}

pub const BAD_ALERT_KIND: &str = "Send just the word `token` or `wallet`. /cancel to stop.";

pub fn ask_which_target(listing: &[String]) -> String {
    format!(
        "Which one? Send the number in front of it.\n\n{}",
        listing.join("\n")
    )
}

pub const BAD_TARGET_NUMBER: &str = "Send just the number in front of the one you want, like `1`.";

/// The step people get lost on, so each option is spelled out for this target type.
pub fn ask_operator(subject: &str, unit: &str, high: &str, low: &str) -> String {
    format!(
        concat!(
            "When should I ping you?\n",
            "\n",
            "  `>`       {0} goes above a number\n",
            "  `<`       {0} drops below a number\n",
            "  `>=`      at or above\n",
            "  `<=`      at or below\n",
            "  `%up`     it rises by some percent\n",
            "  `%down`   it falls by some percent\n",
            "\n",
            "So `>` then {2} pings you when the {0} passes {2} {1};\n",
            "`<` then {3} pings you when it drops under {3} {1}.\n",
            "\n",
            "Most people want `<` to catch a drop, or `%down` for a crash."
        ),
        subject, unit, high, low
    )
}

pub const BAD_OPERATOR: &str =
    "I didn't recognise that. Send exactly one of:  >  <  >=  <=  %up  %down";

pub const ASK_PERCENT: &str = concat!(
    "How big a move? Send a percentage, like `10` for 10%.\n",
    "\n",
    "I take a reading now and measure from there. Each time the alert\n",
    "fires I reset the starting point, so you hear about every move of\n",
    "that size - not just the first one."
);

pub fn ask_threshold(example: &str, unit: &str) -> String {
    format!(
        concat!(
            "What number? Send it on its own, like `{0}`.\n",
            "\n",
            "This one is in {1}."
        ),
        example, unit
    )
}

pub const BAD_THRESHOLD: &str = concat!(
    "Send a single positive number, like `1.5` or `250`.\n",
    "No symbols, no units."
);

pub const THRESHOLD_TOO_BIG: &str =
    "That's over 1000%, which is almost always a typo. Try a smaller number.";

pub fn ask_cooldown(default_seconds: i64) -> String {
    format!(
        concat!(
            "Last question. How long should I wait before this alert can fire\n",
            "again?\n",
            "\n",
            "Send `skip` for the default of {0} seconds - that's right for\n",
            "almost everyone.\n",
            "\n",
            "It only matters if the value keeps flickering across your number:\n",
            "the alert already stays quiet on its own until the condition clears."
        ),
        default_seconds
    )
}

pub const BAD_COOLDOWN: &str = "Send a whole number of seconds between 0 and 86400, or `skip`.";

pub fn confirm_alert(target: &str, condition: &str, cooldown_seconds: i64) -> String {
    format!(
        concat!(
            "Ready to go:\n",
            "\n",
            "  Watching     {0}\n",
            "  Ping when    {1}\n",
            "  Then wait    {2}s before repeating\n",
            "\n",
            "Send `yes` to save it, or /cancel."
        ),
        target, condition, cooldown_seconds
    )
}

pub fn alert_saved(id: i64, target: &str, condition: &str, poll_seconds: u64) -> String {
    format!(
        concat!(
            "Alert {0} is live: {1} {2}.\n",
            "\n",
            "I check every {3} seconds and will message you when it happens.\n",
            "See it any time with /alerts."
        ),
        id, target, condition, poll_seconds
    )
}

pub const CANCELLED_NO_ALERT: &str = "Cancelled - no alert was created.";

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixed string, plus one rendering of every parameterised one.
    fn all_copy() -> Vec<(&'static str, String)> {
        vec![
            ("quick_start", quick_start(60)),
            ("returning_welcome", returning_welcome("1 token")),
            ("no_admins", NO_ADMINS_WARNING.into()),
            ("help", HELP.into()),
            ("help_admin", HELP_ADMIN.into()),
            ("admin_panel", ADMIN_PANEL.into()),
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
            ("confirm_token", confirm_token("USDC", "MINT")),
            ("confirm_wallet", confirm_wallet("Treasury", "ADDR")),
            ("token_saved", token_saved("USDC (MINT)", 1)),
            ("wallet_saved", wallet_saved("Treasury (ADDR)", 1)),
            ("cancelled_nothing_added", CANCELLED_NOTHING_ADDED.into()),
            ("nothing_to_alert_on", NOTHING_TO_ALERT_ON.into()),
            ("ask_alert_kind", ask_alert_kind(2, 1)),
            ("bad_alert_kind", BAD_ALERT_KIND.into()),
            (
                "ask_which_target",
                ask_which_target(&["1. USDC".to_string()]),
            ),
            ("bad_target_number", BAD_TARGET_NUMBER.into()),
            ("ask_operator", ask_operator("price", "USD", "250", "0.99")),
            ("bad_operator", BAD_OPERATOR.into()),
            ("ask_percent", ASK_PERCENT.into()),
            ("ask_threshold", ask_threshold("1.5", "USD")),
            ("bad_threshold", BAD_THRESHOLD.into()),
            ("threshold_too_big", THRESHOLD_TOO_BIG.into()),
            ("ask_cooldown", ask_cooldown(300)),
            ("bad_cooldown", BAD_COOLDOWN.into()),
            (
                "confirm_alert",
                confirm_alert("USDC", "price < 0.99 USD", 300),
            ),
            (
                "alert_saved",
                alert_saved(1, "USDC", "price < 0.99 USD", 60),
            ),
            ("cancelled_no_alert", CANCELLED_NO_ALERT.into()),
        ]
    }

    #[test]
    fn no_message_has_ragged_indentation() {
        // The exact defect this module exists to prevent: a mangled line continuation
        // baking source indentation into the message. Deep indents and runs of spaces
        // mid-sentence are both symptoms.
        for (name, text) in all_copy() {
            for (n, line) in text.lines().enumerate() {
                let indent = line.len() - line.trim_start().len();
                // Two spaces indents a list item; four is the deepest nesting used.
                assert!(
                    indent <= 4,
                    "{name} line {n} is indented {indent}: {line:?}"
                );

                let body = line.trim_start();
                assert!(
                    !body.contains("   ") || body.contains("  "),
                    "{name} line {n}: {line:?}"
                );
            }
        }
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
    fn admin_help_still_fits_one_send_when_concatenated() {
        // `/help` sends HELP and HELP_ADMIN as a single message. Chunking would handle
        // an overflow, but splitting a reference table across two bubbles reads badly,
        // so this is the budget to stay inside when adding to either.
        let combined = format!("{HELP}{HELP_ADMIN}");
        let chunks = crate::telegram::reply::chunk_message(&combined, 3900);
        assert_eq!(
            chunks.len(),
            1,
            "combined help is {} UTF-16 units",
            combined.encode_utf16().count()
        );
    }

    #[test]
    fn no_message_is_wider_than_a_phone_screen() {
        // Telegram wraps, but wrapping a hand-aligned column list destroys it.
        for (name, text) in all_copy() {
            for (n, line) in text.lines().enumerate() {
                assert!(
                    line.chars().count() <= 72,
                    "{name} line {n} is {} chars: {line:?}",
                    line.chars().count()
                );
            }
        }
    }

    #[test]
    fn help_covers_every_operator_and_state_with_examples() {
        for operator in [">", "<", ">=", "<=", "%up", "%down"] {
            assert!(HELP.contains(operator), "{operator} is undocumented");
        }
        // The three words /alerts prints must be explained somewhere.
        for state in ["armed", "firing", "disabled"] {
            assert!(HELP.contains(state), "{state} is unexplained");
        }
        assert!(HELP.contains("e.g."), "no worked examples");
        assert!(HELP.contains("Solana only"));
        assert!(HELP.contains("read-only"));
    }

    #[test]
    fn admin_help_shows_real_arguments_not_placeholders() {
        // "/addadmin <telegram_id>" tells a user nothing about what to type.
        assert!(HELP_ADMIN.contains("/addadmin 123456789"));
        assert!(HELP_ADMIN.contains("@userinfobot"));
        assert!(!HELP_ADMIN.contains('<'));
        assert!(!ADMIN_PANEL.contains('<'));
    }

    #[test]
    fn onboarding_stays_short() {
        // The original welcome was a 25-line command dump.
        assert!(quick_start(60).lines().count() <= 20);
        assert!(returning_welcome("1 token").lines().count() <= 15);
    }

    #[test]
    fn prompts_show_a_concrete_example_of_the_answer() {
        assert!(ASK_MINT.contains("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"));
        assert!(ask_threshold("1.5", "USD").contains("1.5"));
        assert!(ask_operator("price", "USD", "250", "0.99").contains("250"));
        assert!(ASK_PERCENT.contains("10"));
    }

    #[test]
    fn every_step_says_how_to_get_out() {
        for text in [ASK_MINT, ASK_ADDRESS, BAD_ADDRESS] {
            assert!(text.contains("/cancel"), "no escape hatch: {text:?}");
        }
    }

    #[test]
    fn skipping_is_described_by_a_word_not_a_symbol() {
        // `-` is not something anyone guesses.
        assert!(ask_token_name("x").contains("`skip`"));
        assert!(ask_cooldown(300).contains("`skip`"));
        assert!(ASK_SHORT_NAME.contains("`skip`"));
    }
}
