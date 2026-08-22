# ChainSentinel

A private Solana monitoring daemon controlled entirely through Telegram. You tell it
what to watch in a chat; a long-running Rust process polls prices and balances,
evaluates your rules, and messages you when something crosses a line you drew.

Single binary, SQLite, no web UI, no external services beyond the price API and a
Solana RPC endpoint.

## Scope

**Solana only.** There is no multi-chain support and no abstraction waiting for one:
prices come from CoinGecko's `solana` asset platform, and balances from a Solana
JSON-RPC endpoint. Pointing it at another chain would mean a new provider, a new
address format, and a new balance unit.

Exactly two things can be watched:

| You track | You can alert on | Unit |
|---|---|---|
| A token, by mint address | Its price | USD |
| A wallet, by address | Its **native SOL** balance | SOL |

with `>`, `<`, `>=`, `<=`, `%up`, `%down`.

Everything else is deliberately absent:

- **Read-only.** It never holds a key, signs, or moves funds.
- **No SPL token balances.** A wallet rule watches its SOL balance, not its USDC.
- **No transaction or activity monitoring.** Values only, sampled on an interval.
- **No NFTs, no LP positions, no staking.**
- **No multi-tenancy.** Every authorized user shares one set of targets and rules.
  It is a private tool for a small trusted group.

Limits worth knowing: values are sampled every `POLL_INTERVAL_SECONDS` (60 by
default), so this is not a mempool watcher and will not catch a spike that comes and
goes inside one interval. A token with no CoinGecko listing cannot be tracked at all,
and `/addtoken` refuses it up front rather than letting you build an alert that can
never fire.

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

## Setup

Two values, five minutes.

**1. Create the bot.** Message [@BotFather](https://t.me/BotFather), send
`/newbot`, pick a name and a username. He replies with a token that looks like
`8123456789:AAE...`. That token *is* the bot — anyone holding it controls it.

**2. Find your Telegram ID.** Message [@userinfobot](https://t.me/userinfobot). It
replies with a number. Not your @username — usernames can change, so the bot never
trusts them.

**3. Put both in `.env`.**

```bash
cp .env.example .env
```

```bash
TELEGRAM_BOT_TOKEN=8123456789:AAE...   # step 1
ADMIN_TELEGRAM_IDS=123456789           # step 2
```

**4. Start it, then message your bot `/start`.**

```bash
./scripts/ctl.sh start
```

That is the whole setup. Everything else has a working default, and `.env.example`
documents each option with its range. Configuration is validated at startup: the daemon
**refuses to start** on an invalid value and tells you which variable is wrong, rather
than booting into a half-working state.

Both required values are empty in `.env.example` on purpose. A plausible-looking
placeholder would silently become a working configuration — the previous version
shipped `ADMIN_TELEGRAM_IDS=123456789`, which would have granted a stranger control.

### Using it

The bot registers its commands with Telegram, so typing `/` shows a menu — you do not
have to remember anything. `/start` gives a three-step path, `/help` has a worked
example for every operator, and each guided flow asks one short question at a time with
an example of the answer. `/cancel` gets you out of any of them.

### Settings that matter in production

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

## Running it locally

See [Setup](#setup) for the two values you need first.

```bash
./scripts/ctl.sh start     # builds if needed, waits for a healthy startup
./scripts/ctl.sh follow    # tail the log
```

Also `stop`, `restart`, `status`, `logs`, and `reset`. `start` does not return success
until the daemon has authenticated with Telegram, so a bad token or a broken migration
surfaces immediately instead of leaving a stale pid file behind.

If the bot does not answer `/start`, the log says why.

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
