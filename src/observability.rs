//! Logging setup.
//!
//! Writes to stdout and a rolling file. File logging is best effort: if the log
//! directory cannot be created, startup continues with stdout so systemd and local
//! process supervisors still receive diagnostics.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Must be held for the process lifetime: dropping it flushes and stops the
/// background log writer. Returned rather than leaked so the flush happens on a
/// normal shutdown.
#[must_use = "dropping the guard stops file logging"]
pub struct LoggingGuard {
    // Held purely for its `Drop`: it flushes the non-blocking writer's buffer.
    #[allow(dead_code)]
    file_writer: Option<WorkerGuard>,
}

/// Initialises logging. Never panics and never returns an error: losing logs must not
/// prevent the daemon from starting.
pub fn init(log_dir: &str, max_files: usize) -> LoggingGuard {
    let filter = EnvFilter::try_from_default_env()
        // teloxide and sqlx are noisy at debug; default to our own crate at info.
        .unwrap_or_else(|_| EnvFilter::new("info,watchtower=info"));

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(std::io::stdout)
        .boxed();

    let (file_layer, guard, file_error) = match build_file_layer(log_dir, max_files) {
        Ok((layer, guard)) => (Some(layer), Some(guard), None),
        Err(err) => (None, None, Some(err)),
    };

    let mut layers: Vec<BoxedLayer> = vec![stdout_layer];
    layers.extend(file_layer);

    // `EnvFilter` is a global filter, so it applies to every layer regardless of
    // where it sits in the stack; layers are added first only to keep the generic
    // types of the boxed layers uniform.
    tracing_subscriber::registry()
        .with(layers)
        .with(filter)
        .init();

    match &file_error {
        Some(err) => tracing::warn!(
            log_dir,
            error = %err,
            "file logging disabled; continuing with stdout only"
        ),
        None => tracing::debug!(log_dir, max_files, "file logging enabled"),
    }

    LoggingGuard { file_writer: guard }
}

type BoxedLayer = Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync>;

fn build_file_layer(log_dir: &str, max_files: usize) -> std::io::Result<(BoxedLayer, WorkerGuard)> {
    std::fs::create_dir_all(Path::new(log_dir))?;

    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("watchtower")
        .filename_suffix("log")
        .max_log_files(max_files)
        .build(log_dir)
        .map_err(std::io::Error::other)?;

    let (writer, guard) = tracing_appender::non_blocking(appender);

    let layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        // ANSI colour codes are for terminals; in a file they are noise that breaks
        // grep and log shippers.
        .with_ansi(false)
        .with_writer(writer)
        .boxed();

    Ok((layer, guard))
}
