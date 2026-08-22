# End-to-end test guide

A walkthrough of every user-facing behaviour, in the order it makes sense to try it.
Each test says what to do, exactly what you should see, and **why the system behaves
that way** — because most of these behaviours are deliberate choices with a failure mode
behind them, and you cannot tell a correct result from a lucky one without knowing which.

Roughly 45 minutes end to end. Tests 1–9 are the ones that must pass before you rely on
this for anything.

**Setup:** a running daemon (`./scripts/ctl.sh start`) and a private chat with your bot.
Keep a second terminal on `./scripts/ctl.sh follow`.

Throughout, USDC is used as the example target because it is reliably priced and sits
near $1, which makes it easy to write a rule that fires on demand:

```
EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
```

---

## 1. The command menu appears

**Do:** open the chat and type a single `/` without sending it.

**Expect:** a list pops up — `start`, `help`, `addtoken`, `addwallet`, `addalert`,
`alerts`, `tokens`, `wallets`, `history`, `status`, `cancel`, and because you are an
admin, `admin` and `listusers`.

**Why it works this way.** Telegram does not read your code. That list exists only
because the daemon calls `setMyCommands` at startup, and it is published in two scopes:
the everyday commands to all private chats, and the admin commands additionally to each
active admin's own chat. So a non-admin never sees commands they cannot use, and a newly
promoted admin gets them without a restart.

You will not see `/deletetoken`, `/deleterule` or `/block` in the menu. That is
intentional: they need an id argument, so a tapped menu entry would send a bare command
and get a usage error. They are in `/help` instead.

**If it fails:** the menu is cached per chat by the Telegram client. Force-close the app
and reopen. If it is still empty, the log will contain `could not publish the command
menu` — a menu failure is deliberately not fatal, because the bot works fine without it.

---

## 2. First-run onboarding

**Do:** send `/start`.

**Expect:** a short message — three numbered steps, a note that `/cancel` always works,
and USDC's address to try.

**Why it works this way.** `/start` adapts to what you have. With nothing set up you get
a task; once you have targets it becomes *"ChainSentinel is watching 2 tokens and 1
wallet with 3 alerts"* plus the three commands you actually reach for. The first message
someone ever sees is not the place for a twenty-command reference — that is `/help`.

---

## 3. Help is actually helpful

**Do:** send `/help`.

**Expect:** sections `HOW ALERTS WORK`, `SET SOMETHING UP`, `WHAT YOU CAN WATCH`,
`LOOK AT THINGS`, `CHANGE THINGS`, `WHAT THE STATES MEAN`, `GOOD TO KNOW`, then `ADMIN`.
Every operator has a worked example (`< 5  balance dips under 5 SOL`), and the arguments
are real (`/addadmin 123456789`, not `/addadmin <telegram_id>`).

**Why it works this way.** Two things in `/help` exist because of specific gaps. `WHAT
THE STATES MEAN` defines `armed`/`firing`/`disabled` — words `/alerts` prints and that
were previously explained nowhere. `GOOD TO KNOW` states Solana-only and read-only,
because nothing in the product said so and the old schema actively implied transaction
monitoring that never existed.

It arrives as **one** message. A test pins the combined length against Telegram's
4096-unit limit, because splitting a reference table across two bubbles reads badly.

---

## 4. Track a token, with verification

**Do:** `/addtoken` → paste the USDC address → `USDC` → `yes`.

**Expect:**
1. A prompt telling you it is 32–44 letters and numbers, where to find it, and USDC as
   an example.
2. After the address: **`Current price: 0.9999 USD.`** then the name question.
3. A confirmation block showing Name and Mint.
4. `Tracking USDC (EPjFW…) as token 1.` and a nudge to `/addalert`.

**Why it works this way.** Step 2 is the important one — the bot calls the price API
*before* saving. A mint with no listing can never satisfy a price rule, so accepting it
would let you build an alert that silently never fires. Seeing a real price also tells
you instantly that you pasted the right address.

The confirmation shows the **full** mint, while later listings abbreviate to `EPjF…Dt1v`.
That asymmetry is deliberate: you verify the full string once, then never wade through 44
characters again.

---

## 5. Bad input does not lose your place

**Do:** `/addtoken` → send `hello` → send `12345` → then paste the real address.

**Expect:** each bad answer gets *"That doesn't look like a Solana address…"* and you
stay on the same step. The real address then works normally.

**Why it works this way.** A rejected answer re-prompts rather than aborting. Nothing is
saved and no state advances, so you can simply try again — being kicked back to the start
after a typo is how people give up.

**Also try:** `/addtoken` then `/cancel`. Expect *"Cancelled adding a token."*

---

## 6. Commands always escape a flow

**Do:** `/addtoken`, then instead of an address send `/tokens`. Then send a bare address.

**Expect:** `/tokens` lists your tokens. The bare address afterwards gets *"That looks
like a Solana address. To watch it: /addtoken / /addwallet"* — **not** silently consumed.

**Why this is the single most important test.** The original version of this bot could
not run one command. Every flow had its own dialogue storage defaulting to that flow's
first active step, so every user was permanently mid-`/addtoken` and the flow branch —
registered before commands — matched everything. `/start` was answered with *"that does
not look like a valid Solana mint address."*

The fix is one dialogue state whose default is `Idle`, with commands checked before flow
steps, and any command clearing an in-progress flow first. That last part is why the
bare address is not consumed: `/tokens` ended the flow. If it had been treated as an
answer, the ordering has regressed.

---

## 7. Track a wallet

**Do:** `/addwallet` → paste any Solana address → `Treasury` → `yes`.

**Expect:** the prompt notes *"I only read the balance — I never need a key or a seed
phrase"*; then the real balance, e.g. `Current balance: 2.5 SOL.`; then confirmation.

**Why it works this way.** Same live verification as tokens, and the prompt says what the
bot cannot do. Anything asking for a wallet address should tell you why it does not need
your key.

**Also try:** `skip` instead of a name. `skip`, `none`, `no` and `-` all work — the old
build accepted only `-`, which nobody guesses.

---

## 8. Create an alert that will actually fire

**Do:** `/addalert` → `token` → `1` → `<` → `2` → `skip` → `yes`.

USDC trades near $1, so `< 2` is already true and will fire on the next poll.

**Expect at the operator step** each option explained against *your* target
(`price goes above a number`), a worked example using real numbers, and a hint that most
people want `<` or `%down`. Then a summary block:

```
  Watching     USDC (EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v)
  Ping when    price < 2 USD
  Then wait    300s before repeating
```

and finally `Alert 1 is live: … I check every 60 seconds…`.

**Why it works this way.** You pick the target by **number from a list**, not by pasting
an address. That is what makes the rule a real foreign-key reference to something you
track. Previously any base58-looking string was accepted, so you could alert on something
you had never added — and deleting a token left its rules polling forever.

The cooldown question accepts `skip` for the configured default. That default
(`ALERT_DEFAULT_COOLDOWN_SECONDS`) was previously parsed, validated, and then ignored
because the flow hardcoded 300.

---

## 9. The alert arrives

**Do:** wait up to one poll interval (60s by default).

**Expect** a message like:

```
⚠️ Token price alert
Target: USDC (EPjF…Dt1v)
Now: 0.9812 USD
Rule: < 2 USD
At: 2026-08-22 19:33:42 UTC
Rule id: 12
```

**Why it works this way.** `Now` is the observed price and `Rule` is what it was compared
against — both, always. The old build put the computed percentage in the "current value"
field for percentage rules, so the message showed neither the price nor the baseline.

`Now: 0.9812` also demonstrates the formatting fix: values render with adaptive
precision. Fixed two-decimal formatting turned every sub-cent token into `0.00`, which is
useless for most of what people track on Solana.

**This is the test that matters most.** Everything before it is verified automatically
against a mock Telegram server; the long-poll connection to Telegram's real API is the
one thing only you can confirm.

---

## 10. It fires once, not forever

**Do:** wait 3–4 more intervals. Then `/alerts` and `/history`.

**Expect:** **no further messages.** `/alerts` shows `[firing]`. `/history` shows exactly
**one** entry.

**Why this is the deepest behavioural fix.** Alerting is *edge-triggered*: a rule fires
when its condition becomes true, then stays quiet until it clears. `price < 2` is
continuously true, so it is worth exactly one notification.

The old build tried to do this with a SHA-256 hash of the rule and the current wall-clock
second. For rules without a time window that key changed every second, so deduplication
never matched and a rule re-notified every cooldown period forever. For rules with one,
the key was a `UNIQUE` column, so a second alert in the same bucket *aborted the database
write*.

Firing state lives in `rules.state`, not memory — see test 14.

**Failure symptom:** a repeat message every ~5 minutes means edge-triggering is broken.

---

## 11. It re-arms and can fire again

**Do:** `/addalert` → `token` → `1` → `>` → `0.5` → `skip` → `yes`. Wait for it to fire.
Then `/deleterule <that id>` and recreate the same rule.

**Expect:** it fires again after recreation.

**Why it works this way.** A rule is quiet only while continuously firing. Once the
condition clears — or the rule is recreated — it is armed again. The point is that
"quiet" is a state, not a permanent mute.

Cheaper alternative if you do not want to wait: test 12 below.

---

## 12. Toggling a rule resets it

**Do:** on a `[firing]` rule: `/disablerule 1`, then `/enablerule 1`. Wait one interval.

**Expect:** `Rule 1 (…) disabled.` then `enabled.` — and the alert **fires again**.

**Why it works this way.** Enabling clears the rule's state, its percentage baseline, and
its last-fired time. Toggling a rule off and on is an explicit request to re-arm it, so
keeping the old trigger time would latch it straight back to firing and silently swallow
the very alert you just asked for.

For a percentage rule, `/enablerule` also says *"Its baseline will be taken from the next
observation."* — otherwise it would measure change across the whole period it was off.

---

## 13. Percentage alerts re-baseline

**Do:** `/addalert` → `token` → `1` → `%down` → `1` → `skip` → `yes`. Then `/alerts`.

**Expect:** initially `baseline not set yet`. After one poll, `baseline 0.9812 USD`. If
the price then falls 1%, it fires and the baseline **moves to the new price**.

**Why it works this way.** `%down 1` means *tell me about every 1% drop*, so after each
alert the reference resets to the current value. The old build fixed the baseline at the
first observation forever, so the rule re-fired every cooldown period until you manually
recreated it — the README even documented recreating it as the workaround.

`/alerts` shows the baseline for exactly this reason: without it you cannot tell what a
percentage rule is currently measuring against.

---

## 14. Firing state survives a restart

**Do:** with a rule showing `[firing]`, run `./scripts/ctl.sh restart`. Wait two
intervals, then `/alerts`.

**Expect:** still `[firing]`, and **no duplicate alert**.

**Why it works this way.** Firing state is a column in `rules`, not in memory. If it were
in memory, every restart would re-notify every currently-true rule — and a daemon under
`Restart=always` restarts more than you think.

---

## 15. `/status` answers "is this working?"

**Do:** send `/status`.

**Expect:**

```
ChainSentinel 0.2.0 — healthy

Engine
  poll interval: 60s
  uptime: 12m 30s
  last poll: 14s ago (2026-08-22 19:33:42 UTC)
  last poll took: 214ms
  polls completed: 12
  rules evaluated last poll: 3

Providers
  price api: ok
  solana rpc: ok
  rpc endpoints configured: 1

Tracked
  tokens: 1
  wallets: 1
  rules: 3 enabled / 3 total

Alerts
  fired since start: 2
  history entries: 2
  recipients (active admins): 1
```

**Why each line is there.** `last poll` proves the monitoring loop is alive — the failure
this catches is a daemon that looks fine but stopped polling, which was previously
undetectable. Provider lines separate *"the feed is down"* from *"your rule is wrong"*.
`recipients (active admins)` is the one people trip over: if it is `0`, alerts are still
recorded but have nowhere to go, and you would only find them in `/history`.

The counter says **fired**, not delivered, because an alert with no reachable recipient is
still recorded. `degraded` appears if a poll has failed or none has happened within two
intervals.

The `Tracked` numbers come from live database queries, so a successful `/status` also
proves persistence works.

---

## 16. Listings show what depends on what

**Do:** `/tokens`, `/wallets`, `/alerts`.

**Expect:**

```
Tracked tokens (1):

1. USDC — EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
   2 alerts
```

and per rule: state, condition, `last seen` value, baseline for percentage rules, and
`last fired`.

**Why it works this way.** The rule count warns you before a delete cascades (test 17).
`last seen` is the fastest way to diagnose a rule that is not firing: if it is missing,
the target's value is not being read at all, which points at `/status` rather than at
your threshold.

---

## 17. Deleting a target reports the collateral damage

**Do:** `/deletetoken 1`.

**Expect:** `Removed token USDC (EPjFWdd…) and 2 alert rules.` Then `/alerts` no longer
lists them, and **`/history` still shows their past alerts**.

**Why it works this way.** A rule cannot exist without its target, so the database
cascades the delete — but silently removing someone's alerts is the kind of surprise that
destroys trust, so the reply counts them.

History survives because `alert_events` snapshots the target name, operator and threshold
at the moment it fired. It is an audit log, not a set of pointers into rows that may be
gone.

---

## 18. A deleted address can be re-added

**Do:** `/addtoken` with the mint you just deleted.

**Expect:** it works, with a new id.

**Why this deserves its own test.** It used to be permanently impossible. Deletion was a
soft delete behind a `UNIQUE` index, so the tombstone row blocked the address forever and
all you saw was *"Failed to add token."* Fixing it needed a schema change, not a patch —
which is why the daemon now hard-deletes and keeps history separately.

---

## 19. Bad arguments say what to do

**Do:** `/enablerule abc`, `/deleterule`, `/deletetoken zero`.

**Expect:** `/enablerule <id>` plus *"The number comes from the listing, e.g. /alerts."*

**Why it works this way.** Two commands previously used typed parsing, so a non-numeric
argument did not match the branch at all and fell through to the generic fallback — you
were told to read `/help` instead of that your id was invalid. Every argument is now
parsed by the handler.

---

## 20. You cannot lock yourself out

**Do:** `/demote <your own id>`, then `/block <your own id>`.

**Expect:** both refused — *"You cannot demote yourself…"*, *"You cannot block
yourself."*

**Then, carefully:** `/addadmin <a second id>`, then from that second account
`/demote <the last remaining admin>`.

**Expect:** *"That is the last active admin…"*

**Why this matters more than it looks.** There is no recovery path. Admin can only be
granted by an admin, so zero admins means nobody can manage the daemon and alerts have no
recipients — recoverable only by hand-editing SQLite. `/demote <self>` used to be enough
to cause it.

Take a backup before this test if the database matters to you.

---

## 21. Non-admins are limited, and probing reveals nothing

**Do:** from a second, unregistered Telegram account, send `/start`.

**Expect:** *"You are not authorized to use this bot."*

**Then:** `/addadmin <that id>`, `/demote <that id>` from your admin account to make them
a plain user. From that account try `/addadmin 999` and `/alerts`.

**Expect:** `/addadmin` → *"This action requires admin privileges."* `/alerts` works —
plain users can read and manage targets, they just cannot manage users.

**Why it works this way.** Unregistered and blocked users get an **identical** refusal.
Distinguishing them would let anyone enumerate which Telegram IDs are known to the
system. `/block` that account and it gets the same message a stranger does.

---

## 22. Group chats are refused

**Do:** add the bot to a group and send `/addtoken`. Then send ordinary text.

**Expect:** *"ChainSentinel only works in a direct message."* for the command; **silence**
for the ordinary text.

**Why it works this way.** Dialogue state is keyed by *chat*, so a flow started in a group
would make the next message from **any** member the answer to that step — an unauthorized
member could supply the mint or send the confirmation. Group support was never coherent
anyway: identity is per user, and alerts are delivered to individual admins.

Ordinary text gets no reply on purpose. Answering every message in a group would be noise
and could trip Telegram's flood limits.

---

## 23. Blocking takes effect immediately, even mid-flow

**Do:** from a second registered account, send `/addtoken` and stop there. From your admin
account, `/block <that id>`. Now have that account paste an address and answer the
remaining questions.

**Expect:** every message refused; nothing is created.

**Why it works this way.** Every flow *step* re-authorizes, not just the command that
started the flow. Otherwise a revocation would not apply until the user finished whatever
they were already doing — and in a group this was an outright bypass.

---

## 24. Graceful shutdown

**Do:** `./scripts/ctl.sh stop`, watching the log.

**Expect:** `monitoring engine received shutdown signal`, `telegram dispatcher stopped`,
`ChainSentinel stopped`, and a prompt exit. Then `./scripts/ctl.sh status` → `not
running`.

**Why it works this way.** SIGTERM stops accepting updates, drains in-flight handlers,
checkpoints the write-ahead log, and closes the pool, with a bounded grace period before
anything is aborted. The WAL checkpoint is what makes the database file self-contained for
a backup.

---

## 25. Misconfiguration fails loudly

**Do:** stop the daemon. Break `.env` (corrupt the token, or set
`POLL_INTERVAL_SECONDS=1`). Run `./scripts/ctl.sh start`.

**Expect:** it refuses to start, names the variable, and exits non-zero:

```
chainsentinel: configuration error: POLL_INTERVAL_SECONDS is invalid: must be between 10 and 86400 (got 1)
```

With a well-formed but wrong token:

```
Telegram rejected the bot token (...). Check TELEGRAM_BOT_TOKEN against @BotFather
```

**Why it works this way.** Three separate failure modes are fixed here. Configuration is
validated up front, so you never get a half-working daemon. The token is checked with an
explicit `getMe` call, because teloxide's dispatcher `expect`s that result internally and
a bad token otherwise surfaced as a panic from inside a dependency. And the exit code is
non-zero, because it used to be `0` — so a supervisor could not distinguish a crash from a
clean shutdown.

The poll-interval floor of 10s is not arbitrary: each poll costs one price request per
tracked token, and a 1-second interval gets your host rate-limited or banned, which
silently stops all alerting.

**Restore your `.env` afterwards.**

---

## Sign-off

Blocking if they fail: **1, 2, 6, 8, 9, 10, 14, 20, 22**.

| # | Behaviour | |
|---|---|---|
| 1 | `/` shows the command menu | ☐ |
| 2 | `/start` gives three steps, not a wall | ☐ |
| 3 | `/help` explains operators and states | ☐ |
| 4 | Token verified against the price API before saving | ☐ |
| 5 | Bad input re-prompts without losing the step | ☐ |
| 6 | Commands escape a flow; text is not swallowed | ☐ |
| 7 | Wallet balance read on chain before saving | ☐ |
| 8 | Alert created against a target picked from a list | ☐ |
| 9 | **Alert actually arrives in Telegram** | ☐ |
| 10 | Fires once, not every cooldown | ☐ |
| 11 | Re-arms and can fire again | ☐ |
| 12 | Toggling resets state, baseline and cooldown | ☐ |
| 13 | Percentage baseline moves after firing | ☐ |
| 14 | Firing state survives a restart | ☐ |
| 15 | `/status` reports engine and provider health | ☐ |
| 16 | Listings show dependent rule counts and last values | ☐ |
| 17 | Delete cascades and says so; history survives | ☐ |
| 18 | A deleted address can be tracked again | ☐ |
| 19 | Bad arguments produce usage, not generic help | ☐ |
| 20 | Self-demote, self-block and last-admin all refused | ☐ |
| 21 | Roles enforced; unknown and blocked look identical | ☐ |
| 22 | Group chats refused; no flow can start there | ☐ |
| 23 | Blocking applies mid-flow | ☐ |
| 24 | Clean shutdown | ☐ |
| 25 | Bad configuration refuses to start, exits non-zero | ☐ |

## If something is wrong

1. `/status` — is the engine polling, are the providers up, is the recipient list empty?
2. `./scripts/ctl.sh logs 200` — every rejection and failure is logged with context.
3. `/alerts` — is the rule `disabled`, already `firing`, or missing a `last seen` value?

The automated suite covers everything here except test 9's real Telegram delivery:

```bash
cargo test                                       # 171 tests, no network
./scripts/verify-migrations.sh                   # schema and data integrity
cargo test --test live_providers -- --ignored     # real CoinGecko and Solana RPC
```

If a manual test fails but the suite passes, the gap is in the suite. Add the case —
`tests/telegram_routing.rs` for control-plane behaviour, `tests/monitoring_engine.rs` for
alerting — and confirm it fails before you fix it.
