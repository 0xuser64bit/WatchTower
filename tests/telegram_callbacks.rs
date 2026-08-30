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
    price: Arc<support::FakePriceProvider>,
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
        let state = support::app_state(db, &server.url(), price.clone(), chain);

        Self {
            _server: server,
            state,
            storage: InMemStorage::new(),
            price,
        }
    }

    /// The scripted price provider behind this session.
    fn state_price(&self) -> &support::FakePriceProvider {
        &self.price
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
    session.tap("at:us").await; // accept the catalog's name for a known mint
    session.tap("at:ok").await; // confirm

    let tokens = TokenRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].mint_address, MINT);
    // USDC is in the built-in catalog, so the reviewed symbol is offered by name
    // rather than leaving a well-known token unnamed.
    assert_eq!(tokens[0].symbol.as_deref(), Some("USDC"));
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

/// The whole point of the catalog: a well-known token with nothing typed at all.
#[tokio::test]
async fn a_popular_token_can_be_added_without_typing_anything() {
    let session = Session::new().await;

    // Add Token → 🔥 Popular → SOL & stablecoins → USDC → Use USDC → confirm.
    session.tap("at:new").await;
    session.tap("at:pop").await;
    session.tap("at:g:c").await;
    let index = catalog_index("USDC");
    session.tap(&format!("at:p:{index}")).await;
    session.tap("at:us").await;
    session.tap("at:ok").await;

    let tokens = TokenRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].mint_address, MINT);
    assert_eq!(tokens[0].symbol.as_deref(), Some("USDC"));
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

/// The catalog is reachable from the Tokens screen, not only from inside the flow.
#[tokio::test]
async fn the_catalog_is_reachable_from_the_tokens_screen() {
    let session = Session::new().await;

    session.tap("tk").await;
    session.tap("at:pop").await;
    session.tap("at:g:c").await;
    session
        .tap(&format!("at:p:{}", catalog_index("USDC")))
        .await;
    session.tap("at:us").await;
    session.tap("at:ok").await;

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
async fn a_catalog_pick_is_still_price_verified_before_being_stored() {
    let session = Session::new().await;
    // BONK is in the catalog but this fake provider only prices USDC, so the pick
    // must be refused rather than stored as a token whose alerts could never fire.
    let bonk = catalog_index("BONK");

    session.tap("at:pop").await;
    session.tap("at:g:m").await;
    session.tap(&format!("at:p:{bonk}")).await;

    assert!(
        TokenRepo::new(&session.state.db)
            .list()
            .await
            .unwrap()
            .is_empty(),
        "an unpriced catalog token must not be stored"
    );
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn an_out_of_range_catalog_index_stores_nothing() {
    let session = Session::new().await;

    // A stale or hand-crafted button must not be able to reach past the catalog.
    session.tap("at:pop").await;
    session
        .tap(&format!("at:p:{}", watchtower::catalog::ENTRIES.len()))
        .await;
    session.tap("at:p:999999").await;
    session.tap("at:p:not-a-number").await;
    session.tap("at:g:zz").await;

    assert!(TokenRepo::new(&session.state.db)
        .list()
        .await
        .unwrap()
        .is_empty());
    assert!(matches!(
        session.dialogue_state().await,
        DialogueState::AddToken(watchtower::telegram::flows::add_token::Step::AwaitingMint)
    ));
}

#[tokio::test]
async fn a_catalog_pick_that_is_already_tracked_exits_cleanly() {
    let session = Session::new().await;
    TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session.tap("at:pop").await;
    session.tap("at:g:c").await;
    session
        .tap(&format!("at:p:{}", catalog_index("USDC")))
        .await;

    assert_eq!(
        TokenRepo::new(&session.state.db)
            .list()
            .await
            .unwrap()
            .len(),
        1,
        "the token must not be duplicated"
    );
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn a_blocked_user_cannot_add_a_catalog_token() {
    let session = Session::new().await;

    UserRepo::new(&session.state.db)
        .set_blocked(support::ADMIN_ID, true)
        .await
        .unwrap();

    session.tap("at:pop").await;
    session.tap("at:g:c").await;
    session
        .tap(&format!("at:p:{}", catalog_index("USDC")))
        .await;
    session.tap("at:us").await;
    session.tap("at:ok").await;

    assert!(
        TokenRepo::new(&session.state.db)
            .list()
            .await
            .unwrap()
            .is_empty(),
        "the catalog must not be a way around authorization"
    );
}

/// A mint the catalog does not know still gets the plain Skip path.
#[tokio::test]
async fn an_unknown_mint_is_still_offered_the_skip_button() {
    let session = Session::new().await;
    const UNKNOWN: &str = "CZecYkamnAJKs6g2s4uoykkrstweT6XWu5zi9bdJiaS8";
    session.state_price().set(UNKNOWN, Ok(3.5));

    session.tap("at:new").await;
    session.send(UNKNOWN).await;
    session.tap("at:sk").await;
    session.tap("at:ok").await;

    let tokens = TokenRepo::new(&session.state.db).list().await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].mint_address, UNKNOWN);
    assert_eq!(tokens[0].symbol, None);
}

/// The index a button carries for `symbol`. Looked up rather than hard-coded so the
/// tests keep testing the flow when the catalog grows.
fn catalog_index(symbol: &str) -> usize {
    watchtower::catalog::ENTRIES
        .iter()
        .position(|entry| entry.symbol == symbol)
        .unwrap_or_else(|| panic!("{symbol} is not in the catalog"))
}

// ── Favourites ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_token_can_be_starred_and_unstarred_by_tapping() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    session.tap(&format!("tk:v:{}", token.id)).await;
    session.tap(&format!("tk:f:{}:1", token.id)).await;
    assert!(
        TokenRepo::new(&session.state.db)
            .find(token.id)
            .await
            .unwrap()
            .unwrap()
            .is_favourite(),
        "the star should have been recorded"
    );

    session.tap(&format!("tk:f:{}:0", token.id)).await;
    assert!(!TokenRepo::new(&session.state.db)
        .find(token.id)
        .await
        .unwrap()
        .unwrap()
        .is_favourite());
}

#[tokio::test]
async fn the_favourite_button_carries_the_end_state_so_a_double_tap_converges() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    // Two taps on the same (possibly stale) button must not undo each other, which is
    // exactly what a "flip it" callback would do.
    session.tap(&format!("tk:f:{}:1", token.id)).await;
    session.tap(&format!("tk:f:{}:1", token.id)).await;

    assert!(TokenRepo::new(&session.state.db)
        .find(token.id)
        .await
        .unwrap()
        .unwrap()
        .is_favourite());
    assert_eq!(
        TokenRepo::new(&session.state.db)
            .count_favourites()
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn the_favourites_screen_is_reachable_and_lists_only_starred_tokens() {
    use mockito::Matcher;

    let mut server = mockito::Server::new_async().await;

    // Asserted on the rendered screen rather than only on the database, because the
    // route is what is under test: an unrecognised callback silently falls through to
    // "That button has expired" and the main menu, which a database check would miss.
    let screen = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[eE]ditMessageText$".into()),
        )
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(regex::escape("Favourites")),
            Matcher::Regex(regex::escape("USDC")),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect_at_least(1)
        .create_async()
        .await;

    // The unstarred token must not appear on this screen at all.
    let absent = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[eE]ditMessageText$".into()),
        )
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(regex::escape("Favourites")),
            Matcher::Regex(regex::escape("BONK")),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect(0)
        .create_async()
        .await;

    for method in [
        "[sS]endMessage",
        "[eE]ditMessageText",
        "[aA]nswerCallbackQuery",
    ] {
        server
            .mock("POST", Matcher::Regex(format!(r"^/bot.+/{method}$")))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(support::SEND_MESSAGE_OK)
            .expect_at_least(0)
            .create_async()
            .await;
    }

    let db = support::database().await;
    let state = support::app_state(
        db.clone(),
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );
    let storage = InMemStorage::<DialogueState>::new();

    let repo = TokenRepo::new(&db);
    let starred = repo.create(MINT, Some("USDC")).await.unwrap();
    repo.create("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", Some("BONK"))
        .await
        .unwrap();
    repo.set_favourite(starred.id, true).await.unwrap();

    for data in ["fv", "fv:p:0"] {
        support::dispatch(&state, storage.clone(), support::callback(data))
            .await
            .expect("dispatch");
    }

    screen.assert_async().await;
    absent.assert_async().await;
}

#[tokio::test]
async fn a_favourite_starts_an_alert_with_the_token_already_chosen() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();
    TokenRepo::new(&session.state.db)
        .set_favourite(token.id, true)
        .await
        .unwrap();

    // Create Alert on the token detail skips the kind and target steps entirely: the
    // condition is the first thing asked.
    session.tap(&format!("ac:tk:{}", token.id)).await;
    session.tap("ac:op:lt").await;
    session.send("0.99").await;
    session.tap("ac:cd").await;
    session.tap("ac:ok").await;

    let rules = RuleRepo::new(&session.state.db).list_all().await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].target.id, token.id);
    assert_eq!(rules[0].operator, Operator::Lt);
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn a_shortcut_onto_a_token_that_is_gone_creates_nothing() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();
    TokenRepo::new(&session.state.db)
        .delete(token.id)
        .await
        .unwrap();

    // A stale keyboard must not seed a rule pointing at a token that has been removed;
    // the foreign key would refuse it, but the flow must not get that far.
    session.tap(&format!("ac:tk:{}", token.id)).await;
    session.tap("ac:op:lt").await;

    assert!(RuleRepo::new(&session.state.db)
        .list_all()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(session.dialogue_state().await, DialogueState::Idle);
}

#[tokio::test]
async fn a_malformed_favourite_button_changes_nothing() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    // Only the two flags this crate emits may be honoured, and an id must be a real
    // positive row: a hand-crafted button cannot coerce anything else into a write.
    for data in [
        format!("tk:f:{}:yes", token.id),
        format!("tk:f:{}:2", token.id),
        format!("tk:f:{}:", token.id),
        "tk:f:0:1".to_string(),
        "tk:f:-1:1".to_string(),
        "tk:f:99999:1".to_string(),
        "tk:f:abc:1".to_string(),
    ] {
        session.tap(&data).await;
    }

    assert!(!TokenRepo::new(&session.state.db)
        .find(token.id)
        .await
        .unwrap()
        .unwrap()
        .is_favourite());
    assert_eq!(
        TokenRepo::new(&session.state.db)
            .count_favourites()
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn a_blocked_user_cannot_star_a_token() {
    let session = Session::new().await;
    let token = TokenRepo::new(&session.state.db)
        .create(MINT, Some("USDC"))
        .await
        .unwrap();

    UserRepo::new(&session.state.db)
        .set_blocked(support::ADMIN_ID, true)
        .await
        .unwrap();

    session.tap(&format!("tk:f:{}:1", token.id)).await;
    session.tap("fv").await;

    assert!(
        !TokenRepo::new(&session.state.db)
            .find(token.id)
            .await
            .unwrap()
            .unwrap()
            .is_favourite(),
        "favourites must not be a way around authorization"
    );
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
