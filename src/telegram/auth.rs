//! Authorization for the Telegram control plane.
//!
//! Identity is the numeric Telegram user id and nothing else: usernames are
//! mutable and are never trusted. The `users` table is the single authority for
//! who may act; the bootstrap ids in configuration only seed it.

use crate::db::repos::users::{AuthUser, Role, UserRepo};
use crate::db::Db;
use crate::error::Result;
use teloxide::types::Message;

/// Outcome of an authorization check. Modelled as a value, not an error, because
/// "this user may not do that" is an expected control-plane result that always has
/// a specific user-facing reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authorization {
    Allowed(AuthUser),
    Denied(DenyReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// Not present in the users table.
    NotRegistered,
    /// Present but blocked by an admin.
    Blocked,
    /// Present and active, but lacks the admin role.
    NotAdmin,
    /// The update carried no sender (e.g. an anonymous channel post).
    NoSender,
}

impl DenyReason {
    pub fn user_message(self) -> &'static str {
        match self {
            // Deliberately identical for unregistered and blocked users: revealing
            // which one applies tells an unknown caller whether an id is known.
            DenyReason::NotRegistered | DenyReason::Blocked | DenyReason::NoSender => {
                "You are not authorized to use this bot."
            }
            DenyReason::NotAdmin => "This action requires admin privileges.",
        }
    }
}

impl Authorization {
    pub fn allowed(&self) -> Option<&AuthUser> {
        match self {
            Authorization::Allowed(user) => Some(user),
            Authorization::Denied(_) => None,
        }
    }

    /// Narrows an existing decision to admins only.
    pub fn require_admin(self) -> Self {
        match self {
            Authorization::Allowed(user) if user.role != Role::Admin => {
                Authorization::Denied(DenyReason::NotAdmin)
            }
            other => other,
        }
    }
}

/// Resolves the sender of `message` against the users table.
pub async fn authorize(db: &Db, message: &Message) -> Result<Authorization> {
    let Some(sender) = message.from() else {
        return Ok(Authorization::Denied(DenyReason::NoSender));
    };

    let telegram_id = sender.id.0 as i64;

    let Some(user) = UserRepo::new(db).find_by_telegram_id(telegram_id).await? else {
        tracing::info!(telegram_id, "rejected update from unregistered user");
        return Ok(Authorization::Denied(DenyReason::NotRegistered));
    };

    if user.blocked {
        tracing::info!(telegram_id, "rejected update from blocked user");
        return Ok(Authorization::Denied(DenyReason::Blocked));
    }

    Ok(Authorization::Allowed(AuthUser {
        telegram_id: user.telegram_id,
        role: user.role,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: Role) -> Authorization {
        Authorization::Allowed(AuthUser {
            telegram_id: 1,
            role,
        })
    }

    #[test]
    fn require_admin_keeps_admins() {
        assert_eq!(user(Role::Admin).require_admin(), user(Role::Admin));
    }

    #[test]
    fn require_admin_denies_plain_users() {
        assert_eq!(
            user(Role::User).require_admin(),
            Authorization::Denied(DenyReason::NotAdmin)
        );
    }

    #[test]
    fn require_admin_preserves_earlier_denial() {
        let denied = Authorization::Denied(DenyReason::Blocked);
        assert_eq!(denied.clone().require_admin(), denied);
    }

    #[test]
    fn unknown_and_blocked_are_indistinguishable_to_the_caller() {
        assert_eq!(
            DenyReason::NotRegistered.user_message(),
            DenyReason::Blocked.user_message()
        );
    }
}
