use crate::db::repos::users::{Role, UserRepo};
use crate::db::Db;
use crate::telegram::auth;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn admin_menu(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    if auth::require_admin(&ctx).is_err() {
        auth::send_forbidden(&bot, msg.chat.id).await;
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
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    if auth::require_admin(&ctx).is_err() {
        auth::send_forbidden(&bot, msg.chat.id).await;
        return Ok(());
    }

    let users = UserRepo::new(&db).list().await?;
    let text = users
        .iter()
        .map(|user| {
            let blocked = if user.is_blocked() { " (blocked)" } else { "" };
            format!("{}: {}{blocked}", user.telegram_id, user.role)
        })
        .collect::<Vec<_>>()
        .join("\n");

    bot.send_message(msg.chat.id, format!("Users:\n{text}")).await?;
    Ok(())
}

pub async fn add_admin(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    if auth::require_admin(&ctx).is_err() {
        auth::send_forbidden(&bot, msg.chat.id).await;
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            bot.send_message(msg.chat.id, "Usage: /addadmin <telegram_id>").await?;
            return Ok(());
        }
    };

    let repo = UserRepo::new(&db);
    match repo.find_by_telegram_id(target_id).await? {
        Some(_) => {
            repo.set_role(target_id, Role::Admin).await?;
            bot.send_message(msg.chat.id, "User promoted to admin.").await?;
        }
        None => {
            repo.create(target_id, Role::Admin).await?;
            bot.send_message(msg.chat.id, "Admin user created.").await?;
        }
    }

    Ok(())
}

pub async fn demote_user(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    if auth::require_admin(&ctx).is_err() {
        auth::send_forbidden(&bot, msg.chat.id).await;
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            bot.send_message(msg.chat.id, "Usage: /demote <telegram_id>").await?;
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
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    if auth::require_admin(&ctx).is_err() {
        auth::send_forbidden(&bot, msg.chat.id).await;
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            bot.send_message(msg.chat.id, "Usage: /block <telegram_id>").await?;
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
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    if auth::require_admin(&ctx).is_err() {
        auth::send_forbidden(&bot, msg.chat.id).await;
        return Ok(());
    }

    let target_id = match args.trim().parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            bot.send_message(msg.chat.id, "Usage: /unblock <telegram_id>").await?;
            return Ok(());
        }
    };

    UserRepo::new(&db).set_blocked(target_id, false).await?;
    bot.send_message(msg.chat.id, "User unblocked.").await?;
    Ok(())
}
