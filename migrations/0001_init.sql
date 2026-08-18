PRAGMA foreign_keys = ON;

CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id INTEGER NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin','user')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    blocked_at TEXT
);

CREATE TABLE tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mint_address TEXT NOT NULL UNIQUE,
    symbol TEXT,
    name TEXT,
    decimals INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

CREATE TABLE wallets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL UNIQUE,
    label TEXT,
    last_seen_signature TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

CREATE TABLE rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('price','balance','activity')),
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

CREATE TABLE alert_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id INTEGER NOT NULL REFERENCES rules(id),
    current_value REAL NOT NULL,
    threshold_value REAL NOT NULL,
    message TEXT NOT NULL,
    dedup_key TEXT NOT NULL UNIQUE,
    triggered_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX idx_alert_events_rule ON alert_events(rule_id, triggered_at);

CREATE TABLE provider_state (
    key TEXT PRIMARY KEY,
    last_success_at TEXT,
    last_error TEXT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT
);
