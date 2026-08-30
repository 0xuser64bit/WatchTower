//! Telegram's command menu and menu button.
//!
//! Registers commands with Telegram via `setMyCommands`, which is what populates the
//! autocomplete list users see when they type `/`. The menu button next to the text
//! field is explicitly set to open that command list, so the very first thing a new
//! user can do is tap it and land on `/start`, which opens the inline main menu.
//!
//! Two scopes are used so the menu matches what the caller can actually do:
//!
//! * every private chat gets the everyday commands;
//! * each active admin's own chat additionally gets the admin commands.
//!
//! Failures are logged and never fatal. The menu is a convenience, and a per-chat
//! registration legitimately fails when the user has not opened a chat with the bot.

use crate::db::repos::users::UserRepo;
use crate::db::Db;
use teloxide::prelude::*;
use teloxide::types::{BotCommand, BotCommandScope, MenuButton, Recipient};
use tracing::{debug, warn};

/// Commands shown to everyone, in the order they appear in the menu.
///
/// The list leads with the two that open the whole interface — `start` and `menu` —
/// because navigation is now tap-driven; the rest are shortcuts into individual
/// screens for people who prefer typing.
fn everyday_commands() -> Vec<BotCommand> {
    vec![
        BotCommand::new("menu", "Open the menu"),
        BotCommand::new("start", "Getting started"),
        BotCommand::new("alerts", "Your alerts"),
        BotCommand::new("addalert", "Create an alert"),
        BotCommand::new("tokens", "Tracked tokens"),
        BotCommand::new("favourites", "Starred tokens"),
        BotCommand::new("addtoken", "Track a token"),
        BotCommand::new("wallets", "Tracked wallets"),
        BotCommand::new("addwallet", "Track a wallet"),
        BotCommand::new("history", "Alerts that have fired"),
        BotCommand::new("status", "Is monitoring healthy?"),
        BotCommand::new("help", "How it works"),
        BotCommand::new("cancel", "Stop what we were doing"),
    ]
}

fn admin_commands() -> Vec<BotCommand> {
    let mut commands = everyday_commands();
    commands.extend([
        BotCommand::new("admin", "Admin panel"),
        BotCommand::new("listusers", "Who can use this bot"),
    ]);
    commands
}

/// Publishes the menus and sets the menu button. Called once at startup.
pub async fn publish(bot: &Bot, db: &Db) {
    // Point the menu button at the command list so the paperclip-adjacent button is a
    // real entry point rather than the default.
    if let Err(err) = bot
        .set_chat_menu_button()
        .menu_button(MenuButton::Commands)
        .await
    {
        debug!(%err, "could not set the chat menu button");
    }

    if let Err(err) = bot
        .set_my_commands(everyday_commands())
        .scope(BotCommandScope::AllPrivateChats)
        .await
    {
        warn!(%err, "could not publish the command menu");
        return;
    }

    debug!("published the command menu");

    let admins = match UserRepo::new(db).list_active_admins().await {
        Ok(admins) => admins,
        Err(err) => {
            warn!(%err, "could not load admins to publish their command menu");
            return;
        }
    };

    for admin in admins {
        publish_for_admin(bot, admin.telegram_id, true).await;
    }
}

/// Updates one user's menu after their role changes, so a newly promoted admin sees
/// the admin commands without restarting the daemon and a demoted one stops seeing
/// commands they can no longer use.
pub async fn publish_for_admin(bot: &Bot, telegram_id: i64, is_admin: bool) {
    let commands = if is_admin {
        admin_commands()
    } else {
        everyday_commands()
    };

    let result = bot
        .set_my_commands(commands)
        .scope(BotCommandScope::Chat {
            chat_id: Recipient::Id(ChatId(telegram_id)),
        })
        .await;

    match result {
        Ok(_) => debug!(telegram_id, is_admin, "published a per-user command menu"),
        // Expected when the user has never opened a chat with the bot; their menu is
        // published the next time the daemon starts, after they have.
        Err(err) => debug!(telegram_id, %err, "could not publish a per-user command menu"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::Command;
    use teloxide::utils::command::BotCommands;

    #[test]
    fn every_published_command_exists() {
        // A menu entry that does not parse is worse than no menu: Telegram offers it,
        // the user taps it, and the bot answers "I only understand commands".
        let declared: Vec<String> = Command::bot_commands()
            .into_iter()
            .map(|c| c.command.trim_start_matches('/').to_string())
            .collect();

        for entry in admin_commands() {
            assert!(
                declared.contains(&entry.command),
                "{} is published but not handled",
                entry.command
            );
        }
    }

    #[test]
    fn descriptions_are_short_enough_for_the_menu() {
        for entry in admin_commands() {
            // Telegram rejects descriptions longer than 256 characters, and anything
            // beyond a short phrase is truncated in the UI anyway.
            assert!(
                (1..=64).contains(&entry.description.chars().count()),
                "{}: {:?}",
                entry.command,
                entry.description
            );
            assert!(
                entry.command.chars().all(|c| c.is_ascii_lowercase()),
                "{} must be lowercase to match the parser",
                entry.command
            );
        }
    }

    #[test]
    fn admins_see_everything_a_user_sees() {
        let everyday = everyday_commands();
        let admin = admin_commands();

        assert!(admin.len() > everyday.len());
        for entry in everyday {
            assert!(admin.iter().any(|a| a.command == entry.command));
        }
    }

    #[test]
    fn destructive_commands_are_not_offered_by_the_menu() {
        // Deletions take an id argument, so a tapped menu entry would send a bare
        // command and the user would just get a usage error. They stay in /help.
        let published: Vec<String> = admin_commands().into_iter().map(|c| c.command).collect();

        for command in [
            "deletetoken",
            "deletewallet",
            "deleterule",
            "block",
            "demote",
        ] {
            assert!(
                !published.contains(&command.to_string()),
                "{command} should not be in the tap-to-send menu"
            );
        }
    }
}
