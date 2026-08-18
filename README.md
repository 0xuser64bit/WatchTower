# ChainSentinel

A private Telegram-controlled Solana monitoring and alerting platform.

ChainSentinel is a single long-running Rust daemon. Telegram is only the control interface; the
monitoring engine, rule evaluator, provider layer, and persistence are the actual system.

## Architecture

```
chainsentinel (single binary)
├── telegram/    control plane (auth, commands, guided flows)
├── engine/      monitoring loop + scheduler
├── rules/       generic rule evaluation
├── providers/   price + Solana RPC abstraction
├── alerts/      dispatch, dedup, cooldown
├── db/          SQLite + sqlx repositories
└── config/      typed settings loader
```

## Stack

- Rust 2021, tokio
- teloxide for Telegram long polling
- reqwest with rustls
- SQLite via sqlx (WAL mode)
- tracing for structured logging
- governor-ready endpoint pool with circuit breaking
- systemd deployment

## Commands

| Command | Description |
|---|---|
| `/start` | Main menu |
| `/help` | Help |
| `/addtoken` | Guided token tracking flow |
| `/tokens` | List tracked tokens |
| `/addwallet` | Guided wallet tracking flow |
| `/wallets` | List tracked wallets |
| `/addalert` | Guided alert rule flow |
| `/alerts` | List alert rules |
| `/history` | Recent alert events |
| `/admin` | Admin panel |
| `/listusers` | List authorized users |
| `/addadmin <id>` | Grant admin |
| `/demote <id>` | Revoke admin |
| `/block <id>` | Block user |
| `/unblock <id>` | Unblock user |

## Configuration

Copy `.env.example` to `.env` and fill in the values:

```
TELEGRAM_BOT_TOKEN=...
ADMIN_TELEGRAM_IDS=...
DATABASE_URL=sqlite://data/chainsentinel.db
COINGECKO_API_URL=...
PRICE_FALLBACK_URLS=...
SOLANA_RPC_ENDPOINTS=...
POLL_INTERVAL_SECONDS=60
```

## Build

```bash
cargo build --release
cargo test
```

The release binary is `target/release/chainsentinel`.

## Deploy on Ubuntu

1. Create the service user:

```bash
sudo useradd --system --home /opt/chainsentinel --shell /usr/sbin/nologin chainsentinel
```

2. Copy the release binary, `.env`, and create the data directory:

```bash
sudo mkdir -p /opt/chainsentinel/data
sudo cp target/release/chainsentinel /opt/chainsentinel/
sudo cp .env /opt/chainsentinel/
sudo chown -R chainsentinel:chainsentinel /opt/chainsentinel
```

3. Install the systemd unit:

```bash
sudo cp deploy/chainsentinel.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now chainsentinel
```

4. Check status:

```bash
sudo systemctl status chainsentinel
journalctl -u chainsentinel -f
```

## Security Model

- Telegram user ID is the only identity. Usernames are never trusted.
- All commands pass an authorization middleware.
- Admin commands re-check the role.
- Blocked users are rejected at the auth layer.
- No secrets are stored in code or the database.
