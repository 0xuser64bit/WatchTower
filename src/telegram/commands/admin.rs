use crate::db::repos::users::{Role, UserRepo};
use crate::db::Db;
use crate::telegram::reply;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn admin_menu(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_admin(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let text = "Admin panel:\n/listusers - list authorized users\n/addadmin <id> - grant admin\n/demote <id> - revoke admin\n/block <id> - block user\n/unblock <id> - unblock user";

    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

pub async fn list_users(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_admin(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let users = UserRepo::new(&db).list().await?;
    let text = users
        .iter()
        .map(|user| {
            let blocked = if user.blocked { " (blocked)" } else { "" };
            format!("{}: {}{blocked}", user.telegram_id, user.role)
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(&bot, msg.chat.id, format!("Users:\n{text}")).await?;
    Ok(())
}

pub async fn add_admin(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_admin(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            bot.send_message(msg.chat.id, "Usage: /addadmin <telegram_id>")
                .await?;
            return Ok(());
        }
    };

    UserRepo::new(&db).upsert(target_id, Role::Admin).await?;
    bot.send_message(msg.chat.id, "User promoted to admin.")
        .await?;

    Ok(())
}

pub async fn demote_user(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_admin(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            bot.send_message(msg.chat.id, "Usage: /demote <telegram_id>")
                .await?;
            return Ok(());
        }
    };

    UserRepo::new(&db).set_role(target_id, Role::User).await?;
    bot.send_message(msg.chat.id, "User demoted.").await?;
    Ok(())
}

pub async fn block_user(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_admin(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            bot.send_message(msg.chat.id, "Usage: /block <telegram_id>")
                .await?;
            return Ok(());
        }
    };

    UserRepo::new(&db).set_blocked(target_id, true).await?;
    bot.send_message(msg.chat.id, "User blocked.").await?;
    Ok(())
}

pub async fn unblock_user(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_admin(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            bot.send_message(msg.chat.id, "Usage: /unblock <telegram_id>")
                .await?;
            return Ok(());
        }
    };

    UserRepo::new(&db).set_blocked(target_id, false).await?;
    bot.send_message(msg.chat.id, "User unblocked.").await?;
    Ok(())
}
