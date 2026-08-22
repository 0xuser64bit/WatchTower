//! Telegram control-plane tests.
//!
//! Each test drives the real `schema()` and asserts on outgoing `sendMessage`
//! payloads so routing, authorization, dialogue state, and reply text are verified
//! together.

mod support;

use mockito::Matcher;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use watchtower::telegram::flows::DialogueState;

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
    assert_reply_contains("/start", "WatchTower watches Solana").await;
}

#[tokio::test]
async fn a_new_user_is_given_three_steps_not_a_command_dump() {
    // The first message tells someone what to press instead of listing every command.
    assert_reply_contains("/start", "/addtoken").await;
    assert_reply_contains("/start", "/addalert").await;
}

#[tokio::test]
async fn help_explains_operators_with_worked_examples() {
    assert_reply_contains("/help", "%down").await;
    assert_reply_contains("/help", "e.g.").await;
    // The three states /alerts can show must be looked up somewhere.
    assert_reply_contains("/help", "armed").await;
    assert_reply_contains("/help", "firing").await;
}

#[tokio::test]
async fn help_states_the_scope_so_nobody_assumes_multi_chain() {
    assert_reply_contains("/help", "Solana only").await;
    assert_reply_contains("/help", "read-only").await;
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
    assert_reply_contains("/addtoken", "Which token?").await;
}

#[tokio::test]
async fn prompts_show_a_real_example_of_what_to_paste() {
    // "Send the SPL token mint address" assumes the user knows what one looks like.
    assert_reply_contains("/addtoken", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").await;
    assert_reply_contains("/addwallet", "never need a key").await;
}

#[tokio::test]
async fn a_bad_id_argument_reports_usage_instead_of_generic_help() {
    assert_reply_contains("/enablerule abc", "/enablerule <id>").await;
    assert_reply_contains("/deleterule ", "/deleterule <id>").await;
    // And says where the number comes from, rather than leaving the user to guess.
    assert_reply_contains("/deletetoken zero", "comes from the listing").await;
}

#[tokio::test]
async fn plain_text_is_not_swallowed_by_a_flow() {
    // A bare base58 address with no flow active must not be treated as an answer.
    assert_reply_contains("hello there", "I only take commands").await;
}

#[tokio::test]
async fn a_pasted_address_is_answered_with_what_to_do_with_it() {
    // Pasting an address is the most likely thing someone tries without a command;
    // pointing them at the manual instead of the two relevant commands is a dead end.
    assert_reply_contains("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "/addtoken").await;
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
    use watchtower::db::repos::users::UserRepo;

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
        watchtower::db::repos::tokens::TokenRepo::new(&db)
            .list()
            .await
            .unwrap()
            .is_empty(),
        "a blocked user must not be able to complete a flow they had already started"
    );
}

#[tokio::test]
async fn the_command_menu_is_published_to_telegram() {
    // This fills the `/` autocomplete list and menu button.
    let mut server = mockito::Server::new_async().await;

    let all_private = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]etMyCommands$".to_string()),
        )
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(regex::escape("all_private_chats")),
            Matcher::Regex(regex::escape(r#""command":"addalert""#)),
            Matcher::Regex(regex::escape(r#""command":"addtoken""#)),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":true}"#)
        .expect(1)
        .create_async()
        .await;

    // The seeded admin additionally gets the admin entries, scoped to their own chat.
    let admin_scope = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]etMyCommands$".to_string()),
        )
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(regex::escape(r#""type":"chat""#)),
            Matcher::Regex(regex::escape(r#""command":"listusers""#)),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":true}"#)
        .expect(1)
        .create_async()
        .await;

    let db = support::database().await;
    let state = support::app_state(
        db.clone(),
        &server.url(),
        Arc::new(support::FakePriceProvider::new()),
        Arc::new(support::FakeChainProvider::new()),
    );

    watchtower::telegram::menu::publish(&state.bot, &state.db).await;

    all_private.assert_async().await;
    admin_scope.assert_async().await;
}

#[tokio::test]
async fn a_failed_menu_publish_is_not_fatal() {
    // The menu is a convenience. A per-chat registration legitimately fails when the
    // user has never opened a chat with the bot, and that must not break startup.
    let mut server = mockito::Server::new_async().await;

    let _failing = server
        .mock(
            "POST",
            Matcher::Regex(r"^/bot.+/[sS]etMyCommands$".to_string()),
        )
        .with_status(400)
        .with_body(r#"{"ok":false,"error_code":400,"description":"Bad Request"}"#)
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

    // Must simply return.
    watchtower::telegram::menu::publish(&state.bot, &state.db).await;
}
