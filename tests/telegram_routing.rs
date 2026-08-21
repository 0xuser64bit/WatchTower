//! Routing regression tests for the Telegram control plane.
//!
//! The defect these guard against made the entire bot unusable: because every
//! guided flow had its own dialogue storage whose state defaulted to that flow's
//! first *active* step, `dialogue::enter` put every fresh chat into
//! `AddTokenState::AwaitingMint`, and the add-token branch — registered before the
//! command branch — matched every message. `/start`, `/help`, `/alerts` and all the
//! rest were answered with "that does not look like a valid Solana mint address".
//!
//! Each test drives the real `schema()` and asserts on the actual outgoing
//! `sendMessage` payload, so a regression in routing, authorization, or reply text
//! fails here rather than in production.

mod support;

use chainsentinel::telegram::flows::DialogueState;
use mockito::Matcher;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;

/// Asserts that dispatching `text` produces a reply containing `expected`.
async fn assert_reply_contains(text: &str, expected: &str) {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .match_body(Matcher::Regex(regex::escape(expected)))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect_at_least(1)
        .create_async()
        .await;

    let db = support::database().await;
    let state = support::app_state(
        db,
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );

    support::dispatch(
        &state,
        InMemStorage::<DialogueState>::new(),
        support::message(text),
    )
    .await
    .unwrap_or_else(|err| panic!("dispatching {text:?} failed: {err}"));

    mock.assert_async().await;
}

#[tokio::test]
async fn start_reaches_the_welcome_handler() {
    assert_reply_contains("/start", "Welcome to ChainSentinel").await;
}

#[tokio::test]
async fn help_lists_commands() {
    assert_reply_contains("/help", "/addalert").await;
}

#[tokio::test]
async fn status_reaches_the_status_handler() {
    assert_reply_contains("/status", "poll interval").await;
}

#[tokio::test]
async fn empty_listings_reach_their_handlers() {
    assert_reply_contains("/tokens", "No tokens tracked yet").await;
    assert_reply_contains("/wallets", "No wallets tracked yet").await;
    assert_reply_contains("/alerts", "No alert rules yet").await;
    assert_reply_contains("/history", "No alerts have fired yet").await;
}

#[tokio::test]
async fn admin_commands_reach_their_handlers() {
    assert_reply_contains("/admin", "Admin panel").await;
    assert_reply_contains("/listusers", "Users (1)").await;
}

#[tokio::test]
async fn addtoken_starts_the_flow_rather_than_consuming_the_command() {
    assert_reply_contains("/addtoken", "Send the SPL token mint address").await;
}

#[tokio::test]
async fn a_bad_id_argument_reports_usage_instead_of_generic_help() {
    // Previously `/enablerule abc` did not match the typed `i64` command branch at
    // all and fell through to the "use /help" fallback.
    assert_reply_contains("/enablerule abc", "Usage: /enablerule <id>").await;
    assert_reply_contains("/deleterule ", "Usage: /deleterule <id>").await;
    assert_reply_contains("/deletetoken zero", "Usage: /deletetoken <id>").await;
}

#[tokio::test]
async fn plain_text_is_not_swallowed_by_a_flow() {
    // A bare base58 address with no flow active must not be treated as an answer.
    assert_reply_contains(
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        "I only understand commands",
    )
    .await;
}

#[tokio::test]
async fn unregistered_users_are_refused_without_revealing_anything() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .match_body(Matcher::Regex(regex::escape(
            "You are not authorized to use this bot.",
        )))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect(1)
        .create_async()
        .await;

    let db = support::database().await;
    let state = support::app_state(
        db,
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );

    support::dispatch(
        &state,
        InMemStorage::<DialogueState>::new(),
        support::message_from(999_999, "/start"),
    )
    .await
    .expect("dispatch");

    mock.assert_async().await;
}

#[tokio::test]
async fn a_command_cancels_an_active_flow() {
    let mut server = mockito::Server::new_async().await;

    let _catch_all = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect_at_least(1)
        .create_async()
        .await;

    let db = support::database().await;
    let state = support::app_state(
        db,
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );
    let storage = InMemStorage::<DialogueState>::new();

    // Enter the add-token flow.
    support::dispatch(&state, storage.clone(), support::message("/addtoken"))
        .await
        .expect("start flow");

    // Issuing an unrelated command must clear the dialogue, otherwise the next
    // message the user sends is silently eaten by the abandoned flow.
    support::dispatch(&state, storage.clone(), support::message("/tokens"))
        .await
        .expect("list tokens");

    let stored: DialogueState = teloxide::dispatching::dialogue::Storage::get_dialogue(
        storage.clone(),
        teloxide::types::ChatId(support::CHAT_ID),
    )
    .await
    .expect("storage read")
    .unwrap_or_default();

    assert_eq!(stored, DialogueState::Idle, "flow should have been cleared");
}

#[tokio::test]
async fn cancel_reports_whether_a_flow_was_active() {
    assert_reply_contains("/cancel", "Nothing to cancel.").await;
}

#[tokio::test]
async fn group_chats_are_refused_and_never_enter_a_flow() {
    let mut server = mockito::Server::new_async().await;

    let refusal = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .match_body(Matcher::Regex(regex::escape(
            "only works in a direct message",
        )))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect(1)
        .create_async()
        .await;

    let db = support::database().await;
    let state = support::app_state(
        db,
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );
    let storage = InMemStorage::<DialogueState>::new();

    // Dialogue state is keyed by chat, so a flow started in a group would make the
    // next message from any member — authorized or not — the answer to that step.
    support::dispatch(
        &state,
        storage.clone(),
        support::group_message_from(support::ADMIN_ID, "/addtoken"),
    )
    .await
    .expect("dispatch");

    refusal.assert_async().await;

    let stored: DialogueState = teloxide::dispatching::dialogue::Storage::get_dialogue(
        storage,
        teloxide::types::ChatId(-100_123),
    )
    .await
    .expect("storage read")
    .unwrap_or_default();

    assert_eq!(stored, DialogueState::Idle, "no flow may start in a group");
}

#[tokio::test]
async fn non_command_group_messages_are_ignored_silently() {
    let mut server = mockito::Server::new_async().await;

    // Replying to ordinary group chatter would be noise and could trip Telegram's
    // flood limits.
    let silent = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect(0)
        .create_async()
        .await;

    let db = support::database().await;
    let state = support::app_state(
        db,
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );

    support::dispatch(
        &state,
        InMemStorage::<DialogueState>::new(),
        support::group_message_from(support::ADMIN_ID, "good morning"),
    )
    .await
    .expect("dispatch");

    silent.assert_async().await;
}

#[tokio::test]
async fn blocking_a_user_takes_effect_mid_flow() {
    use chainsentinel::db::repos::users::UserRepo;

    let mut server = mockito::Server::new_async().await;

    let _prompts = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]endMessage$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(support::SEND_MESSAGE_OK)
        .expect_at_least(1)
        .create_async()
        .await;

    let db = support::database().await;
    // Priced, so nothing else in the flow can stop it: the authorization check is the
    // only thing standing between the blocked user and a created token.
    let price = Arc::new(support::FakePriceProvider::with_price(
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        1.0,
    ));
    let state = support::app_state(
        db.clone(),
        &server.url(),
        price,
        Arc::new(support::FakeChainProvider::new()),
    );
    let storage = InMemStorage::<DialogueState>::new();

    support::dispatch(&state, storage.clone(), support::message("/addtoken"))
        .await
        .expect("start flow");

    // Revoked while the user sits on a flow step. Flow steps must re-authorize, not
    // trust that access was checked when the flow began.
    UserRepo::new(&db)
        .set_blocked(support::ADMIN_ID, true)
        .await
        .unwrap();

    // Drive the flow all the way to its confirmation. Every one of these steps must
    // be refused; stopping after one would pass even with no check at all, because a
    // single step does not yet write anything.
    for answer in [
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        "USDC",
        "yes",
    ] {
        support::dispatch(&state, storage.clone(), support::message(answer))
            .await
            .expect("dispatch");
    }

    assert!(
        chainsentinel::db::repos::tokens::TokenRepo::new(&db)
            .list()
            .await
            .unwrap()
            .is_empty(),
        "a blocked user must not be able to complete a flow they had already started"
    );
}
