//! Process entry: `watchtower` / `watchtower run` start the daemon, `watchtower setup`
//! writes `.env`.

use crate::{app, setup};
use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "watchtower",
    version,
    about = "Private Telegram-controlled Solana monitoring daemon",
    after_help = "Configuration is read from the process environment, optionally seeded by .env.\n\
                  If required values are missing and you are on a terminal, you will be offered setup."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the monitoring daemon (default)
    Run,
    /// Guided configuration; writes .env
    ///
    /// Explains each variable, live-checks the Telegram bot token plus CoinGecko
    /// and Solana RPC, then writes `.env` (mode 600). Re-running loads existing
    /// values as defaults and backs up the previous file if anything changed.
    Setup,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run_daemon(),
        Command::Setup => setup::run_cli(),
    }
}

fn run_daemon() -> ExitCode {
    let settings = match crate::config::Settings::load() {
        Ok(settings) => settings,
        Err(err) => match setup::recover(err) {
            Ok(settings) => settings,
            Err(code) => return code,
        },
    };
    app::run(settings)
}
