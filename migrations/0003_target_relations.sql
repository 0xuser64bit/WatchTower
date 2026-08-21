-- Rebuilds the data model around real relations between rules and their targets.
--
-- Problems in the previous schema this migration fixes:
--
--  * `rules.target_ref` was free text with no relation to `tokens`/`wallets`, so a
--    rule could poll an address that was never tracked, and deleting a token left
--    its rules running forever against a target the user believed was gone.
--  * `tokens.mint_address` / `wallets.address` were UNIQUE while deletion was a soft
--    delete, so once a token was deleted its mint could never be added again: the
--    insert failed on the unique constraint against the tombstone row.
--  * `alert_events.dedup_key` was UNIQUE and derived from wall-clock time, making it
--    an accidental integrity constraint that could abort a write rather than a
--    deduplication mechanism.
--  * History was stored as a pre-rendered message blob, so it could not be
--    re-formatted and could not survive its rule being deleted.
--
-- Data is staged into FK-free scratch tables, the old tables are dropped
-- child-before-parent, and the final tables are then created under their real names.
-- Deliberately avoids `ALTER TABLE ... RENAME`: SQLite only rewrites `REFERENCES`
-- clauses on rename when `foreign_keys` happens to be ON, which would make the
-- resulting schema depend on connection pragma state rather than on this file.

CREATE TABLE mig_tokens AS
SELECT id, mint_address, symbol, created_at FROM tokens WHERE deleted_at IS NULL;

CREATE TABLE mig_wallets AS
SELECT id, address, label, created_at FROM wallets WHERE deleted_at IS NULL;

CREATE TABLE mig_rules AS
SELECT id, target_type, target_ref, operator, threshold, cooldown_seconds,
       reference_value, enabled, created_at, updated_at
FROM rules
WHERE deleted_at IS NULL
  AND threshold > 0
  AND operator IN ('>','<','>=','<=','pct_change_up','pct_change_down');

CREATE TABLE mig_alert_events AS
SELECT id, rule_id, current_value, threshold_value, triggered_at FROM alert_events;

DROP TABLE alert_events;
DROP TABLE rules;
DROP TABLE tokens;
DROP TABLE wallets;

CREATE TABLE tokens (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    mint_address  TEXT NOT NULL UNIQUE,
    symbol        TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE wallets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    address     TEXT NOT NULL UNIQUE,
    label       TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- Exactly one of token_id / wallet_id is set; the CHECK makes that an invariant the
-- database enforces rather than a convention the application hopes to maintain.
CREATE TABLE rules (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id           INTEGER REFERENCES tokens(id) ON DELETE CASCADE,
    wallet_id          INTEGER REFERENCES wallets(id) ON DELETE CASCADE,
    operator           TEXT NOT NULL CHECK (operator IN ('gt','lt','gte','lte','pct_up','pct_down')),
    threshold          REAL NOT NULL CHECK (threshold > 0),
    cooldown_seconds   INTEGER NOT NULL DEFAULT 300 CHECK (cooldown_seconds >= 0),
    reference_value    REAL,
    state              TEXT NOT NULL DEFAULT 'ok' CHECK (state IN ('ok','firing')),
    enabled            INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    last_value         REAL,
    last_evaluated_at  TEXT,
    last_triggered_at  TEXT,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    CHECK ((token_id IS NOT NULL) <> (wallet_id IS NOT NULL))
);

-- Append-only audit log. Target details are snapshotted so history stays readable
-- after the rule (or its target) is deleted.
CREATE TABLE alert_events (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id          INTEGER REFERENCES rules(id) ON DELETE SET NULL,
    target_kind      TEXT NOT NULL CHECK (target_kind IN ('token','wallet')),
    target_ref       TEXT NOT NULL,
    target_label     TEXT,
    operator         TEXT NOT NULL,
    threshold_value  REAL NOT NULL,
    observed_value   REAL NOT NULL,
    reference_value  REAL,
    triggered_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

INSERT INTO tokens (id, mint_address, symbol, created_at)
SELECT id, mint_address, symbol, created_at FROM mig_tokens;

INSERT INTO wallets (id, address, label, created_at)
SELECT id, address, label, created_at FROM mig_wallets;

-- Rules previously pointed at untracked addresses; materialise those targets so no
-- live rule is silently dropped.
INSERT OR IGNORE INTO tokens (mint_address)
SELECT DISTINCT target_ref FROM mig_rules WHERE target_type = 'token';

INSERT OR IGNORE INTO wallets (address)
SELECT DISTINCT target_ref FROM mig_rules WHERE target_type = 'wallet';

INSERT INTO rules (
    id, token_id, wallet_id, operator, threshold, cooldown_seconds,
    reference_value, enabled, created_at, updated_at
)
SELECT
    r.id,
    t.id,
    w.id,
    CASE r.operator
        WHEN '>'  THEN 'gt'
        WHEN '<'  THEN 'lt'
        WHEN '>=' THEN 'gte'
        WHEN '<=' THEN 'lte'
        WHEN 'pct_change_up'   THEN 'pct_up'
        WHEN 'pct_change_down' THEN 'pct_down'
    END,
    r.threshold,
    MAX(r.cooldown_seconds, 0),
    r.reference_value,
    CASE WHEN r.enabled = 0 THEN 0 ELSE 1 END,
    r.created_at,
    r.updated_at
FROM mig_rules r
LEFT JOIN tokens  t ON r.target_type = 'token'  AND t.mint_address = r.target_ref
LEFT JOIN wallets w ON r.target_type = 'wallet' AND w.address      = r.target_ref
WHERE (t.id IS NOT NULL) <> (w.id IS NOT NULL);

INSERT INTO alert_events (
    id, rule_id, target_kind, target_ref, target_label,
    operator, threshold_value, observed_value, triggered_at
)
SELECT
    e.id,
    r.id,
    CASE WHEN r.token_id IS NOT NULL THEN 'token' ELSE 'wallet' END,
    COALESCE(t.mint_address, w.address),
    COALESCE(t.symbol, w.label),
    r.operator,
    e.threshold_value,
    e.current_value,
    e.triggered_at
FROM mig_alert_events e
JOIN rules r ON r.id = e.rule_id
LEFT JOIN tokens  t ON t.id = r.token_id
LEFT JOIN wallets w ON w.id = r.wallet_id;

DROP TABLE mig_alert_events;
DROP TABLE mig_rules;
DROP TABLE mig_tokens;
DROP TABLE mig_wallets;

-- The scheduler reads exactly this predicate every tick.
CREATE INDEX idx_rules_enabled ON rules(enabled) WHERE enabled = 1;
CREATE INDEX idx_rules_token   ON rules(token_id);
CREATE INDEX idx_rules_wallet  ON rules(wallet_id);

-- One alert per target/operator/threshold: re-creating an identical rule is almost
-- always a mistake and would double every notification.
CREATE UNIQUE INDEX idx_rules_token_unique
    ON rules(token_id, operator, threshold) WHERE token_id IS NOT NULL;
CREATE UNIQUE INDEX idx_rules_wallet_unique
    ON rules(wallet_id, operator, threshold) WHERE wallet_id IS NOT NULL;

CREATE INDEX idx_alert_events_triggered_at ON alert_events(triggered_at DESC);
CREATE INDEX idx_alert_events_rule         ON alert_events(rule_id, triggered_at DESC);
