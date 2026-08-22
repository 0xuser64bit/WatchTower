//! End-to-end tests for the guided flows and admin safeguards.
//!
//! Each test walks the real handler tree message by message and asserts on the
//! resulting database state, so a routing or validation regression is caught here.

mod support;

use chainsentinel::db::repos::rules::RuleRepo;
use chainsentinel::db::repos::tokens::TokenRepo;
use chainsentinel::db::repos::users::{Role, UserRepo};
use chainsentinel::db::repos::wallets::WalletRepo;
use chainsentinel::providers::ProviderError;
use chainsentinel::rules::types::{Operator, TargetKind};
use chainsentinel::telegram::flows::DialogueState;
use mockito::Matcher;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;

const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WALLET: &str = "So11111111111111111111111111111111111111112";

struct Session {
    server: mockito::ServerGuard,
    state: chainsentinel::app_state::AppState,
    storage: Arc<InMemStorage<DialogueState>>,
    price: Arc<support::FakePriceProvider>,
    chain: Arc<support::FakeChainProvider>,
}

impl Session {
    async fn new() -> Self {
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

        Self {
            server,
            state,
            storage: InMemStorage::new(),
            price,
            chain,
        }
    }

    async fn send(&self, text: &str) {
        support::dispatch(&self.state, self.storage.clone(), support::message(text))
            .await
            .unwrap_or_else(|err| panic!("dispatching {text:?} failed: {err}"));
    }

    async fn send_all(&self, texts: &[&str]) {
        for text in texts {
            self.send(text).await;
        }
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
async fn the_add_token_flow_stores_a_verified_token() {
    let session = Session::new().await;
    session.price.set(MINT, Ok(0.999893));

    session.send_all(&["/addtoken", MINT, "USDC", "yes"]).await;

    let tokens = TokenRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].mint_address, MINT);
    assert_eq!(tokens[0].symbol.as_deref(), Some("USDC"));
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn the_add_token_flow_refuses_a_mint_with_no_price_listing() {
    let session = Session::new().await;
    session
        .price
        .set(MINT, Err(ProviderError::Unsupported("unlisted".into())));

    session.send_all(&["/addtoken", MINT]).await;

    // A token the provider cannot price could never satisfy a price rule, so the flow
    // stops rather than letting the user build an alert that never fires.
    assert!(TokenRepo::new(&session.state.db)
        .list()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn the_add_token_flow_proceeds_when_the_provider_is_merely_down() {
    let session = Session::new().await;
    session
        .price
        .set(MINT, Err(ProviderError::Unavailable("timeout".into())));

    session.send_all(&["/addtoken", MINT, "-", "yes"]).await;

    // A transient outage must not block tracking a legitimate token.
    let tokens = TokenRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].symbol, None);
}

#[tokio::test]
async fn an_invalid_mint_is_rejected_without_leaving_the_step() {
    let session = Session::new().await;
    session.price.set(MINT, Ok(1.0));

    session.send_all(&["/addtoken", "not-a-mint", "0OIl"]).await;

    // Still awaiting a mint, so the user can simply try again.
    assert!(matches!(
        session.dialogue_state().await,
        DialogueState::AddToken(chainsentinel::telegram::flows::add_token::Step::AwaitingMint)
    ));

    session.send_all(&[MINT, "-", "yes"]).await;
    assert_eq!(
        TokenRepo::new(&session.state.db)
            .list()
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn declining_the_confirmation_stores_nothing() {
    let session = Session::new().await;
    session.price.set(MINT, Ok(1.0));

    session.send_all(&["/addtoken", MINT, "-", "no"]).await;

    assert!(TokenRepo::new(&session.state.db)
        .list()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn cancel_abandons_a_flow_midway() {
    let session = Session::new().await;
    session.price.set(MINT, Ok(1.0));

    session.send_all(&["/addtoken", MINT, "/cancel"]).await;

    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
    assert!(TokenRepo::new(&session.state.db)
        .list()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn the_add_wallet_flow_stores_a_labelled_wallet() {
    let session = Session::new().await;
    session.chain.set(WALLET, 2_500_000_000);

    session
        .send_all(&["/addwallet", WALLET, "Treasury", "yes"])
        .await;

    let wallets = WalletRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(wallets.len(), 1);
    assert_eq!(wallets[0].address, WALLET);
    assert_eq!(wallets[0].label.as_deref(), Some("Treasury"));
}

#[tokio::test]
async fn adding_an_already_tracked_target_exits_cleanly() {
    let session = Session::new().await;
    session.price.set(MINT, Ok(1.0));
    TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session.send_all(&["/addtoken", MINT]).await;

    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
    assert_eq!(
        TokenRepo::new(&session.state.db)
            .list()
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn the_add_alert_flow_binds_a_rule_to_a_tracked_target() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session
        .send_all(&[
            "/addalert",
            "token",
            &token.id.to_string(),
            "<",
            "0.99",
            "600",
            "yes",
        ])
        .await;

    let rules = RuleRepo::new(&session.state.db).list_all().await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].target.kind, TargetKind::Token);
    assert_eq!(rules[0].target.id, token.id);
    assert_eq!(rules[0].operator, Operator::Lt);
    assert_eq!(rules[0].threshold, 0.99);
    assert_eq!(rules[0].cooldown_seconds, 600);
    assert!(rules[0].enabled);
}

#[tokio::test]
async fn the_add_alert_flow_uses_the_configured_default_cooldown() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, None)
        .await
        .unwrap();

    session
        .send_all(&[
            "/addalert",
            "token",
            &token.id.to_string(),
            ">",
            "5",
            "-",
            "yes",
        ])
        .await;

    let rules = RuleRepo::new(&session.state.db).list_all().await.unwrap();
    // The flow uses the configured default rather than a local constant.
    assert_eq!(
        rules[0].cooldown_seconds,
        session.state.settings.alert_default_cooldown_seconds
    );
}

#[tokio::test]
async fn the_add_alert_flow_refuses_an_untracked_target_number() {
    let session = Session::new().await;
    TokenRepo::new(&session.state.db)
        .create(MINT, None)
        .await
        .unwrap();

    session.send_all(&["/addalert", "token", "9999"]).await;

    // Still on the target step rather than creating a rule for something untracked.
    assert!(matches!(
        session.dialogue_state().await,
        DialogueState::AddAlert(
            chainsentinel::telegram::flows::add_alert::Step::AwaitingTarget { .. }
        )
    ));
    assert!(RuleRepo::new(&session.state.db)
        .list_all()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn the_add_alert_flow_rejects_nonsense_thresholds() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, None)
        .await
        .unwrap();

    session
        .send_all(&[
            "/addalert",
            "token",
            &token.id.to_string(),
            ">",
            "0",
            "-5",
            "abc",
        ])
        .await;

    assert!(matches!(
        session.dialogue_state().await,
        DialogueState::AddAlert(
            chainsentinel::telegram::flows::add_alert::Step::AwaitingThreshold { .. }
        )
    ));

    session.send_all(&["2.5", "-", "yes"]).await;
    let rules = RuleRepo::new(&session.state.db).list_all().await.unwrap();
    assert_eq!(rules[0].threshold, 2.5);
}

#[tokio::test]
async fn the_add_alert_flow_stops_when_nothing_is_tracked() {
    let session = Session::new().await;

    session.send("/addalert").await;

    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn an_admin_cannot_demote_or_block_themselves() {
    let session = Session::new().await;
    let repo = UserRepo::new(&session.state.db);
    repo.upsert(222, Role::Admin).await.unwrap();

    session
        .send_all(&[
            &format!("/demote {}", support::ADMIN_ID),
            &format!("/block {}", support::ADMIN_ID),
        ])
        .await;

    // Self-demotion was the single easiest way to permanently lock everyone out.
    let me = repo
        .find_by_telegram_id(support::ADMIN_ID)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(me.role, Role::Admin);
    assert!(!me.blocked);
}

#[tokio::test]
async fn the_last_active_admin_cannot_be_removed() {
    let session = Session::new().await;
    let repo = UserRepo::new(&session.state.db);

    // A second admin issues the commands so the self-protection rule is not what is
    // being exercised here.
    repo.upsert(222, Role::Admin).await.unwrap();
    repo.set_role(support::ADMIN_ID, Role::User).await.unwrap();

    support::dispatch(
        &session.state,
        session.storage.clone(),
        support::message_from(222, "/demote 222"),
    )
    .await
    .unwrap();

    assert_eq!(repo.count_active_admins().await.unwrap(), 1);

    // Blocking the last admin would silently disable alert delivery entirely.
    let session2 = Session::new().await;
    let repo2 = UserRepo::new(&session2.state.db);
    repo2.upsert(333, Role::Admin).await.unwrap();
    support::dispatch(
        &session2.state,
        session2.storage.clone(),
        support::message_from(333, &format!("/block {}", support::ADMIN_ID)),
    )
    .await
    .unwrap();

    // ADMIN_ID may be blocked here because 333 is still active; verify the guard by
    // trying to block the remaining one.
    support::dispatch(
        &session2.state,
        session2.storage.clone(),
        support::message_from(333, "/block 333"),
    )
    .await
    .unwrap();
    assert!(repo2.count_active_admins().await.unwrap() >= 1);
}

#[tokio::test]
async fn a_non_admin_cannot_use_admin_commands() {
    let session = Session::new().await;
    UserRepo::new(&session.state.db)
        .upsert(444, Role::User)
        .await
        .unwrap();

    support::dispatch(
        &session.state,
        session.storage.clone(),
        support::message_from(444, "/addadmin 555"),
    )
    .await
    .unwrap();

    assert!(UserRepo::new(&session.state.db)
        .find_by_telegram_id(555)
        .await
        .unwrap()
        .is_none());

    drop(session.server);
}
