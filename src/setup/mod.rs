//! Interactive first-run configuration.
//!
//! `watchtower setup` walks the operator through every variable they actually
//! need, live-checks Telegram / CoinGecko / Solana RPC, and writes `.env` with
//! mode 600. The daemon start path can offer this wizard when required config
//! is missing and stdin is a TTY.

mod flow;
mod live;
mod prompt;
mod writer;

use crate::config::{ConfigError, Settings};
use prompt::InquirePrompter;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

pub use live::{HttpLiveChecker, LiveChecker};
pub use prompt::SetupError;
pub use writer::{mask_secret, parse_env_file, save_env, SaveResult};

pub struct SetupOutcome {
    pub path: std::path::PathBuf,
    pub backup: Option<std::path::PathBuf>,
}

pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::env::var_os("CI").is_none()
}

pub fn run_cli() -> ExitCode {
    let dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("watchtower setup: {err}");
            return ExitCode::FAILURE;
        }
    };

    match run_interactive(&dir) {
        Ok(outcome) => {
            println!();
            print_written(&outcome);
            println!();
            println!("Start the daemon:");
            println!("  ./scripts/ctl.sh start");
            println!("  # or: watchtower");
            ExitCode::SUCCESS
        }
        Err(SetupError::Cancelled) => {
            eprintln!("watchtower setup: cancelled; nothing was written.");
            ExitCode::from(130)
        }
        Err(SetupError::NotInteractive) => {
            eprintln!("watchtower setup requires an interactive terminal.");
            eprintln!("For unattended installs, copy .env.example to .env and edit it.");
            ExitCode::from(2)
        }
        Err(err) => {
            eprintln!("watchtower setup: {err}");
            ExitCode::FAILURE
        }
    }
}

/// After a failed [`Settings::load`], offer the wizard on a TTY and reload.
pub fn recover(err: ConfigError) -> Result<Settings, ExitCode> {
    eprintln!("watchtower: configuration error: {err}");
    if !is_interactive() {
        eprintln!("watchtower: run `watchtower setup` to configure, or see .env.example");
        return Err(ExitCode::FAILURE);
    }

    eprintln!();
    let mut prompter = InquirePrompter;
    match prompt::Prompter::confirm(&mut prompter, "Run setup now?", true) {
        Ok(true) => {
            let dir = std::env::current_dir().map_err(|err| {
                eprintln!("watchtower: {err}");
                ExitCode::FAILURE
            })?;
            match run_interactive(&dir) {
                Ok(outcome) => {
                    print_written(&outcome);
                    Settings::load().map_err(|err| {
                        eprintln!("watchtower: configuration error after setup: {err}");
                        ExitCode::FAILURE
                    })
                }
                Err(SetupError::Cancelled) => {
                    eprintln!("watchtower: setup cancelled");
                    Err(ExitCode::from(130))
                }
                Err(err) => {
                    eprintln!("watchtower setup: {err}");
                    Err(ExitCode::FAILURE)
                }
            }
        }
        _ => {
            eprintln!("watchtower: run `watchtower setup` to configure");
            Err(ExitCode::FAILURE)
        }
    }
}

pub fn run_interactive(dir: &Path) -> Result<SetupOutcome, SetupError> {
    if !is_interactive() {
        return Err(SetupError::NotInteractive);
    }

    let env_path = dir.join(".env");
    let previous = fs::read_to_string(&env_path).ok();
    let existing = previous
        .as_deref()
        .map(writer::parse_env_file)
        .unwrap_or_default();

    let mut prompter = InquirePrompter;
    prompt::Prompter::intro(
        &mut prompter,
        &format!(
            "WatchTower setup\n\
             This writes {} (mode 600) and does not start the bot until you say so.\n\
             \n\
             Required: Telegram bot token + at least one admin id.\n\
             Recommended: CoinGecko API key + a private Solana RPC (public defaults\n\
             work for a quick test and rate-limit under real use).\n\
             Advanced: poll interval, database path, logs, commitment — skip unless\n\
             you have a reason to change them.",
            env_path.display()
        ),
    );
    if previous.is_some() {
        prompt::Prompter::note(
            &mut prompter,
            &format!(
                "Found existing {}. Values are pre-filled; secrets are masked.",
                env_path.display()
            ),
        );
    }

    let timeout = Duration::from_secs(
        existing
            .get("HTTP_TIMEOUT_SECONDS")
            .and_then(|value| value.parse().ok())
            .unwrap_or(10),
    );
    let checker = live::HttpLiveChecker::new(timeout)
        .map_err(|err| SetupError::Invalid(format!("failed to build HTTP client: {err}")))?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let answers = runtime.block_on(flow::collect_answers(
        &mut prompter,
        &checker,
        &existing,
        &env_path,
    ))?;

    let result = writer::save_env(&env_path, &answers, previous.as_deref())?;
    Ok(SetupOutcome {
        path: result.path,
        backup: result.backup,
    })
}

fn print_written(outcome: &SetupOutcome) {
    println!("Wrote {} (mode 600).", outcome.path.display());
    if let Some(backup) = &outcome.backup {
        println!("Previous file saved as {}.", backup.display());
    }
}
