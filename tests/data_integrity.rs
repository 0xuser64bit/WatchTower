//! Data-integrity tests exercised through the real migrations and repositories.

mod support;

use watchtower::db::repos::rules::{NewRuleTarget, RuleRepo};
use watchtower::db::repos::tokens::TokenRepo;
use watchtower::db::repos::users::{Role, UserRepo};
use watchtower::db::repos::wallets::WalletRepo;
use watchtower::db::Db;
use watchtower::error::AppError;
use watchtower::rules::types::Operator;

const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const WALLET: &str = "So11111111111111111111111111111111111111112";

async fn db() -> Db {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();
    db
}

#[tokio::test]
async fn migrations_apply_cleanly_and_are_idempotent() {
    let db = db().await;
    // A second run must be a no-op rather than re-applying DDL.
    db.migrate().await.unwrap();
    db.ping().await.unwrap();
}

#[tokio::test]
async fn a_deleted_token_can_be_added_again() {
    let db = db().await;
    let repo = TokenRepo::new(&db);

    let first = repo.create(MINT, Some("USDC")).await.unwrap();
    repo.delete(first.id).await.unwrap();

    // Deletion must release the unique mint so it can be tracked again.
    let second = repo.create(MINT, Some("USDC")).await.unwrap();
    assert_ne!(second.id, first.id);
    assert_eq!(second.mint_address, MINT);
}

#[tokio::test]
async fn a_deleted_wallet_can_be_added_again() {
    let db = db().await;
    let repo = WalletRepo::new(&db);

    let first = repo.create(WALLET, Some("Treasury")).await.unwrap();
    repo.delete(first.id).await.unwrap();

    assert!(repo.create(WALLET, None).await.is_ok());
}

#[tokio::test]
async fn tracking_the_same_target_twice_is_a_clear_conflict() {
    let db = db().await;
    let repo = TokenRepo::new(&db);
    repo.create(MINT, None).await.unwrap();

    let err = repo.create(MINT, None).await.unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err}");
    assert!(err.user_message().contains("already tracked"));
}

#[tokio::test]
async fn an_identical_rule_cannot_be_created_twice() {
    let db = db().await;
    let token = TokenRepo::new(&db).create(MINT, None).await.unwrap();
    let repo = RuleRepo::new(&db);

    repo.create(
        NewRuleTarget::Token { id: token.id },
        Operator::Gt,
        100.0,
        300,
    )
    .await
    .unwrap();

    let err = repo
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            100.0,
            300,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err}");

    // A different threshold or operator is a genuinely different rule.
    assert!(repo
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            200.0,
            300
        )
        .await
        .is_ok());
    assert!(repo
        .create(
            NewRuleTarget::Token { id: token.id },
            Operator::Lt,
            100.0,
            300
        )
        .await
        .is_ok());
}

#[tokio::test]
async fn a_rule_cannot_reference_a_target_that_does_not_exist() {
    let db = db().await;

    // Enforced by the foreign key, so it holds regardless of application checks.
    assert!(RuleRepo::new(&db)
        .create(NewRuleTarget::Token { id: 9_999 }, Operator::Gt, 1.0, 0)
        .await
        .is_err());
    assert!(RuleRepo::new(&db)
        .create(NewRuleTarget::Wallet { id: 9_999 }, Operator::Gt, 1.0, 0)
        .await
        .is_err());
}

#[tokio::test]
async fn invalid_rule_parameters_are_rejected() {
    let db = db().await;
    let token = TokenRepo::new(&db).create(MINT, None).await.unwrap();
    let repo = RuleRepo::new(&db);
    let target = NewRuleTarget::Token { id: token.id };

    for threshold in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let err = repo
            .create(target, Operator::Gt, threshold, 0)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::InvalidInput(_)),
            "{threshold}: {err}"
        );
    }

    assert!(matches!(
        repo.create(target, Operator::Gt, 1.0, -5)
            .await
            .unwrap_err(),
        AppError::InvalidInput(_)
    ));
}

#[tokio::test]
async fn deleting_a_target_reports_and_removes_its_rules() {
    let db = db().await;
    let token = TokenRepo::new(&db).create(MINT, None).await.unwrap();
    let repo = RuleRepo::new(&db);

    for threshold in [1.0, 2.0, 3.0] {
        repo.create(
            NewRuleTarget::Token { id: token.id },
            Operator::Gt,
            threshold,
            0,
        )
        .await
        .unwrap();
    }

    let removed = TokenRepo::new(&db).delete(token.id).await.unwrap();
    assert_eq!(removed, 3);
    assert!(repo.list_all().await.unwrap().is_empty());
}

#[tokio::test]
async fn listings_report_how_many_rules_depend_on_each_target() {
    let db = db().await;
    let token = TokenRepo::new(&db).create(MINT, None).await.unwrap();
    let wallet = WalletRepo::new(&db).create(WALLET, None).await.unwrap();

    RuleRepo::new(&db)
        .create(NewRuleTarget::Token { id: token.id }, Operator::Gt, 1.0, 0)
        .await
        .unwrap();
    RuleRepo::new(&db)
        .create(NewRuleTarget::Token { id: token.id }, Operator::Gt, 2.0, 0)
        .await
        .unwrap();

    let tokens = TokenRepo::new(&db).list().await.unwrap();
    assert_eq!(tokens[0].rule_count, 2);

    let wallets = WalletRepo::new(&db).list().await.unwrap();
    assert_eq!(wallets[0].rule_count, 0);
    assert_eq!(wallets[0].id, wallet.id);
}

#[tokio::test]
async fn deleting_a_missing_row_is_not_found_rather_than_silent_success() {
    let db = db().await;

    assert!(matches!(
        TokenRepo::new(&db).delete(1).await.unwrap_err(),
        AppError::NotFound(_)
    ));
    assert!(matches!(
        WalletRepo::new(&db).delete(1).await.unwrap_err(),
        AppError::NotFound(_)
    ));
    assert!(matches!(
        RuleRepo::new(&db).delete(1).await.unwrap_err(),
        AppError::NotFound(_)
    ));
    assert!(matches!(
        RuleRepo::new(&db).set_enabled(1, true).await.unwrap_err(),
        AppError::NotFound(_)
    ));
}

#[tokio::test]
async fn user_upsert_is_idempotent_and_promotes_in_place() {
    let db = db().await;
    let repo = UserRepo::new(&db);

    let created = repo.upsert(500, Role::User).await.unwrap();
    let promoted = repo.upsert(500, Role::Admin).await.unwrap();

    assert_eq!(created.id, promoted.id, "must update, not duplicate");
    assert_eq!(promoted.role, Role::Admin);
    assert_eq!(repo.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn blocking_removes_a_user_from_the_alert_recipient_set() {
    let db = db().await;
    let repo = UserRepo::new(&db);

    repo.upsert(1, Role::Admin).await.unwrap();
    repo.upsert(2, Role::Admin).await.unwrap();
    repo.upsert(3, Role::User).await.unwrap();
    assert_eq!(repo.count_active_admins().await.unwrap(), 2);

    repo.set_blocked(2, true).await.unwrap();
    let admins = repo.list_active_admins().await.unwrap();
    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0].telegram_id, 1);

    repo.set_blocked(2, false).await.unwrap();
    assert_eq!(repo.count_active_admins().await.unwrap(), 2);
}

#[tokio::test]
async fn updating_a_missing_user_is_not_found() {
    let db = db().await;
    let repo = UserRepo::new(&db);

    assert!(matches!(
        repo.set_role(404, Role::Admin).await.unwrap_err(),
        AppError::NotFound(_)
    ));
    assert!(matches!(
        repo.set_blocked(404, true).await.unwrap_err(),
        AppError::NotFound(_)
    ));
}

#[tokio::test]
async fn a_corrupted_operator_is_surfaced_instead_of_silently_defaulting() {
    let db = db().await;
    let token = TokenRepo::new(&db).create(MINT, None).await.unwrap();
    let rule = RuleRepo::new(&db)
        .create(NewRuleTarget::Token { id: token.id }, Operator::Gt, 1.0, 0)
        .await
        .unwrap();

    // Bypass the CHECK constraint the way a bad migration or a manual edit would, to
    // prove the read path validates rather than coercing the value to `>`.
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query("UPDATE rules SET operator = 'sideways' WHERE id = ?")
        .bind(rule.id)
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query("PRAGMA ignore_check_constraints = OFF")
        .execute(db.pool())
        .await
        .unwrap();

    let err = RuleRepo::new(&db).find(rule.id).await.unwrap_err();
    assert!(matches!(err, AppError::Data(_)), "{err}");

    // The same must hold for a listing, so one corrupt row cannot be silently
    // evaluated with a wrong operator during a monitoring tick.
    assert!(matches!(
        RuleRepo::new(&db).list_enabled().await.unwrap_err(),
        AppError::Data(_)
    ));
}

#[tokio::test]
async fn user_facing_errors_never_leak_internals() {
    let messages = [
        AppError::Database(sqlx::Error::PoolClosed).user_message(),
        AppError::Data("rules.operator = 'sideways'".into()).user_message(),
        AppError::Io(std::io::Error::other("/srv/secret/path")).user_message(),
    ];

    for message in messages {
        assert!(!message.contains("sideways"), "{message}");
        assert!(!message.contains("/srv"), "{message}");
        assert!(!message.to_lowercase().contains("pool"), "{message}");
    }

    // Errors caused by the user, however, must be specific enough to act on.
    assert!(AppError::NotFound("token 7".into())
        .user_message()
        .contains("token 7"));
}

#[tokio::test]
async fn a_target_deleted_mid_flow_yields_a_clear_conflict() {
    let db = db().await;
    let token = TokenRepo::new(&db).create(MINT, None).await.unwrap();

    // The guided flow resolves the target, then asks two more questions before saving.
    // If the target disappears in between, the raw foreign-key error would surface as
    // "something went wrong on our side", which is both wrong and unactionable.
    TokenRepo::new(&db).delete(token.id).await.unwrap();

    let err = RuleRepo::new(&db)
        .create(NewRuleTarget::Token { id: token.id }, Operator::Gt, 1.0, 0)
        .await
        .unwrap_err();

    assert!(matches!(err, AppError::Conflict(_)), "{err}");
    assert!(err.user_message().contains("no longer tracked"), "{err}");
}
