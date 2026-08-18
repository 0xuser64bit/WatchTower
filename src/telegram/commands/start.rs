use crate::db::Db;
use crate::telegram::auth;
use std::sync::Arc;
use teloxide::prelude::*;

const COMMAND_HELP: &str = "ChainSentinel commands:\n\
/start - main menu\n\
/help - this help\n\
/addtoken - add a token to track\n\
/tokens - list tracked tokens\n\
/deletetoken <id> - delete a tracked token\n\
/addwallet - add a wallet to track\n\
/wallets - list tracked wallets\n\
/deletewallet <id> - delete a tracked wallet\n\
/addalert - create an alert rule\n\
/alerts - list alert rules\n\
/enablerule <id> - enable an alert rule\n\
/disablerule <id> - disable an alert rule\n\
/deleterule <id> - delete an alert rule\n\
/history - show recent alert events";

const ADMIN_COMMAND_HELP: &str = "\n\nAdmin commands:\n\
/admin - show this panel\n\
/listusers - list authorized users\n\
/addadmin <id> - grant admin\n\
/demote <id> - revoke admin\n\
/block <id> - block user\n\
/unblock <id> - unblock user";

pub async fn start(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = match auth::authorize_or_send(&bot, &db, &msg).await {
        Some(ctx) => ctx,
        None => return Ok(()),
    };

    bot.send_message(
        msg.chat.id,
        format!(
            "Welcome to ChainSentinel, Telegram ID {}.\n\n{COMMAND_HELP}",
            ctx.user.telegram_id
        ),
    )
    .await?;

    Ok(())
}

pub async fn help(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = match auth::authorize_or_send(&bot, &db, &msg).await {
        Some(ctx) => ctx,
        None => return Ok(()),
    };

    let mut text = COMMAND_HELP.to_string();

    if ctx.user.role() == crate::db::repos::users::Role::Admin {
        text.push_str(ADMIN_COMMAND_HELP);
    }

    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}
