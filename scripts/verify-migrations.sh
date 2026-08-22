#!/usr/bin/env bash
#
# Applies the migration chain along every upgrade path a real deployment can take and
# asserts the resulting schema, data, and constraints.
#
# Covers the populated-database path because migration 0002 drops the `rules` parent
# table while `alert_events` rows still reference it. Empty databases do not exercise
# that foreign-key path.
#
# 0002 cannot be edited: sqlx records a checksum per migration, so changing an applied
# migration makes the daemon refuse to start. It is instead superseded by 0003, which
# rebuilds the same tables in child-before-parent order. Both real paths reach 0002
# while the database is still empty, which is why they succeed.

set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

command -v sqlite3 >/dev/null 2>&1 || {
    echo "sqlite3 is required" >&2
    exit 1
}

workdir="$(mktemp -d)"
trap 'rm -rf "${workdir}"' EXIT

failures=0

fail() {
    echo "  FAIL: $*" >&2
    failures=$((failures + 1))
}

expect() {
    local label="$1" expected="$2" actual="$3"
    if [[ "${actual}" == "${expected}" ]]; then
        echo "  ok: ${label}"
    else
        fail "${label}: expected '${expected}', got '${actual}'"
    fi
}

# Applies one migration file the way sqlx does: a single transaction, foreign keys
# enabled on the connection.
apply() {
    local db="$1" migration="$2"
    sqlite3 "${db}" "PRAGMA foreign_keys = ON; BEGIN; $(cat "${migration}") COMMIT;"
}

apply_all() {
    local db="$1"
    for migration in migrations/*.sql; do
        apply "${db}" "${migration}" || fail "${migration} did not apply cleanly"
    done
}

seed_legacy_data() {
    sqlite3 "$1" <<'SQL'
PRAGMA foreign_keys = ON;
INSERT INTO users (telegram_id, role) VALUES (1, 'admin'), (2, 'user');
INSERT INTO tokens (mint_address, symbol) VALUES ('MINT_LIVE', 'LIVE'), ('MINT_DEAD', 'DEAD');
UPDATE tokens SET deleted_at = '2026-01-01T00:00:00.000Z' WHERE mint_address = 'MINT_DEAD';
INSERT INTO wallets (address, label) VALUES ('WALLET_LIVE', 'Treasury');
INSERT INTO rules (kind, target_type, target_ref, metric, operator, threshold, cooldown_seconds, enabled)
VALUES ('price',   'token',  'MINT_LIVE',      'price',   '>',              1.5, 300, 1),
       ('price',   'token',  'MINT_UNTRACKED', 'price',   'pct_change_up', 10.0,  60, 1),
       ('balance', 'wallet', 'WALLET_LIVE',    'balance', '<=',             5.0, 300, 0),
       ('price',   'token',  'MINT_LIVE',      'price',   '>',              9.9, 300, 1);
UPDATE rules SET deleted_at = '2026-01-01T00:00:00.000Z' WHERE id = 4;
INSERT INTO alert_events (rule_id, current_value, threshold_value, message, dedup_key)
VALUES (1, 2.0, 1.5, 'legacy rendered blob', 'legacy-key');
SQL
}

assert_healthy_schema() {
    local db="$1"
    expect "no scratch tables remain" "0" \
        "$(sqlite3 "${db}" "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'mig_%';")"
    # A rename-based rebuild silently leaves REFERENCES pointing at the temporary name
    # unless foreign keys happen to be on, which would make the schema depend on
    # connection pragma state.
    expect "no dangling foreign key targets" "0" \
        "$(sqlite3 "${db}" "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND sql LIKE '%REFERENCES %_new(%';")"
    expect "integrity check" "ok" "$(sqlite3 "${db}" "PRAGMA integrity_check;")"
    expect "foreign key check" "" "$(sqlite3 "${db}" "PRAGMA foreign_key_check;")"
    expect "expected tables exist" "alert_events|rules|tokens|users|wallets" \
        "$(sqlite3 "${db}" "SELECT group_concat(name, '|') FROM (SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%' ORDER BY name);")"
}

assert_constraints() {
    local db="$1"
    local token_id
    token_id="$(sqlite3 "${db}" "SELECT id FROM tokens LIMIT 1;")"

    reject() {
        local label="$1" statement="$2"
        if sqlite3 "${db}" "PRAGMA foreign_keys = ON; ${statement}" >/dev/null 2>&1; then
            fail "${label}: accepted, but the database should have rejected it"
        else
            echo "  ok: ${label} rejected"
        fi
    }

    sqlite3 "${db}" "PRAGMA foreign_keys = ON; INSERT INTO rules (token_id, operator, threshold) VALUES (${token_id}, 'gte', 42.0);"

    reject "duplicate rule"         "INSERT INTO rules (token_id, operator, threshold) VALUES (${token_id}, 'gte', 42.0);"
    reject "rule with two targets"  "INSERT INTO rules (token_id, wallet_id, operator, threshold) VALUES (${token_id}, 1, 'gt', 1.0);"
    reject "rule with no target"    "INSERT INTO rules (operator, threshold) VALUES ('gt', 1.0);"
    reject "non-positive threshold" "INSERT INTO rules (token_id, operator, threshold) VALUES (${token_id}, 'gt', 0);"
    reject "negative cooldown"      "INSERT INTO rules (token_id, operator, threshold, cooldown_seconds) VALUES (${token_id}, 'lt', 1.0, -1);"
    reject "unknown operator"       "INSERT INTO rules (token_id, operator, threshold) VALUES (${token_id}, 'sideways', 1.0);"
    reject "unknown rule state"     "UPDATE rules SET state = 'wobbling' WHERE id = (SELECT MIN(id) FROM rules);"
    reject "orphan rule"            "INSERT INTO rules (token_id, operator, threshold) VALUES (99999, 'gt', 1.0);"
    reject "duplicate mint"         "INSERT INTO tokens (mint_address) VALUES ((SELECT mint_address FROM tokens LIMIT 1));"
    reject "unknown user role"      "INSERT INTO users (telegram_id, role) VALUES (12345, 'superuser');"
}

echo "=== path 1: fresh database ==="
fresh="${workdir}/fresh.db"
apply_all "${fresh}"
assert_healthy_schema "${fresh}"
sqlite3 "${fresh}" "PRAGMA foreign_keys = ON; INSERT INTO tokens (mint_address, symbol) VALUES ('MINT_NEW', 'NEW');"
assert_constraints "${fresh}"

echo
echo "=== path 2: existing deployment upgrading with live data ==="
# Reaches the current release with 0001+0002 already recorded, then gains data, then
# takes 0003 — the sequence a real upgrade follows.
upgraded="${workdir}/upgraded.db"
apply "${upgraded}" migrations/0001_init.sql
apply "${upgraded}" migrations/0002_mvp_hardening.sql
seed_legacy_data "${upgraded}"
apply "${upgraded}" migrations/0003_target_relations.sql || fail "0003 failed on populated data"

assert_healthy_schema "${upgraded}"

expect "users preserved" "2" "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM users;")"
expect "live token rules migrated" "2" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM rules WHERE token_id IS NOT NULL;")"
expect "wallet rule migrated and stays disabled" "1|0" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*), MIN(enabled) FROM rules WHERE wallet_id IS NOT NULL;")"
expect "operators normalised" "gt|pct_up|lte" \
    "$(sqlite3 "${upgraded}" "SELECT group_concat(operator, '|') FROM (SELECT operator FROM rules ORDER BY id);")"
expect "percentage baseline reset for re-arming" "1" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM rules WHERE operator = 'pct_up' AND reference_value IS NULL;")"
expect "rule pointing at an untracked address keeps working" "1" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM tokens WHERE mint_address = 'MINT_UNTRACKED';")"
expect "soft-deleted token dropped" "0" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM tokens WHERE mint_address = 'MINT_DEAD';")"
expect "soft-deleted rule dropped" "3" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM rules;")"
expect "history preserved as a re-renderable snapshot" "token|MINT_LIVE|LIVE|gt|1.5|2.0" \
    "$(sqlite3 "${upgraded}" "SELECT target_kind||'|'||target_ref||'|'||target_label||'|'||operator||'|'||threshold_value||'|'||observed_value FROM alert_events;")"

echo
echo "=== cascade and re-add behaviour ==="
token_id="$(sqlite3 "${upgraded}" "SELECT id FROM tokens WHERE mint_address = 'MINT_LIVE';")"
sqlite3 "${upgraded}" "PRAGMA foreign_keys = ON; DELETE FROM tokens WHERE id = ${token_id};"
expect "deleting a target removes its rules" "0" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM rules WHERE token_id = ${token_id};")"
expect "history is detached, not deleted" "1|1" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*), SUM(rule_id IS NULL) FROM alert_events;")"
# A removed mint must be available to track again.
sqlite3 "${upgraded}" "PRAGMA foreign_keys = ON; INSERT INTO tokens (mint_address, symbol) VALUES ('MINT_LIVE', 'LIVE');"
expect "a deleted mint can be tracked again" "1" \
    "$(sqlite3 "${upgraded}" "SELECT COUNT(*) FROM tokens WHERE mint_address = 'MINT_LIVE';")"

echo
# Idempotence is a property of sqlx's version tracking, not of the raw SQL (re-running
# a `CREATE TABLE` always fails), so it is asserted in
# `tests/data_integrity.rs::migrations_apply_cleanly_and_are_idempotent` instead.

if ((failures > 0)); then
    echo "migration verification FAILED (${failures} problem(s))" >&2
    exit 1
fi

echo "migration verification passed"
