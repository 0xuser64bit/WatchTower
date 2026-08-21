use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("telegram error: {0}")]
    Telegram(#[from] teloxide::RequestError),

    #[error("provider error: {0}")]
    Provider(#[from] crate::providers::ProviderError),

    /// A persisted value could not be interpreted by this build. Indicates data
    /// written by a different schema version or manual tampering.
    #[error("inconsistent stored data: {0}")]
    Data(String),

    #[error("not found: {0}")]
    NotFound(String),

    /// The request was well-formed but conflicts with current state.
    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    /// Message safe to show a Telegram user. Infrastructure errors are deliberately
    /// collapsed so provider URLs, SQL, and internal paths never reach a chat.
    pub fn user_message(&self) -> String {
        match self {
            AppError::NotFound(what) => format!("Not found: {what}."),
            AppError::Conflict(what) => format!("Cannot do that: {what}."),
            AppError::InvalidInput(what) => format!("Invalid input: {what}."),
            AppError::Provider(_) => {
                "A data provider is unavailable right now. Please try again shortly.".to_string()
            }
            _ => "Something went wrong on our side. The error has been logged.".to_string(),
        }
    }
}
