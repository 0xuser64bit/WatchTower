# ChainSentinel

A private Solana monitoring daemon controlled entirely through Telegram. You tell it
what to watch in a chat; a long-running Rust process polls prices and balances,
evaluates your rules, and messages you when something crosses a line you drew.

Single binary, SQLite, no web UI, no external services beyond the price API and a
Solana RPC endpoint.

## What it does

- Tracks SPL tokens (by mint address) and wallets (by address).
- Alerts on a token's USD price or a wallet's native SOL balance, using absolute
  thresholds or percentage moves.
- Delivers alerts to every active admin, with an audit trail you can read back.
- Manages who is allowed to use the bot at all.

## What it does not do

Stated plainly so nobody builds on an assumption:

- **No transactions.** It never holds a key, signs, or moves funds. It is read-only.
- **No SPL token balances.** Balance rules cover the native SOL balance only.
- **No transaction or account activity monitoring.** Only price and balance values.
- **No multi-chain support.** Solana only.
- **No multi-tenancy.** Every authorized user sees and edits the same directory of
  targets and rules. It is a private tool for a small trusted group.

## How alerting behaves

This is the part worth understanding before you create a rule.

**Alerts are edge-triggered.** A rule fires when its condition *becomes* true and then
stays quiet until the condition clears. `price > 100` fires once when the price crosses
100 — not every minute for as long as it stays there. When the price drops back below
100 the rule re-arms, and the next crossing alerts again.

**Cooldown limits flapping, not duration.** A rule's cooldown is the minimum gap
between alerts for a condition that keeps crossing back and forth. It has no effect on
a condition that simply stays true.

**Percentage rules use a rolling baseline.** `%up 10` takes its baseline from the first
value observed after the rule is created, and re-baselines to the current value every
time it fires — so it tells you about each 10% move, not once about the first one.
Disabling and re-enabling a rule clears the baseline.

**A provider outage never invents or loses an alert.** If a value cannot be read, the
rule keeps its current state, so an outage cannot re-arm a firing rule and produce a
duplicate when the provider recovers. Delivery is at-least-once: if recording succeeds
but sending fails, the send is retried on the next poll.

## Commands

| Command | What it does |
|---|---|
| `/start`, `/help` | Command list, with admin commands shown to admins |
| `/status` | Engine uptime, last poll, provider health, counts, last error |
| `/cancel` | Abandon the current guided step |
| `/addtoken` | Track a token; the mint is checked against the price provider first |
| `/tokens` | Tracked tokens and how many rules depend on each |
| `/deletetoken <id>` | Stop tracking a token, and its rules |
| `/addwallet` | Track a wallet; the balance is read on chain first |
| `/wallets` | Tracked wallets and how many rules depend on each |
| `/deletewallet <id>` | Stop tracking a wallet, and its rules |
| `/addalert` | Create a rule against a tracked target |
| `/alerts` | Rules with their state, last observed value, and baseline |
| `/enablerule <id>` / `/disablerule <id>` | Toggle a rule; enabling re-arms it |
| `/deleterule <id>` | Delete a rule; its history is kept |
| `/history` | Recent alerts |

Admin only:

| Command | What it does |
|---|---|
| `/admin` | Admin panel |
| `/listusers` | Users, roles, and who is blocked |
| `/addadmin <telegram_id>` | Grant admin, creating the user if needed |
| `/demote <telegram_id>` | Revoke admin |
| `/block <telegram_id>` / `/unblock <telegram_id>` | Revoke or restore all access |

Operators: `>`, `<`, `>=`, `<=`, `%up`, `%down`.

Deleting a target deletes the rules that watch it — the reply tells you how many.
Alert history is kept regardless, because it snapshots what fired rather than pointing
at a rule that may be gone.

## Security model

- **Telegram user ID is the only identity.** Usernames are mutable and never trusted.
- **The database is the authority.** `ADMIN_TELEGRAM_IDS` seeds the users table on
  first start and nothing more. Removing an ID does not revoke access; an admin
  demoted through the bot is not re-promoted on restart.
- **Every update is authorized before anything else happens**, including the guided
  flow steps. Unregistered and blocked senders receive an identical refusal, so
  probing cannot tell whether an ID is known.
- **Alert recipients are exactly the active admins**, read from the database at send
  time. Blocking someone stops their alerts.
- **You cannot lock yourself out.** An admin cannot demote or block themselves, and the
  last active admin cannot be removed — there is no recovery path other than editing
  SQLite by hand.
- **Errors shown in chat never contain internals.** Provider URLs, SQL, and filesystem
  paths stay in the logs.
- **The bot token is the only secret.** It is never logged, and never rendered by
  `Debug`.

Anyone holding the bot token controls the bot. Treat `.env` as a credential file
(`chmod 600`, owned by the service user).

## Configuration

Copy `.env.example` to `.env`. Only two values are required:

```bash
TELEGRAM_BOT_TOKEN=   # from @BotFather
ADMIN_TELEGRAM_IDS=   # your numeric Telegram ID, from @userinfobot
```

Everything else has a working default; `.env.example` documents each one. Configuration
is validated at startup and the daemon **refuses to start** on an invalid value rather
than falling back silently.

Two settings matter in production:

- **`COINGECKO_API_KEY`** — the free unauthenticated tier rate-limits hard, and each
  poll costs one request per tracked token because CoinGecko's public tier accepts only
  one contract address per request. Without a key, expect missed polls once you track
  more than a handful of tokens.
- **`SOLANA_RPC_ENDPOINTS`** — give at least two. A failing endpoint is benched for 60
  seconds and traffic moves to the next. The public mainnet endpoint is heavily
  rate-limited and is not suitable on its own. Wallet balances are read in a single
  batched `getMultipleAccounts` call per poll regardless of how many wallets you track.

## Architecture

```
                    ┌─────────────────────────── SQLite (source of truth) ──┐
                    │  users · tokens · wallets · rules · alert_events       │
                    └───────────────▲───────────────────────▲───────────────┘
                                    │                       │
   Telegram ──long poll──▶  telegram/  (control plane)   engine/  (data plane)
                            authorize                    poll on an interval
                            commands, guided flows        │
                            mutate targets and rules      ▼
                                                        rules/   evaluate
                                                          │
                                                          ▼
                            providers/  price, RPC ◀──── alerts/  dispatch
                                                          │
   Telegram ◀────────────────────────────────────────── admins
```

| Module | Responsibility |
|---|---|
| `app` | Bootstrap, task supervision, graceful shutdown |
| `config` | Typed, validated settings from the environment |
| `telegram` | Authorization, command routing, guided flows, rendering |
| `engine` | The polling loop and runtime health |
| `rules` | Rule model and evaluation — pure, no I/O |
| `alerts` | Delivery and message formatting |
| `providers` | Price and Solana RPC clients, with retry and failover |
| `db` | Connection pool, migrations, repositories |

Two design decisions worth knowing:

**Rules reference targets by foreign key.** A rule cannot exist for something that is
not tracked, and deleting a target removes its rules — enforced by the schema, not by
application code. Every invariant that matters (exactly one target, positive threshold,
known operator, known state, no duplicate rules) is a `CHECK` or a unique index.

**Rule state is persisted, not inferred.** Whether a rule is currently firing lives in
the `rules` table, which is what makes edge-triggering survive a restart.

## Running it

```bash
cargo build --release
cp .env.example .env   # then fill in the two required values
./scripts/ctl.sh start
./scripts/ctl.sh follow
```

`./scripts/ctl.sh` also handles `stop`, `restart`, `status`, `logs`, and `reset`.

Message your bot `/start`. If nothing happens, check the log: startup authenticates
with Telegram and reports exactly why it failed.

## Deploying on Ubuntu

```bash
# 1. Service user and layout
sudo useradd --system --home /opt/chainsentinel --shell /usr/sbin/nologin chainsentinel
sudo mkdir -p /opt/chainsentinel/{data,logs}

# 2. Binary and configuration
sudo cp target/release/chainsentinel /opt/chainsentinel/
sudo cp .env /opt/chainsentinel/
sudo chown -R chainsentinel:chainsentinel /opt/chainsentinel
sudo chmod 600 /opt/chainsentinel/.env      # contains the bot token

# 3. Service
sudo cp deploy/chainsentinel.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now chainsentinel

# 4. Verify
systemctl status chainsentinel
journalctl -u chainsentinel -f
```

Migrations run automatically at startup and are applied in a transaction.

**Upgrading:** stop the service, back up the database, replace the binary, start it.
The daemon logs each migration it applies and refuses to start if one fails, leaving the
database untouched.

```bash
sudo systemctl stop chainsentinel
sudo -u chainsentinel sqlite3 /opt/chainsentinel/data/chainsentinel.db ".backup '/opt/chainsentinel/data/backup.db'"
sudo cp chainsentinel /opt/chainsentinel/ && sudo systemctl start chainsentinel
```

`.backup` is used rather than copying the file because it produces a consistent
snapshot while the write-ahead log is in use.

## Operating it

**Is it working?** `/status` in the chat. It reports uptime, when the last poll ran,
whether each provider is answering, how many rules were evaluated, and the last error.
It also performs a database round-trip, so a successful reply means persistence works.

**An alert did not fire.** In order of likelihood:

1. `/alerts` — is the rule `disabled`, or already `firing`? A firing rule stays quiet
   until its condition clears.
2. `/alerts` — is `last seen` present and current? If absent, the target's value is not
   being read; check `/status` for provider health.
3. A percentage rule with `baseline not set yet` needs one poll before it can evaluate.
4. `/status` — is `recipients (active admins)` zero? Alerts are recorded but have
   nowhere to go. `/history` will show them.

**Nothing responds in the chat.** The control plane is down, which means the process
is down: if either half dies the process exits non-zero and systemd restarts it. Check
`journalctl -u chainsentinel`.

**Logs** go to stdout (so `journalctl` is authoritative) and to a daily rolling file in
`LOG_DIR`, kept for `LOG_MAX_FILES` days. Set `RUST_LOG=debug,chainsentinel=trace` for
per-rule detail.

**Data** lives entirely in the SQLite file. Back that up and you have backed up
everything except `.env`.

## Development

```bash
cargo test                      # unit and integration tests, no network needed
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
./scripts/verify-migrations.sh  # applies the migration chain to seeded data

cargo test --test live_providers -- --ignored   # real CoinGecko and Solana RPC
```

Tests drive real code paths rather than mirroring the implementation: the Telegram
tests dispatch through the actual handler tree with the API pointed at a mock server
and assert on the outgoing payload, and the engine tests run real monitoring cycles.
Network-dependent tests are `#[ignore]`d so CI never fails because a third party is
rate-limiting.

CI checks formatting, clippy, tests, doc tests, a release build, the migration gate,
and `cargo audit`.

## Stack

Rust 2021 · tokio · teloxide (long polling) · reqwest with rustls · SQLite via sqlx
(WAL) · tracing · systemd
