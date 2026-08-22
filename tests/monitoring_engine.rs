//! End-to-end tests for the monitoring data plane.
//!
//! Drives the real `engine::scheduler::tick` against a real (in-memory) database, a
//! mock Telegram API, and scripted providers, so evaluation, persistence, and
//! delivery are verified together.

mod support;

use mockito::Matcher;
use std::sync::Arc;
use watchtower::db::repos::alert_events::AlertEventRepo;
use watchtower::db::repos::rules::{NewRuleTarget, RuleRepo};
use watchtower::db::repos::tokens::TokenRepo;
use watchtower::db::repos::wallets::WalletRepo;
use watchtower::engine::scheduler;
use watchtower::rules::types::{Operator, RuleState};

const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WALLET: &str = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

struct Harness {
    server: mockito::ServerGuard,
    state: watchtower::app_state::AppState,
    price: Arc<support::FakePriceProvider>,
    chain: Arc<support::FakeChainProvider>,
}

async fn harness() -> Harness {
    let mut server = mockito::Server::new_async().await;

    server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect_at_least(0)
        .create_async()
        .await;

    let db = support::database().await;
    let price = Arc::new(support::FakePriceProvider::new());
    let chain = Arc::new(support::FakeChainProvider::new());
    let state = support::app_state(db, &server.url(), price.clone(), chain.clone());

    Harness {
        server,
        state,
        price,
        chain,
    }
}

#[tokio::test]
async fn a_threshold_breach_notifies_exactly_once_while_it_persists() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    let rule = RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            0,
        )
        .await
        .unwrap();

    // First cycle: crosses the threshold and alerts.
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.rules_evaluated, 1);
    assert_eq!(report.alerts_sent, 1);

    let stored = RuleRepo::new(&h.state.db)
        .find(rule.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.state, RuleState::Firing);
    assert_eq!(stored.last_value, Some(150.0));
    assert!(stored.last_triggered_at.is_some());

    // Ten further cycles with the condition still true produce no more alerts.
    for _ in 0..10 {
        let report = scheduler::tick(&h.state).await.unwrap();
        assert_eq!(report.alerts_sent, 0);
    }

    assert_eq!(AlertEventRepo::new(&h.state.db).count().await.unwrap(), 1);
}

#[tokio::test]
async fn a_rule_rearms_after_recovering_and_can_fire_again() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    let rule = RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            0,
        )
        .await
        .unwrap();

    scheduler::tick(&h.state).await.unwrap();

    // Recover below the threshold.
    h.price.set(MINT, Ok(50.0));
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.alerts_sent, 0);
    assert_eq!(
        RuleRepo::new(&h.state.db)
            .find(rule.id)
            .await
            .unwrap()
            .unwrap()
            .state,
        RuleState::Ok
    );

    // Breach again: a genuine new edge, so it must alert.
    h.price.set(MINT, Ok(200.0));
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.alerts_sent, 1);
    assert_eq!(AlertEventRepo::new(&h.state.db).count().await.unwrap(), 2);
}

#[tokio::test]
async fn cooldown_suppresses_a_rapidly_flapping_condition() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            3_600,
        )
        .await
        .unwrap();

    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);

    // Flap down and back up inside the cooldown window.
    h.price.set(MINT, Ok(50.0));
    scheduler::tick(&h.state).await.unwrap();
    h.price.set(MINT, Ok(150.0));

    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 0);
    assert_eq!(AlertEventRepo::new(&h.state.db).count().await.unwrap(), 1);
}

#[tokio::test]
async fn a_percentage_rule_rebaselines_after_each_alert() {
    let h = harness().await;
    h.price.set(MINT, Ok(100.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    let rule = RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::PctUp,
            10.0,
            0,
        )
        .await
        .unwrap();

    // First cycle only establishes the baseline.
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.alerts_sent, 0);
    let stored = RuleRepo::new(&h.state.db)
        .find(rule.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.reference_value, Some(100.0));

    // +10% fires and re-baselines to the new value.
    h.price.set(MINT, Ok(110.0));
    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);
    let stored = RuleRepo::new(&h.state.db)
        .find(rule.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.reference_value, Some(110.0));

    // Same absolute price: no further move relative to the new baseline.
    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 0);

    // Another +10% from the new 110 baseline fires again.
    h.price.set(MINT, Ok(121.0));
    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);
    assert_eq!(
        RuleRepo::new(&h.state.db)
            .find(rule.id)
            .await
            .unwrap()
            .unwrap()
            .reference_value,
        Some(121.0)
    );
}

#[tokio::test]
async fn history_records_a_readable_snapshot_that_outlives_its_rule() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();
    RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            0,
        )
        .await
        .unwrap();

    scheduler::tick(&h.state).await.unwrap();

    // Deleting the token cascades to the rule; history must survive and stay readable.
    TokenRepo::new(&h.state.db).delete(token.id).await.unwrap();

    let events = AlertEventRepo::new(&h.state.db)
        .list_recent(10)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].rule_id, None);
    assert_eq!(events[0].target_ref, MINT);
    assert_eq!(events[0].target_label.as_deref(), Some("USDC"));
    assert_eq!(events[0].observed_value, 150.0);
    assert_eq!(events[0].threshold_value, 100.0);

    assert!(RuleRepo::new(&h.state.db)
        .list_all()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn one_failing_target_does_not_stop_the_other_rules() {
    let h = harness().await;

    let good = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    let bad = TokenRepo::new(&h.state.db)
        .create("BadMint1111111111111111111111111111111111111", None)
        .await
        .unwrap();

    h.price.set(MINT, Ok(150.0));
    h.price.set(
        "BadMint1111111111111111111111111111111111111",
        Err(watchtower::providers::ProviderError::Unavailable(
            "down".into(),
        )),
    );

    for token_id in [bad.id, good.id] {
        RuleRepo::new(&h.state.db)
            .create(
                NewRuleTarget::Token { id: token_id },
                Operator::Gt,
                100.0,
                0,
            )
            .await
            .unwrap();
    }

    // The provider failure is isolated, so the healthy rule is still evaluated.
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.rules_evaluated, 1, "healthy rule must still run");
    assert_eq!(report.alerts_sent, 1);
    assert_eq!(report.targets_unavailable, 1);
}

#[tokio::test]
async fn an_unreadable_target_does_not_rearm_a_firing_rule() {
    let h = harness().await;
    h.chain.set(WALLET, LAMPORTS_PER_SOL);

    let wallet = WalletRepo::new(&h.state.db)
        .create(WALLET, Some("Treasury"))
        .await
        .unwrap();
    let rule = RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Wallet { id: wallet.id },
            Operator::Lt,
            5.0,
            0,
        )
        .await
        .unwrap();

    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);

    // RPC outage: the rule must hold its firing state rather than being treated as
    // recovered, which would produce a duplicate alert once RPC returns.
    h.chain.fail_with("rpc down");
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.rules_evaluated, 0);
    assert_eq!(
        RuleRepo::new(&h.state.db)
            .find(rule.id)
            .await
            .unwrap()
            .unwrap()
            .state,
        RuleState::Firing
    );

    h.chain.clear_failure();
    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 0);
    assert_eq!(AlertEventRepo::new(&h.state.db).count().await.unwrap(), 1);
}

#[tokio::test]
async fn wallet_balances_are_read_in_one_batched_call() {
    let h = harness().await;

    let addresses = [
        "So11111111111111111111111111111111111111112",
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    ];

    for address in addresses {
        h.chain.set(address, 42 * LAMPORTS_PER_SOL);
        let wallet = WalletRepo::new(&h.state.db)
            .create(address, None)
            .await
            .unwrap();
        RuleRepo::new(&h.state.db)
            .create(
                NewRuleTarget::Wallet { id: wallet.id },
                Operator::Lt,
                1.0,
                0,
            )
            .await
            .unwrap();
    }

    scheduler::tick(&h.state).await.unwrap();

    // Three wallets, one RPC round-trip.
    assert_eq!(h.chain.batch_call_count(), 1);
}

#[tokio::test]
async fn several_rules_on_one_token_share_a_single_price_lookup() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();

    for threshold in [10.0, 20.0, 30.0] {
        RuleRepo::new(&h.state.db)
            .create(
                NewRuleTarget::Token { id: token.id },
                Operator::Gt,
                threshold,
                0,
            )
            .await
            .unwrap();
    }

    scheduler::tick(&h.state).await.unwrap();
    assert_eq!(h.price.call_count(), 1);
}

#[tokio::test]
async fn disabled_rules_are_not_evaluated_and_rearm_when_re_enabled() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    let repo = RuleRepo::new(&h.state.db);
    let rule = repo
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            0,
        )
        .await
        .unwrap();

    scheduler::tick(&h.state).await.unwrap();
    assert_eq!(
        repo.find(rule.id).await.unwrap().unwrap().state,
        RuleState::Firing
    );

    repo.set_enabled(rule.id, false).await.unwrap();
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.rules_evaluated, 0);

    // Re-enabling clears the latched firing state, so a still-true condition alerts
    // again rather than staying permanently silent.
    let reenabled = repo.set_enabled(rule.id, true).await.unwrap();
    assert_eq!(reenabled.state, RuleState::Ok);
    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);
}

#[tokio::test]
async fn alerts_are_recorded_even_when_no_admin_can_receive_them() {
    use watchtower::db::repos::users::UserRepo;

    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    RuleRepo::new(&h.state.db)
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            0,
        )
        .await
        .unwrap();

    UserRepo::new(&h.state.db)
        .set_blocked(support::ADMIN_ID, true)
        .await
        .unwrap();

    // Delivery has no recipients, but the firing must still be auditable rather than
    // disappearing, and the rule must still latch so it does not alert every tick.
    let report = scheduler::tick(&h.state).await.unwrap();
    assert_eq!(report.alerts_sent, 1);
    assert_eq!(AlertEventRepo::new(&h.state.db).count().await.unwrap(), 1);

    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 0);
}

#[tokio::test]
async fn history_pruning_respects_the_retention_window() {
    let h = harness().await;
    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    let rule = RuleRepo::new(&h.state.db)
        .create(NewRuleTarget::Token { id: token.id }, Operator::Gt, 1.0, 0)
        .await
        .unwrap();

    let repo = AlertEventRepo::new(&h.state.db);
    let old = chrono::Utc::now() - chrono::Duration::days(120);
    let recent = chrono::Utc::now();

    repo.record(&rule, 2.0, Some(1.0), old).await.unwrap();
    repo.record(&rule, 3.0, Some(1.0), recent).await.unwrap();

    let removed = repo.prune_older_than_days(90).await.unwrap();
    assert_eq!(removed, 1);
    assert_eq!(repo.count().await.unwrap(), 1);

    drop(h.server);
}

#[tokio::test]
async fn re_enabling_a_rule_clears_its_cooldown_so_the_next_breach_alerts() {
    let h = harness().await;
    h.price.set(MINT, Ok(150.0));

    let token = TokenRepo::new(&h.state.db)
        .create(MINT, None)
        .await
        .unwrap();
    let repo = RuleRepo::new(&h.state.db);
    // A long cooldown, so only an explicit reset can allow a second alert.
    let rule = repo
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            3_600,
        )
        .await
        .unwrap();

    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);

    // Toggling a rule off and on is an explicit request to re-arm it. Keeping the old
    // trigger time would latch it straight back to firing and swallow the alert.
    repo.set_enabled(rule.id, false).await.unwrap();
    let reenabled = repo.set_enabled(rule.id, true).await.unwrap();
    assert_eq!(reenabled.last_triggered_at, None);
    assert_eq!(reenabled.state, RuleState::Ok);

    assert_eq!(scheduler::tick(&h.state).await.unwrap().alerts_sent, 1);
    assert_eq!(AlertEventRepo::new(&h.state.db).count().await.unwrap(), 2);
}

/// The timer loop itself: that it keeps polling on schedule, records health, and stops
/// promptly on cancellation rather than waiting out the current interval.
#[tokio::test]
async fn the_monitoring_loop_polls_on_schedule_and_stops_on_cancellation() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use watchtower::app_state::AppState;

    let db = support::database().await;
    let shutdown = CancellationToken::new();

    // A real, very short interval. Configuration enforces a 10s floor to protect the
    // providers, which is bypassed here deliberately: no rules exist, so a poll makes
    // no outbound requests at all.
    let mut settings = support::settings(&[]);
    settings.poll_interval = Duration::from_millis(50);

    let state = AppState::new(
        db,
        teloxide::Bot::new(support::BOT_TOKEN),
        Arc::new(settings),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
        shutdown.clone(),
    );

    let status = state.status.clone();
    let loop_handle = tokio::spawn(async move { watchtower::engine::scheduler::run(state).await });

    tokio::time::sleep(Duration::from_millis(400)).await;

    let snapshot = status.snapshot();
    assert!(
        snapshot.ticks_completed >= 4,
        "expected repeated polls, got {}",
        snapshot.ticks_completed
    );
    assert_eq!(snapshot.consecutive_failures, 0);
    assert!(snapshot.started_at.is_some());
    assert!(snapshot.last_tick_at.is_some());
    assert!(snapshot.is_healthy(Duration::from_millis(50)));

    shutdown.cancel();

    tokio::time::timeout(Duration::from_secs(2), loop_handle)
        .await
        .expect("monitoring loop did not stop on cancellation")
        .expect("monitoring loop panicked");
}
