//! Shared fixtures for integration tests.
//!
//! Deliberately avoids any real network or filesystem dependency: the Telegram API
//! is pointed at a local mock server, providers are in-process fakes, and the
//! database is in-memory.

#![allow(dead_code)]

use chainsentinel::app_state::AppState;
use chainsentinel::config::Settings;
use chainsentinel::db::Db;
use chainsentinel::providers::{ChainProvider, PriceProvider, ProviderError, ProviderResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use teloxide::types::{Me, Update, UpdateKind};
use teloxide::Bot;
use tokio_util::sync::CancellationToken;

pub const BOT_TOKEN: &str = "1234567890:test-token-value";
pub const ADMIN_ID: i64 = 111;
pub const CHAT_ID: i64 = 111;

/// A price provider whose answers are scripted per mint.
pub struct FakePriceProvider {
    prices: Mutex<HashMap<String, ProviderResult<f64>>>,
    pub calls: AtomicUsize,
}

impl FakePriceProvider {
    pub fn new() -> Self {
        Self {
            prices: Mutex::new(HashMap::new()),
            calls: AtomicUsize::new(0),
        }
    }

    pub fn with_price(mint: &str, price: f64) -> Self {
        let provider = Self::new();
        provider.set(mint, Ok(price));
        provider
    }

    pub fn set(&self, mint: &str, result: ProviderResult<f64>) {
        self.prices.lock().unwrap().insert(mint.to_string(), result);
    }

    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl PriceProvider for FakePriceProvider {
    async fn get_token_price_usd(&self, mint: &str) -> ProviderResult<f64> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        match self.prices.lock().unwrap().get(mint) {
            Some(Ok(price)) => Ok(*price),
            Some(Err(err)) => Err(clone_error(err)),
            None => Err(ProviderError::Unsupported(format!("no price for {mint}"))),
        }
    }
}

/// A chain provider whose balances are scripted per address.
pub struct FakeChainProvider {
    balances: Mutex<HashMap<String, u64>>,
    fail: Mutex<Option<String>>,
    pub batch_calls: AtomicUsize,
}

impl FakeChainProvider {
    pub fn new() -> Self {
        Self {
            balances: Mutex::new(HashMap::new()),
            fail: Mutex::new(None),
            batch_calls: AtomicUsize::new(0),
        }
    }

    pub fn with_balance(address: &str, lamports: u64) -> Self {
        let provider = Self::new();
        provider.set(address, lamports);
        provider
    }

    pub fn set(&self, address: &str, lamports: u64) {
        self.balances
            .lock()
            .unwrap()
            .insert(address.to_string(), lamports);
    }

    pub fn fail_with(&self, message: &str) {
        *self.fail.lock().unwrap() = Some(message.to_string());
    }

    pub fn clear_failure(&self) {
        *self.fail.lock().unwrap() = None;
    }

    pub fn batch_call_count(&self) -> usize {
        self.batch_calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl ChainProvider for FakeChainProvider {
    async fn get_native_balance_lamports(&self, address: &str) -> ProviderResult<u64> {
        if let Some(message) = self.fail.lock().unwrap().clone() {
            return Err(ProviderError::Unavailable(message));
        }

        Ok(self
            .balances
            .lock()
            .unwrap()
            .get(address)
            .copied()
            .unwrap_or(0))
    }

    async fn get_native_balances_lamports(&self, addresses: &[String]) -> ProviderResult<Vec<u64>> {
        self.batch_calls.fetch_add(1, Ordering::SeqCst);

        if let Some(message) = self.fail.lock().unwrap().clone() {
            return Err(ProviderError::Unavailable(message));
        }

        let balances = self.balances.lock().unwrap();
        Ok(addresses
            .iter()
            .map(|address| balances.get(address).copied().unwrap_or(0))
            .collect())
    }
}

fn clone_error(err: &ProviderError) -> ProviderError {
    match err {
        ProviderError::Unsupported(message) => ProviderError::Unsupported(message.clone()),
        ProviderError::Unavailable(message) => ProviderError::Unavailable(message.clone()),
        ProviderError::InvalidResponse(message) => ProviderError::InvalidResponse(message.clone()),
        ProviderError::RateLimited { retry_after } => ProviderError::RateLimited {
            retry_after: *retry_after,
        },
        ProviderError::Http(_) => ProviderError::Unavailable("http error".into()),
    }
}

pub fn settings(overrides: &[(&str, &str)]) -> Settings {
    let mut env: HashMap<String, String> = HashMap::from([
        ("TELEGRAM_BOT_TOKEN".to_string(), BOT_TOKEN.to_string()),
        ("ADMIN_TELEGRAM_IDS".to_string(), ADMIN_ID.to_string()),
        ("DATABASE_URL".to_string(), "sqlite::memory:".to_string()),
    ]);

    for (key, value) in overrides {
        env.insert(key.to_string(), value.to_string());
    }

    Settings::from_env_map(&env).expect("valid test settings")
}

/// A migrated in-memory database with one active admin.
pub async fn database() -> Arc<Db> {
    use chainsentinel::db::repos::users::{Role, UserRepo};

    let db = Db::connect_in_memory().await.expect("connect");
    db.migrate().await.expect("migrate");
    UserRepo::new(&db)
        .upsert(ADMIN_ID, Role::Admin)
        .await
        .expect("seed admin");

    Arc::new(db)
}

/// Builds an `AppState` whose Telegram calls go to `api_url`.
pub fn app_state(
    db: Arc<Db>,
    api_url: &str,
    price: Arc<FakePriceProvider>,
    chain: Arc<FakeChainProvider>,
) -> AppState {
    let bot = Bot::new(BOT_TOKEN).set_api_url(api_url.parse().expect("api url"));

    AppState::new(
        db,
        bot,
        Arc::new(settings(&[])),
        price,
        chain,
        CancellationToken::new(),
    )
}

pub fn me() -> Me {
    serde_json::from_str(
        r#"{"id":1,"is_bot":true,"first_name":"ChainSentinel","username":"chainsentinel_bot",
            "can_join_groups":false,"can_read_all_group_messages":false,
            "supports_inline_queries":false}"#,
    )
    .expect("me json")
}

/// Builds a private-chat text update from `sender`.
pub fn message_from(sender: i64, text: &str) -> Update {
    let entities = if text.starts_with('/') {
        let length = text.split_whitespace().next().map_or(0, str::len);
        format!(r#","entities":[{{"type":"bot_command","offset":0,"length":{length}}}]"#)
    } else {
        String::new()
    };

    let raw = format!(
        r#"{{"update_id":1,"message":{{"message_id":1,"date":1700000000,
            "chat":{{"id":{CHAT_ID},"type":"private","first_name":"T"}},
            "from":{{"id":{sender},"is_bot":false,"first_name":"T"}},
            "text":{}{entities}}}}}"#,
        serde_json::to_string(text).expect("text json")
    );

    serde_json::from_str(&raw).expect("update json")
}

pub fn message(text: &str) -> Update {
    message_from(ADMIN_ID, text)
}

/// Dispatches `update` through the real handler tree and returns the endpoint result.
pub async fn dispatch(
    state: &AppState,
    storage: Arc<
        teloxide::dispatching::dialogue::InMemStorage<
            chainsentinel::telegram::flows::DialogueState,
        >,
    >,
    update: Update,
) -> Result<(), String> {
    let mut deps = teloxide::dptree::deps![state.clone(), storage, me()];
    deps.insert(update.clone());

    if let UpdateKind::Message(msg) = update.kind {
        deps.insert(msg);
    }

    match chainsentinel::telegram::schema().dispatch(deps).await {
        std::ops::ControlFlow::Break(Ok(())) => Ok(()),
        std::ops::ControlFlow::Break(Err(err)) => Err(err.to_string()),
        std::ops::ControlFlow::Continue(_) => Err("update was not handled".to_string()),
    }
}

/// A canned successful `sendMessage` response.
pub const SEND_MESSAGE_OK: &str = r#"{"ok":true,"result":{"message_id":2,"date":1700000000,
    "chat":{"id":111,"type":"private","first_name":"T"},"text":"ok"}}"#;
