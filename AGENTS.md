# Agent instructions

Read this before changing anything. It records the invariants that are easy to break
silently and expensive to notice, most of which have already been broken once.

## Verify behaviour, not shape

The original version of this project passed its whole test suite while being unable to
execute a single command: the tests exercised repository CRUD and pure comparison
helpers, and nothing drove the Telegram handler tree. Tests that mirror the
implementation prove nothing.

When you change the control plane, add a test that dispatches a real `Update` through
`telegram::schema()` and asserts on the outgoing `sendMessage` payload
(`tests/telegram_routing.rs`). When you change the data plane, add a test that runs
`engine::scheduler::tick` (`tests/monitoring_engine.rs`).

For any bug fix, confirm the new test fails without the fix. Two tests in this
repository initially passed either way; both were strengthened only because that check
was performed.

## Invariants

**`DialogueState::Idle` must stay the `Default`.** Every guided flow shares one
dialogue storage. If a default ever lands on an active step, that flow's branch matches
every incoming message and swallows all commands. That is exactly what happened before:
`/start` was answered with "that does not look like a valid Solana mint address".

**Branch order in `telegram::schema()` is load-bearing.** Non-private chats, then
commands, then active flow steps, then the fallback. Commands must precede flows so a
user can never be trapped mid-dialogue.

**Authorize every entry point, including flow steps.** Not just the command that starts
a flow. Blocking a user has to take effect on their next message, whatever state they
are in.

**Constraints belong in the schema.** Exactly one target per rule, positive threshold,
known operator, known state, no duplicate rules, no orphans — all `CHECK`s, foreign
keys, or unique indexes. Application-level validation is a nicer error message on top,
never the only guard.

**Never `unwrap_or` a value read from the database.** Parse it and fail. A coerced
operator silently produced wrong alerts.

**One error path per handler.** Command and flow bodies return
`crate::error::Result<()>` and go through `reply::finish`. A bare `?` in a handler
returns the error to the dispatcher, where it is logged and the user is left with a
chat that never replies.

**Alerting is edge-triggered.** Firing state lives in `rules.state` so it survives a
restart. If a value cannot be read, leave the rule's state alone — treating an outage
as recovery produces a duplicate alert when the provider returns.

**Isolate per-rule failures in a tick.** One unreachable target must not stop the other
rules. Only a whole-tick failure (such as failing to read the rule list) may propagate.

**Configuration seeds; the database decides.** `ADMIN_TELEGRAM_IDS` creates missing
users on startup and nothing more. It must never re-promote a demoted admin or become a
parallel authorization path.

**Never let the last active admin be removed.** There is no recovery except editing
SQLite by hand.

## Migrations

Applied in a transaction with foreign keys **on**, and sqlx stores a checksum per file,
so an applied migration can never be edited — supersede it with a new one.

Do not use `ALTER TABLE ... RENAME` to rebuild a table that other tables reference:
SQLite only rewrites `REFERENCES` clauses when the `foreign_keys` pragma happens to be
on, which makes the resulting schema depend on connection state. Stage data into
FK-free scratch tables, drop child before parent, then create the final tables under
their real names (see `0003_target_relations.sql`).

Run `./scripts/verify-migrations.sh` after any schema change. It applies the chain to a
populated database, which is the check that was missing when migration 0002 shipped
with a foreign-key violation that only appears when `alert_events` has rows.

## Providers

Public APIs rate-limit. Interpret HTTP status (429 is not a parse error), keep requests
retryable only when retrying could help, and prefer one batched call over N. CoinGecko's
public tier accepts exactly one contract address per request, so prices cannot be
batched; wallet balances can, via `getMultipleAccounts`.

## Chat output

All multi-line copy lives in `telegram/copy.rs`, built with `concat!` of single-line
literals. Do not use backslash line continuations for user-facing text: a continuation
strips the next line's leading whitespace only when written exactly right, and getting
it wrong bakes source indentation into the message with nothing to catch it. That
happened once and shipped ragged prompts. The tests in that module assert no message has
ragged indentation, trailing whitespace, a line over 72 characters, or a need to be
split across two sends.

Commands are published to Telegram with `setMyCommands` (`telegram/menu.rs`), which is
what populates the `/` autocomplete list. A command added to the enum but not to the
menu is invisible; a menu entry that does not parse is worse than none, so a test checks
the two agree. Entries taking an id argument stay out of the tap-to-send menu.

Plain text only. Telegram's Markdown modes would require escaping user-supplied labels
and base58 addresses on every path, and one missed escape fails the send — which for an
alert means it is not delivered.

Length is counted in UTF-16 code units, not characters. Everything user-visible goes
through `reply::send_text`.

## Before you finish

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
./scripts/verify-migrations.sh
cargo test --test live_providers -- --ignored   # when provider code changed
```

Then run the daemon. Several defects here were found only by starting the process:
a dependency panicking on a bad token, exit code 0 after a fatal startup failure, and a
shutdown grace period that waited on an already-cancelled token.
