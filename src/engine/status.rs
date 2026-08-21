//! Runtime health of the monitoring engine.
//!
//! Deliberately in-process rather than an external metrics system: this is a
//! single-binary daemon and the operator's interface is Telegram. It exists so the
//! two questions that matter can be answered without reading log files — *is the
//! engine actually polling?* and *why did an alert not fire?*

use chrono::{DateTime, Utc};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Result of one monitoring tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickReport {
    pub rules_evaluated: usize,
    pub alerts_sent: usize,
    /// Targets whose value could not be read this tick.
    pub targets_unavailable: usize,
}

#[derive(Debug, Clone, Default)]
struct Inner {
    started_at: Option<DateTime<Utc>>,
    last_tick_at: Option<DateTime<Utc>>,
    last_tick_duration: Option<Duration>,
    last_report: TickReport,
    ticks_completed: u64,
    alerts_sent_total: u64,
    consecutive_failures: u32,
    last_error: Option<String>,
    last_error_at: Option<DateTime<Utc>>,
    price_provider_healthy: Option<bool>,
    chain_provider_healthy: Option<bool>,
}

/// Point-in-time copy, so readers never hold the lock while formatting.
#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub started_at: Option<DateTime<Utc>>,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub last_tick_duration: Option<Duration>,
    pub last_report: TickReport,
    pub ticks_completed: u64,
    pub alerts_sent_total: u64,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub price_provider_healthy: Option<bool>,
    pub chain_provider_healthy: Option<bool>,
}

impl EngineSnapshot {
    /// Whether the engine looks healthy enough to be trusted with alerting.
    pub fn is_healthy(&self, poll_interval: Duration) -> bool {
        if self.consecutive_failures > 0 {
            return false;
        }

        let Some(last_tick_at) = self.last_tick_at else {
            // Not yet completed a tick; healthy only if we started very recently.
            return self.started_at.is_some_and(|started| {
                Utc::now().signed_duration_since(started).num_seconds()
                    < (poll_interval.as_secs() as i64) * 2 + 30
            });
        };

        // Allow two missed intervals before declaring the loop stalled.
        let stale_after = (poll_interval.as_secs() as i64) * 2 + 30;
        Utc::now().signed_duration_since(last_tick_at).num_seconds() <= stale_after
    }
}

#[derive(Clone, Default)]
pub struct EngineStatus {
    inner: Arc<RwLock<Inner>>,
}

impl EngineStatus {
    pub fn new() -> Self {
        Self::default()
    }

    /// A poisoned lock would mean a writer panicked mid-update. The data is plain
    /// counters, so recovering is strictly better than propagating a panic into
    /// every future status read.
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn mark_started(&self) {
        self.write().started_at = Some(Utc::now());
    }

    pub fn record_tick(&self, report: TickReport, duration: Duration) {
        let mut inner = self.write();
        inner.last_tick_at = Some(Utc::now());
        inner.last_tick_duration = Some(duration);
        inner.last_report = report;
        inner.ticks_completed += 1;
        inner.alerts_sent_total += report.alerts_sent as u64;
        inner.consecutive_failures = 0;
    }

    pub fn record_tick_failure(&self, error: impl std::fmt::Display) {
        let mut inner = self.write();
        inner.consecutive_failures += 1;
        inner.last_error = Some(error.to_string());
        inner.last_error_at = Some(Utc::now());
    }

    pub fn set_price_provider_healthy(&self, healthy: bool) {
        self.write().price_provider_healthy = Some(healthy);
    }

    pub fn set_chain_provider_healthy(&self, healthy: bool) {
        self.write().chain_provider_healthy = Some(healthy);
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        EngineSnapshot {
            started_at: inner.started_at,
            last_tick_at: inner.last_tick_at,
            last_tick_duration: inner.last_tick_duration,
            last_report: inner.last_report,
            ticks_completed: inner.ticks_completed,
            alerts_sent_total: inner.alerts_sent_total,
            consecutive_failures: inner.consecutive_failures,
            last_error: inner.last_error.clone(),
            last_error_at: inner.last_error_at,
            price_provider_healthy: inner.price_provider_healthy,
            chain_provider_healthy: inner.chain_provider_healthy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_tick_clears_the_failure_streak() {
        let status = EngineStatus::new();
        status.record_tick_failure("database gone");
        status.record_tick_failure("database still gone");
        assert_eq!(status.snapshot().consecutive_failures, 2);

        status.record_tick(TickReport::default(), Duration::from_millis(5));
        assert_eq!(status.snapshot().consecutive_failures, 0);
        assert_eq!(status.snapshot().ticks_completed, 1);
    }

    #[test]
    fn alert_totals_accumulate_across_ticks() {
        let status = EngineStatus::new();
        for _ in 0..3 {
            status.record_tick(
                TickReport {
                    rules_evaluated: 5,
                    alerts_sent: 2,
                    targets_unavailable: 0,
                },
                Duration::from_millis(1),
            );
        }

        let snapshot = status.snapshot();
        assert_eq!(snapshot.alerts_sent_total, 6);
        assert_eq!(snapshot.last_report.rules_evaluated, 5);
    }

    #[test]
    fn health_requires_a_recent_tick() {
        let interval = Duration::from_secs(60);

        let status = EngineStatus::new();
        // Never started: not healthy.
        assert!(!status.snapshot().is_healthy(interval));

        status.mark_started();
        // Started but no tick yet: healthy during the grace period.
        assert!(status.snapshot().is_healthy(interval));

        status.record_tick(TickReport::default(), Duration::from_millis(1));
        assert!(status.snapshot().is_healthy(interval));

        status.record_tick_failure("boom");
        assert!(!status.snapshot().is_healthy(interval));
    }

    #[test]
    fn a_stalled_loop_is_unhealthy() {
        let status = EngineStatus::new();
        status.mark_started();
        status.record_tick(TickReport::default(), Duration::from_millis(1));

        // Backdate the last tick well beyond two intervals.
        status.write().last_tick_at = Some(Utc::now() - chrono::Duration::hours(1));

        assert!(!status.snapshot().is_healthy(Duration::from_secs(60)));
    }
}
