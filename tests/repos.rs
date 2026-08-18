use chainsentinel::db::Db;
use chainsentinel::db::repos::users::{Role, UserRepo};

#[tokio::test]
async fn user_repo_crud() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = UserRepo::new(&db);

    let user = repo.create(123, Role::Admin).await.unwrap();
    assert_eq!(user.telegram_id, 123);
    assert_eq!(user.role(), Role::Admin);

    let found = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert_eq!(found.id, user.id);

    repo.set_role(123, Role::User).await.unwrap();
    let updated = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert_eq!(updated.role(), Role::User);

    repo.set_blocked(123, true).await.unwrap();
    let blocked = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert!(blocked.is_blocked());

    repo.set_blocked(123, false).await.unwrap();
    let unblocked = repo.find_by_telegram_id(123).await.unwrap().unwrap();
    assert!(!unblocked.is_blocked());
}

#[tokio::test]
async fn token_repo_soft_delete() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let repo = chainsentinel::db::repos::tokens::TokenRepo::new(&db);
    let token = repo.create("mint_address_12345678901234567890", Some("TKN"), None).await.unwrap();

    assert_eq!(token.symbol.as_deref(), Some("TKN"));

    repo.soft_delete(token.id).await.unwrap();
    assert!(repo.find_by_id(token.id).await.unwrap().is_none());
}
