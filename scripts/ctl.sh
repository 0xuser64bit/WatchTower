#!/usr/bin/env bash
#
# Local process control for WatchTower.
#
# For running the daemon on a workstation. On a server use the systemd unit in
# `deploy/`, which supervises restarts and applies sandboxing this script cannot.
#
#   ./scripts/ctl.sh start|setup|stop|restart|status|logs|follow|reset

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

ENV_FILE="${ROOT}/.env"
DATA_DIR="${ROOT}/data"
LOG_DIR="${ROOT}/logs"
PID_FILE="${DATA_DIR}/watchtower.pid"
# The daemon logs to stdout as well as to its rolling files, so capturing stdout gives
# one complete log including anything emitted before file logging is initialised.
OUT_LOG="${LOG_DIR}/watchtower.out"
BIN="${ROOT}/target/release/watchtower"

die() {
    echo "error: $*" >&2
    exit 1
}

pid_of() {
    [[ -f "${PID_FILE}" ]] && cat "${PID_FILE}" 2>/dev/null || true
}

is_running() {
    local pid
    pid="$(pid_of)"
    [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null
}

ensure_bin() {
    [[ -x "${BIN}" ]] || {
        echo "building release binary..."
        cargo build --release
    }
}

require_env() {
    local missing=0
    if [[ ! -f "${ENV_FILE}" ]]; then
        missing=1
    elif ! grep -Eq '^[[:space:]]*TELEGRAM_BOT_TOKEN=.+' "${ENV_FILE}" \
        || ! grep -Eq '^[[:space:]]*ADMIN_TELEGRAM_IDS=.+' "${ENV_FILE}"; then
        missing=1
    fi

    if (( missing )); then
        if [[ -t 0 && -t 1 ]]; then
            echo "configuration missing; launching setup..."
            cmd_setup || die "setup did not complete"
        else
            die "configuration missing. Run: ./scripts/ctl.sh setup"
        fi
    fi
}

cmd_setup() {
    ensure_bin
    "${BIN}" setup
}

cmd_start() {
    if is_running; then
        echo "already running (pid $(pid_of))"
        return 0
    fi

    require_env
    mkdir -p "${DATA_DIR}" "${LOG_DIR}"

    ensure_bin

    echo "starting..."
    nohup "${BIN}" >>"${OUT_LOG}" 2>&1 &
    local pid=$!
    echo "${pid}" >"${PID_FILE}"

    # Startup validates configuration, runs migrations, and authenticates with
    # Telegram, any of which can fail. Reporting success before that has happened
    # would hide the most common failures behind a stale pid file.
    for _ in $(seq 1 20); do
        sleep 0.5
        if ! kill -0 "${pid}" 2>/dev/null; then
            rm -f "${PID_FILE}"
            echo "--- last output ---" >&2
            tail -n 20 "${OUT_LOG}" >&2 || true
            die "exited during startup"
        fi
        if grep -q "authenticated with Telegram" "${OUT_LOG}" 2>/dev/null; then
            break
        fi
    done

    echo "running (pid ${pid})"
    echo "logs: ./scripts/ctl.sh follow"
}

cmd_stop() {
    if ! is_running; then
        rm -f "${PID_FILE}"
        echo "not running"
        return 0
    fi

    local pid
    pid="$(pid_of)"
    echo "stopping (pid ${pid})..."
    kill -TERM "${pid}"

    # SIGTERM drains handlers, checkpoints the write-ahead log, and closes the pool.
    for _ in $(seq 1 30); do
        kill -0 "${pid}" 2>/dev/null || {
            rm -f "${PID_FILE}"
            echo "stopped"
            return 0
        }
        sleep 1
    done

    echo "graceful shutdown timed out; sending SIGKILL" >&2
    kill -KILL "${pid}" 2>/dev/null || true
    rm -f "${PID_FILE}"
}

cmd_status() {
    if is_running; then
        echo "running (pid $(pid_of))"
    else
        echo "not running"
    fi

    echo "binary:   ${BIN}"
    echo "data:     ${DATA_DIR}"
    echo "logs:     ${LOG_DIR}"
    echo
    echo "Live engine and provider health is available from the bot itself: /status"
}

cmd_logs() {
    [[ -f "${OUT_LOG}" ]] || die "no log at ${OUT_LOG}"
    tail -n "${1:-100}" "${OUT_LOG}"
}

cmd_follow() {
    [[ -f "${OUT_LOG}" ]] || die "no log at ${OUT_LOG}"
    tail -n 50 -f "${OUT_LOG}"
}

cmd_reset() {
    is_running && die "stop the daemon before resetting"

    echo "This deletes the local database: tracked tokens, wallets, alert rules,"
    echo "alert history, and the users table. Your .env is not touched."
    read -r -p "Type 'reset' to confirm: " answer
    [[ "${answer}" == "reset" ]] || {
        echo "aborted"
        return 0
    }

    rm -f "${DATA_DIR}"/*.db "${DATA_DIR}"/*.db-wal "${DATA_DIR}"/*.db-shm "${PID_FILE}"
    echo "local data removed; bootstrap admins are re-seeded on next start"
}

usage() {
    cat <<'USAGE'
WatchTower local process control

  start     build if needed, start in the background, wait for a healthy startup
  setup     interactive configuration wizard; writes .env
  stop      graceful shutdown (SIGTERM), escalating to SIGKILL after 30s
  restart   stop then start
  status    whether the process is running and where its files are
  logs [n]  print the last n lines (default 100)
  follow    tail the log
  reset     delete the local database after confirmation

On a server, use deploy/watchtower.service instead.
USAGE
}

case "${1:-}" in
start) cmd_start ;;
setup) cmd_setup ;;
stop) cmd_stop ;;
restart)
    cmd_stop
    cmd_start
    ;;
status) cmd_status ;;
logs) cmd_logs "${2:-100}" ;;
follow) cmd_follow ;;
reset) cmd_reset ;;
-h | --help | help | "") usage ;;
*)
    echo "unknown command: $1" >&2
    usage >&2
    exit 1
    ;;
esac
