# ChainSentinel

ChainSentinel is a private Telegram-controlled Solana monitoring daemon. You manage what to
track entirely through Telegram; the long-running Rust process handles polling, rule
evaluation, persistence, and alert delivery.

## What It Does

- Tracks a directory of Solana tokens and wallets.
- Creates price rules for token mints and native-balance rules for wallets.
- Evaluates threshold and percentage-change operators on a configurable interval.
- Sends alerts to non-blocked admins with cooldown, deduplication, and trigger-rate limits.
- Provides admin user management (promote, demote, block, unblock).

## Architecture

```
chainsentinel (single binary)
├── telegram/    control plane (auth, commands, guided flows)
├── engine/      monitoring loop and sampling
├── rules/       generic rule evaluation
├── providers/   price and Solana RPC abstractions
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
- systemd deployment

## Commands

### Monitoring

| Command | Description |
|---|---|
| `/addtoken` | Guided token tracking flow |
| `/tokens` | List tracked tokens |
| `/deletetoken <id>` | Delete a tracked token |
| `/addwallet` | Guided wallet tracking flow |
| `/wallets` | List tracked wallets |
| `/deletewallet <id>` | Delete a tracked wallet |
| `/addalert` | Guided alert rule flow |
| `/alerts` | List alert rules |
| `/enablerule <id>` | Enable an alert rule |
| `/disablerule <id>` | Disable an alert rule |
| `/deleterule <id>` | Delete an alert rule |
| `/history` | Recent alert events |

### Admin

| Command | Description |
|---|---|
| `/admin` | Admin panel |
| `/listusers` | List authorized users |
| `/addadmin <id>` | Grant admin |
| `/demote <id>` | Revoke admin |
| `/block <id>` | Block user |
| `/unblock <id>` | Unblock user |

## Alert Semantics

- **Price rules** target a token mint and compare the token's USD price.
- **Balance rules** target a wallet and compare its native SOL balance.
- Supported operators are `>`, `<`, `>=`, `<=`, `%up`, and `%down`.
- `%up` and `%down` use the first observed value as a fixed reference. Recreate the rule to
  reset the reference.
- Alerts obey a configurable default cooldown and can optionally enforce a maximum trigger
  count per time window.

## Configuration

Copy `.env.example` to `.env` and fill in the values:

```
TELEGRAM_BOT_TOKEN=...
ADMIN_TELEGRAM_IDS=...
DATABASE_URL=sqlite://data/chainsentinel.db
COINGECKO_API_URL=https://api.coingecko.com/api/v3
PRICE_FALLBACK_URLS=
SOLANA_RPC_ENDPOINTS=https://api.mainnet-beta.solana.com
SOLANA_RPC_COMMITMENT=confirmed
POLL_INTERVAL_SECONDS=60
ALERT_DEFAULT_COOLDOWN_SECONDS=300
RUST_LOG=info,chainsentinel=debug
```

- `ADMIN_TELEGRAM_IDS` is a comma-separated list of numeric Telegram user IDs.
- `SOLANA_RPC_ENDPOINTS` is a comma-separated list. The first is used as the primary, and
  unhealthy endpoints are temporarily skipped.
- `PRICE_FALLBACK_URLS` may contain `{coin}` and `{mint}` placeholders.
- `SOLANA_RPC_COMMITMENT` must be `processed`, `confirmed`, or `finalized`.

## Build and Test

```bash
cargo build --release
cargo test
cargo clippy --all-targets --all-features -- -D warnings
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

## Development

The repository includes a CI workflow that checks formatting, clippy, tests, and a release
build on every push and pull request.
