PRAGMA foreign_keys = ON;

DROP TABLE provider_state;

CREATE TABLE tokens_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mint_address TEXT NOT NULL UNIQUE,
    symbol TEXT,
    name TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

INSERT INTO tokens_new (id, mint_address, symbol, name, created_at, deleted_at)
SELECT id, mint_address, symbol, name, created_at, deleted_at
FROM tokens;

DROP TABLE tokens;
ALTER TABLE tokens_new RENAME TO tokens;

CREATE TABLE rules_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('price','balance')),
    target_type TEXT NOT NULL CHECK (target_type IN ('token','wallet')),
    target_ref TEXT NOT NULL,
    metric TEXT NOT NULL,
    operator TEXT NOT NULL,
    threshold REAL NOT NULL,
    time_window_seconds INTEGER,
    cooldown_seconds INTEGER NOT NULL DEFAULT 300,
    max_triggers INTEGER,
    reference_value REAL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

INSERT INTO rules_new (
    id, kind, target_type, target_ref, metric, operator, threshold,
    time_window_seconds, cooldown_seconds, max_triggers, reference_value,
    enabled, created_at, updated_at, deleted_at
)
SELECT
    id, kind, target_type, target_ref, metric, operator, threshold,
    time_window_seconds, cooldown_seconds, max_triggers, reference_value,
    enabled, created_at, updated_at, deleted_at
FROM rules
WHERE kind IN ('price','balance');

DROP TABLE rules;
ALTER TABLE rules_new RENAME TO rules;
