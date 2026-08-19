# Control Scripts

These scripts manage the local ChainSentinel daemon. They assume a `.env` file at the project
root and a release binary at `target/release/chainsentinel`.

## Quick Start

```bash
./scripts/start.sh
./scripts/status.sh
./scripts/logs.sh --follow
```

The `start.sh` script will:

1. Create `.env` from `.env.example` if it does not exist.
2. Validate that `TELEGRAM_BOT_TOKEN` is set and not the placeholder.
3. Build the release binary if it is missing.
4. Start the daemon in the background and write its PID to `data/chainsentinel.pid`.

## Commands

| Command | Description |
|---|---|
| `./scripts/start.sh` | Start ChainSentinel in the background. |
| `./scripts/stop.sh` | Stop the running daemon with SIGTERM. |
| `./scripts/restart.sh` | Stop and start again. |
| `./scripts/status.sh` | Show whether the daemon is running. |
| `./scripts/logs.sh` | Print recent log lines. |
| `./scripts/logs.sh -f` | Follow logs live. |
| `./scripts/reset.sh` | Remove local SQLite data after confirming. |
| `./scripts/manage.sh <command>` | Unified wrapper around the commands above. |

## Log and Data Paths

- Logs: `logs/chainsentinel.log`
- PID file: `data/chainsentinel.pid`
- SQLite database: `data/chainsentinel.db`
