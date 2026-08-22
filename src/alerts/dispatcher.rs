//! Alert delivery.
//!
//! Recipients come from the `users` table only: active admins. Configuration seeds
//! missing users at startup; it is not a parallel authorization path.

use crate::alerts::format;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::users::UserRepo;
use crate::db::Db;
use crate::error::Result;
use crate::rules::eval::Decision;
use crate::rules::types::Rule;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tracing::{info, warn};

/// Outcome of attempting to deliver one alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub event_id: i64,
    pub delivered: usize,
    pub failed: usize,
}

pub struct AlertDispatcher {
    bot: Bot,
    db: Arc<Db>,
}

impl AlertDispatcher {
    pub fn new(bot: Bot, db: Arc<Db>) -> Self {
        Self { bot, db }
    }

    /// Records the alert, then delivers it to every active admin.
    ///
    /// The event is persisted **before** delivery is attempted. A crash between the
    /// two produces a recorded alert that was not sent, which is recoverable and
    /// visible in `/history`; the reverse order would let a rule notify repeatedly
    /// with no audit trail and no cooldown anchor.
    pub async fn dispatch(
        &self,
        rule: &Rule,
        decision: &Decision,
        at: DateTime<Utc>,
    ) -> Result<Option<Delivery>> {
        let Decision::Notify {
            observed,
            reference,
        } = decision
        else {
            return Ok(None);
        };

        let Some(message) = format::alert_message(rule, decision, at) else {
            return Ok(None);
        };

        let event_id = AlertEventRepo::new(&self.db)
            .record(rule, *observed, *reference, at)
            .await?;

        let recipients = UserRepo::new(&self.db).list_active_admins().await?;

        if recipients.is_empty() {
            // Not an error: the operator may have deliberately blocked everyone. It
            // must still be loud, because alerting is silently non-functional.
            warn!(
                rule_id = rule.id,
                event_id, "alert recorded but no active admin is available to receive it"
            );
            return Ok(Some(Delivery {
                event_id,
                delivered: 0,
                failed: 0,
            }));
        }

        let mut delivered = 0;
        let mut failed = 0;

        for admin in recipients {
            match self
                .bot
                .send_message(ChatId(admin.telegram_id), &message)
                .await
            {
                Ok(_) => delivered += 1,
                Err(err) => {
                    failed += 1;
                    warn!(
                        telegram_id = admin.telegram_id,
                        rule_id = rule.id,
                        %err,
                        "failed to deliver alert to admin"
                    );
                }
            }
        }

        info!(
            rule_id = rule.id,
            event_id, delivered, failed, "alert dispatched"
        );

        Ok(Some(Delivery {
            event_id,
            delivered,
            failed,
        }))
    }
}
