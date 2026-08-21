use chainsentinel::db::repos::alert_events::AlertEventRepo;
use chainsentinel::db::repos::rules::RuleRepo;
use chainsentinel::db::repos::users::{Role, UserRepo};
use chainsentinel::db::repos::wallets::WalletRepo;
use chainsentinel::db::Db;

#[tokio::test]
async fn user_repo_crud() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = UserRepo::new(&db);

    let user = repo.upsert(123, Role::Admin).await.unwrap();
    assert_eq!(user.telegram_id, 123);
    assert_eq!(user.role, Role::Admin);

    let found = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert_eq!(found.id, user.id);

    repo.set_role(123, Role::User).await.unwrap();
    let updated = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert_eq!(updated.role, Role::User);

    repo.set_blocked(123, true).await.unwrap();
    let blocked = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert!(blocked.blocked);

    repo.set_blocked(123, false).await.unwrap();
    let unblocked = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert!(!unblocked.blocked);
}

#[tokio::test]
async fn user_repo_lists_only_unblocked_admins() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = UserRepo::new(&db);
    repo.upsert(111, Role::Admin).await.unwrap();
    repo.upsert(222, Role::Admin).await.unwrap();
    repo.upsert(333, Role::User).await.unwrap();

    let admins = repo.list_active_admins().await.unwrap();
    assert_eq!(admins.len(), 2);

    repo.set_blocked(222, true).await.unwrap();
    let admins = repo.list_active_admins().await.unwrap();
    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0].telegram_id, 111);
}

#[tokio::test]
async fn token_repo_soft_delete() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = chainsentinel::db::repos::tokens::TokenRepo::new(&db);
    let token = repo
        .create("mint_address_12345678901234567890", Some("TKN"), None)
        .await
        .unwrap();

    assert_eq!(token.symbol.as_deref(), Some("TKN"));

    repo.soft_delete(token.id).await.unwrap();
    assert!(repo.find_by_id(token.id).await.unwrap().is_none());
}

#[tokio::test]
async fn wallet_repo_crud_and_soft_delete() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = WalletRepo::new(&db);
    let wallet = repo
        .create("WalletAddress123456789", Some("Treasury"))
        .await
        .unwrap();
    assert_eq!(wallet.label.as_deref(), Some("Treasury"));

    repo.update_last_seen(wallet.id, "signature").await.unwrap();
    let updated = repo
        .find_by_address("WalletAddress123456789")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.last_seen_signature.as_deref(), Some("signature"));

    repo.soft_delete(wallet.id).await.unwrap();
    assert!(repo
        .find_by_address("WalletAddress123456789")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn rule_repo_reference_and_enabled_state() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = RuleRepo::new(&db);
    let rule = repo
        .create(
            "price",
            "token",
            "mint",
            "price",
            "pct_change_up",
            10.0,
            None,
            300,
            None,
            None,
        )
        .await
        .unwrap();

    repo.initialize_reference_if_missing(rule.id, 100.0)
        .await
        .unwrap();
    let updated = repo.find_by_id(rule.id).await.unwrap().unwrap();
    assert_eq!(updated.reference_value, Some(100.0));

    repo.initialize_reference_if_missing(rule.id, 200.0)
        .await
        .unwrap();
    let unchanged = repo.find_by_id(rule.id).await.unwrap().unwrap();
    assert_eq!(unchanged.reference_value, Some(100.0));

    repo.set_enabled(rule.id, false).await.unwrap();
    assert!(repo.list_enabled().await.unwrap().is_empty());

    repo.soft_delete(rule.id).await.unwrap();
    assert!(repo.find_by_id(rule.id).await.unwrap().is_none());
}

#[tokio::test]
async fn alert_event_dedup_key_is_unique() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let rule = RuleRepo::new(&db)
        .create(
            "price", "token", "mint", "price", ">", 90.0, None, 300, None, None,
        )
        .await
        .unwrap();

    let repo = AlertEventRepo::new(&db);
    let event = repo
        .insert(rule.id, 100.0, 90.0, "alert", "dedup-1")
        .await
        .unwrap();
    assert_eq!(event.dedup_key, "dedup-1");

    let dup = repo.insert(rule.id, 100.0, 90.0, "alert", "dedup-1").await;
    assert!(dup.is_err());

    let recent = repo.list_recent(10).await.unwrap();
    assert_eq!(recent.len(), 1);
}
