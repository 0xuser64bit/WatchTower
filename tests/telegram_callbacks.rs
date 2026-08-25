//! End-to-end tests for the tap-driven interface.
//!
//! These drive the real handler tree with callback-query updates (button taps),
//! sharing one dialogue with typed messages, and assert on the resulting database
//! state — so a routing, authorization, or state-machine regression in the new UX is
//! caught here.

mod support;

use mockito::Matcher;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use watchtower::db::repos::rules::RuleRepo;
use watchtower::db::repos::tokens::TokenRepo;
use watchtower::db::repos::users::{Role, UserRepo};
use watchtower::rules::types::{Operator, RuleState};
use watchtower::telegram::flows::DialogueState;

const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// A session that can both tap buttons and type, against a server that accepts every
/// Telegram method the UI uses.
struct Session {
    _server: mockito::ServerGuard,
    state: watchtower::app_state::AppState,
    storage: Arc<InMemStorage<DialogueState>>,
}

impl Session {
    async fn new() -> Self {
        let mut server = mockito::Server::new_async().await;

        for method in ["[sS]endMessage", "[eE]ditMessageText"] {
            server
                .mock("POST", Matcher::Regex(format!(r"^/bot.+/{method}$")))
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(support::SEND_MESSAGE_OK)
                .expect_at_least(0)
                .create_async()
                .await;
        }
        for method in [
            "[aA]nswerCallbackQuery",
            "[sS]etMyCommands",
            "[sS]etChatMenuButton",
        ] {
            server
                .mock("POST", Matcher::Regex(format!(r"^/bot.+/{method}$")))
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(support::TRUE_OK)
                .expect_at_least(0)
                .create_async()
                .await;
        }

        let db = support::database().await;
        let price = Arc::new(support::FakePriceProvider::with_price(MINT, 1.0));
        let chain = Arc::new(support::FakeChainProvider::new());
        let state = support::app_state(db, &server.url(), price, chain);

        Self {
            _server: server,
            state,
            storage: InMemStorage::new(),
        }
    }

    async fn tap(&self, data: &str) {
        support::dispatch(&self.state, self.storage.clone(), support::callback(data))
            .await
            .unwrap_or_else(|err| panic!("tapping {data:?} failed: {err}"));
    }

    async fn send(&self, text: &str) {
        support::dispatch(&self.state, self.storage.clone(), support::message(text))
            .await
            .unwrap_or_else(|err| panic!("sending {text:?} failed: {err}"));
    }

    async fn dialogue_state(&self) -> DialogueState {
        teloxide::dispatching::dialogue::Storage::get_dialogue(
            self.storage.clone(),
            teloxide::types::ChatId(support::CHAT_ID),
        )
        .await
        .expect("storage read")
        .unwrap_or_default()
    }
}

#[tokio::test]
async fn an_alert_can_be_created_entirely_by_tapping() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    // Menu → Create Alert → Token → the token → Below → (type value) → default
    // cooldown → confirm.
    session.tap("ac:new").await;
    session.tap("ac:k:t").await;
    session.tap(&format!("ac:tg:{}", token.id)).await;
    session.tap("ac:op:lt").await;
    session.send("0.99").await; // the one value that must be typed
    session.tap("ac:cd").await;
    session.tap("ac:ok").await;

    let rules = RuleRepo::new(&session.state.db).list_all().await.unwrap();
    assert_eq!(rules.len(), 1, "exactly one rule should have been created");
    assert_eq!(rules[0].target.id, token.id);
    assert_eq!(rules[0].operator, Operator::Lt);
    assert_eq!(rules[0].threshold, 0.99);
    assert_eq!(
        rules[0].cooldown_seconds,
        session.state.settings.alert_default_cooldown_seconds
    );
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn back_button_steps_the_alert_flow_backwards() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session.tap("ac:new").await;
    session.tap("ac:k:t").await;
    session.tap(&format!("ac:tg:{}", token.id)).await;
    // On the operator step; step back to the target step.
    session.tap("ac:bk").await;

    assert!(matches!(
        session.dialogue_state().await,
        DialogueState::AddAlert(
            watchtower::telegram::flows::add_alert::Step::AwaitingTarget { .. }
        )
    ));
}

#[tokio::test]
async fn cancelling_a_flow_by_button_clears_it() {
    let session = Session::new().await;
    TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session.tap("ac:new").await;
    session.tap("ac:k:t").await;
    session.tap("x").await; // Cancel

    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
    assert!(RuleRepo::new(&session.state.db)
        .list_all()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn an_alert_can_be_toggled_and_deleted_by_tapping() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();
    let rule = RuleRepo::new(&session.state.db)
        .create(
            watchtower::db::repos::rules::NewRuleTarget::Token { id: token.id },
            Operator::Lt,
            0.99,
            300,
        )
        .await
        .unwrap();

    // Open the detail, disable it.
    session.tap(&format!("al:v:{}", rule.id)).await;
    session.tap(&format!("al:t:{}", rule.id)).await;
    assert!(
        !RuleRepo::new(&session.state.db)
            .find(rule.id)
            .await
            .unwrap()
            .unwrap()
            .enabled,
        "toggling should have disabled the rule"
    );

    // Re-enable it: an enable is an explicit re-arm.
    session.tap(&format!("al:t:{}", rule.id)).await;
    let re_enabled = RuleRepo::new(&session.state.db)
        .find(rule.id)
        .await
        .unwrap()
        .unwrap();
    assert!(re_enabled.enabled);
    assert_eq!(re_enabled.state, RuleState::Ok);

    // Delete it: the confirm step, then the confirmation.
    session.tap(&format!("al:d:{}", rule.id)).await;
    session.tap(&format!("al:dy:{}", rule.id)).await;
    assert!(RuleRepo::new(&session.state.db)
        .list_all()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_token_can_be_added_by_tapping_through_the_flow() {
    let session = Session::new().await;

    session.tap("at:new").await;
    session.send(MINT).await; // paste the address
    session.tap("at:sk").await; // skip naming
    session.tap("at:ok").await; // confirm

    let tokens = TokenRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].mint_address, MINT);
    assert_eq!(tokens[0].symbol, None);
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn an_admin_can_be_added_through_the_guided_flow() {
    let session = Session::new().await;

    // Admin Panel → Add Admin → type the id → confirm.
    session.tap("ad:add").await;
    session.send("555").await;
    session.tap("ad:addok").await;

    let user = UserRepo::new(&session.state.db)
        .find_by_telegram_id(555)
        .await
        .unwrap()
        .expect("user should have been created");
    assert_eq!(user.role, Role::Admin);
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn a_non_admin_tapping_an_admin_button_is_refused() {
    let session = Session::new().await;
    UserRepo::new(&session.state.db)
        .upsert(444, Role::User)
        .await
        .unwrap();

    // A plain user taps the admin "add admin" button; nothing must happen.
    support::dispatch(
        &session.state,
        session.storage.clone(),
        support::callback_from(444, "ad:add"),
    )
    .await
    .expect("dispatch");

    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn a_blocked_user_cannot_drive_a_flow_by_tapping() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session.tap("ac:new").await;
    session.tap("ac:k:t").await;

    // Revoked mid-flow. Every subsequent tap must be refused.
    UserRepo::new(&session.state.db)
        .set_blocked(support::ADMIN_ID, true)
        .await
        .unwrap();

    session.tap(&format!("ac:tg:{}", token.id)).await;
    session.tap("ac:op:lt").await;

    assert!(
        RuleRepo::new(&session.state.db)
            .list_all()
            .await
            .unwrap()
            .is_empty(),
        "a blocked user must not be able to continue a flow by tapping"
    );
}
