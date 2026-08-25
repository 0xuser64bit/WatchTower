# WatchTower

WatchTower is a private, read-only Solana monitoring daemon controlled through
Telegram. It polls token prices and native SOL balances, evaluates alert rules, and
notifies the active administrators when a condition becomes true.

It is a single Rust binary backed by SQLite. There is no web UI, wallet custody,
transaction signing, multi-chain support, or multi-tenant data separation.

## What It Monitors

| Target | Metric | Unit |
| --- | --- | --- |
| Solana token mint | Price | USD |
| Solana wallet address | Native balance | SOL |

Rules support `>`, `<`, `>=`, `<=`, `%up`, and `%down`. Values are sampled every
`POLL_INTERVAL_SECONDS` (60 seconds by default), so this is interval monitoring rather
than transaction or mempool monitoring. An unlisted token cannot be tracked, but a
temporary price-provider outage does not block saving a valid mint.

Alerts are edge-triggered: a rule fires when its condition becomes true and stays
quiet until the condition clears. Percentage rules use a rolling baseline that is set
on the first observation and reset after each firing. If a provider is unavailable, the
rule keeps its stored state instead of being incorrectly re-armed.

## Using It

WatchTower is a small menu-driven app inside a private Telegram chat. Send `/start`
(or tap the menu button next to the message box) to open the main menu:

```text
🚨 Alerts     🪙 Tokens
👛 Wallets    📜 History
⚙️ Status     ❔ Help
🛡 Admin      (admins only)
```

Everything is done by tapping. Each section opens its own screen with inline buttons
and consistent navigation (**← Back**, **🏠 Menu**, **✕ Cancel**), and the bot edits
the current message in place instead of flooding the chat.

Creating an alert is a guided flow — pick what to watch, pick the tracked item, pick a
condition, type the one value that must be typed, then confirm:

```text
New Alert

🪙 BONK
Condition: below $0.000025
Then wait: 300s before repeating

[ ✅ Create Alert ]
[ ✎ Edit ] [ ✕ Cancel ]
```

Managing things is tap-driven too: the Alerts screen lists each rule with its state
(🟢 armed · 🔴 firing · ⚪ disabled); tapping one opens a detail screen with
enable/disable and delete. Destructive actions (deleting a rule, token or wallet;
removing an admin; blocking a user) always ask for confirmation first. IDs are used
internally but are never something you have to type.

### Command shortcuts

Slash commands remain as shortcuts for people who prefer typing; they open the same
screens the buttons do. Commands that take an id still accept the number shown in a
listing, but tapping is the primary path.

| Command | Purpose |
| --- | --- |
| `/start`, `/menu` | Open the main menu |
| `/help` | How it works, with buttons into the common actions |
| `/status` | Engine, provider, database, and recipient health |
| `/cancel` | Leave an active guided flow |
| `/addtoken` | Verify and track a token mint |
| `/tokens` | Tracked tokens screen |
| `/deletetoken <id>` | Remove a token and its rules |
| `/addwallet` | Verify and track a wallet address |
| `/wallets` | Tracked wallets screen |
| `/deletewallet <id>` | Remove a wallet and its rules |
| `/addalert` | Start the guided alert flow |
| `/alerts` | Your alerts and their state |
| `/enablerule <id>` / `/disablerule <id>` | Enable or pause a rule |
| `/deleterule <id>` | Delete a rule; history remains |
| `/history` | Recent alert events |

Administrators also have `/admin`, `/listusers`, `/addadmin <telegram_id>`,
`/demote <telegram_id>`, `/block <telegram_id>`, and `/unblock <telegram_id>`. The
Admin Panel offers the same actions as buttons, including a guided prompt for the one
value it genuinely needs — a new admin's Telegram user id. Telegram publishes the
everyday and administrator command menus automatically at startup, and the menu button
is pointed at that list.

Removing a target cascades to its rules. Alert history is stored as a snapshot, so it
remains readable after a rule or target is deleted.

## Requirements

- Rust stable (edition 2021)
- SQLite 3 for the migration verification script
- A Telegram bot token from [@BotFather](https://t.me/BotFather)
- At least one administrator's numeric Telegram user ID, available from
  [@userinfobot](https://t.me/userinfobot)
- A CoinGecko-compatible price API and Solana JSON-RPC endpoint (public defaults exist;
  private endpoints are recommended for regular use)

## Configuration

The fastest path is the setup wizard. It explains each variable, live-checks the
bot token and providers, and writes `.env` with mode `600`:

```bash
./scripts/ctl.sh setup
# or: cargo run --release -- setup
```

If you start the daemon with missing required values on a terminal, it offers to
run the same wizard. Non-interactive hosts print `run: watchtower setup` and exit.

To configure by hand instead:

```bash
cp .env.example .env
```

```dotenv
TELEGRAM_BOT_TOKEN=123456789:replace-with-the-token-from-botfather
ADMIN_TELEGRAM_IDS=123456789
```

All other settings are optional and validated at startup. The complete list, defaults,
limits, and provider notes live in [.env.example](.env.example) and in the wizard itself.

Important production settings:

- `COINGECKO_API_KEY`: strongly recommended because the unauthenticated public tier
  rate-limits quickly; prices require one request per tracked token.
- `SOLANA_RPC_ENDPOINTS`: comma-separated failover endpoints. Wallet balances are read
  in one batched RPC request per poll.
- `DATABASE_URL`: defaults to `sqlite://data/watchtower.db`.
- `POLL_INTERVAL_SECONDS`: 10 to 86400 seconds; the default is 60.
- `ALERT_DEFAULT_COOLDOWN_SECONDS`: default cooldown for new rules, from 0 to 86400.
- `ALERT_HISTORY_RETENTION_DAYS`: pruning window, from 1 to 3650 days; default 90.
- `LOG_DIR` and `LOG_MAX_FILES`: rolling file log location and retention. Logs also go
  to stdout, which is the authoritative stream under systemd.

`ADMIN_TELEGRAM_IDS` seeds missing users on startup. The users table is the authority
after that: removing an id from the environment does not undo a database demotion or
block.

## Run Locally

The control script builds a release binary when needed and manages a local process,
database, and logs:

```bash
./scripts/ctl.sh setup
./scripts/ctl.sh start
./scripts/ctl.sh status
./scripts/ctl.sh follow
./scripts/ctl.sh stop
```

Other commands are `restart`, `logs [n]`, and `reset`. `reset` permanently deletes the
local SQLite database and alert history after an interactive confirmation; it does not
touch `.env`.

Startup validates configuration, applies migrations, and authenticates with Telegram.
If the process exits during startup, `ctl.sh` prints the recent output. For a process
that stays up but does not answer, inspect `./scripts/ctl.sh logs 200` and use `/status`
once the bot is reachable.

## Deploy With systemd

The repository includes a hardened unit at
[deploy/watchtower.service](deploy/watchtower.service). A typical Ubuntu layout
is:

```bash
sudo useradd --system --home /opt/watchtower --shell /usr/sbin/nologin watchtower
sudo mkdir -p /opt/watchtower/data /opt/watchtower/logs
sudo cp target/release/watchtower /opt/watchtower/
sudo cp .env /opt/watchtower/
sudo chown -R watchtower:watchtower /opt/watchtower
sudo chmod 600 /opt/watchtower/.env
sudo cp deploy/watchtower.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now watchtower
```

Check the service with:

```bash
systemctl status watchtower
journalctl -u watchtower -f
```

Migrations run transactionally at startup. Before replacing a running binary, stop the
service and create a consistent SQLite backup:

```bash
sudo systemctl stop watchtower
sudo -u watchtower sqlite3 /opt/watchtower/data/watchtower.db \
  ".backup '/opt/watchtower/data/watchtower-backup.db'"
sudo cp target/release/watchtower /opt/watchtower/
sudo systemctl start watchtower
```

The database contains tracked targets, users, rules, and alert history. Back it up
alongside the separately protected `.env` file.

## Architecture

The daemon has two long-running planes sharing `AppState`:

```text
Telegram long polling -> telegram (messages + button taps, authorization, screens, flows)
                               |
                               v
                         SQLite source of truth
                               ^
                               |
providers (CoinGecko, Solana RPC) <- engine (poll, evaluate, dispatch) -> Telegram admins
```

- `telegram`: private-chat routing for both messages and callback queries,
  authorization, the screen renderers and inline keyboards (`screens`, `ui`,
  `callback`), guided `flows`, user-facing `copy`, and all mutations.
- `engine`: interval scheduling, provider reads, rule evaluation, persistence, and
  runtime health.
- `rules`: pure rule types and evaluation logic.
- `providers`: CoinGecko price reads and batched Solana balance reads with retry and
  endpoint failover.
- `alerts`: structured history records and plain-text Telegram delivery.
- `db`: SQLite pool, migrations, repositories, and relational constraints.

SQLite enforces the important data invariants: a rule has exactly one target, targets
must exist, thresholds are positive, operators and states are known, and duplicate
rules are rejected. Rule firing state is persisted so edge-triggering survives a
restart. Individual target or delivery failures are isolated from other rules in the
same poll.

## Security

- Telegram numeric user IDs are the only identity; usernames are not trusted.
- Only registered, unblocked users in private chats can use the bot.
- Active administrators are read from the database at alert-delivery time.
- The last active administrator cannot be demoted or blocked, and an administrator
  cannot remove themselves.
- The bot is read-only: it never asks for or stores a private key or seed phrase.
- User-facing errors omit provider URLs, SQL, filesystem paths, and other internals.
- Treat `.env` as a credential file and keep it mode `600`.

Anyone with the bot token controls the bot. Rotate it through @BotFather if it is
exposed.

## Development And Verification

Run the local checks before submitting a change:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
./scripts/verify-migrations.sh
```

The ignored provider test uses live CoinGecko and Solana endpoints and is intentionally
separate from the offline suite:

```bash
cargo test --test live_providers -- --ignored
```

The automated tests exercise the real Telegram handler tree — both typed messages and
button taps — and the monitoring scheduler through local fakes. After deployment, use
this short smoke test against a private chat:

1. Send `/start`, confirm the main menu appears, and tap through 🚨 Alerts and
   ⚙️ Status.
2. Tap 🪙 Tokens → Add Token, paste a known CoinGecko-listed mint, and confirm its
   current price is shown before you tap to save it.
3. Create an alert entirely by tapping (🚨 Alerts → Create Alert) whose condition is
   currently true, and confirm exactly one Telegram alert arrives after a poll; the
   alert shows as 🔴 firing.
4. Restart the daemon and confirm the rule remains firing without sending a duplicate.
5. Confirm commands and taps are refused in a group chat, and that a blocked user loses
   access on their next message or tap, including during a guided flow.

## License

WatchTower is released under the [MIT License](LICENSE).

## Stack

Rust 2021, Tokio, teloxide, reqwest with rustls, sqlx with SQLite/WAL, tracing, and
systemd.
