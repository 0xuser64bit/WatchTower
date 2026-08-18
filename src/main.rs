use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    init_logging();

    info!("ChainSentinel starting");
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
