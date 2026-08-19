#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ENV_FILE="${PROJECT_ROOT}/.env"
ENV_EXAMPLE="${PROJECT_ROOT}/.env.example"
DATA_DIR="${PROJECT_ROOT}/data"
LOG_DIR="${PROJECT_ROOT}/logs"
PID_FILE="${DATA_DIR}/chainsentinel.pid"
LOG_FILE="${LOG_DIR}/chainsentinel.log"
BOOTSTRAP_LOG="${LOG_DIR}/bootstrap.log"
BIN="${PROJECT_ROOT}/target/release/chainsentinel"

get_pid() {
    if [[ -f "${PID_FILE}" ]]; then
        cat "${PID_FILE}" 2>/dev/null || true
    fi
}

is_running() {
    local pid
    pid="$(get_pid)"

    if [[ -z "${pid}" ]]; then
        return 1
    fi

    kill -0 "${pid}" 2>/dev/null
}

wait_for_exit() {
    local pid="$1"
    local attempts="${2:-10}"

    for _ in $(seq 1 "${attempts}"); do
        if ! kill -0 "${pid}" 2>/dev/null; then
            return 0
        fi
        sleep 1
    done

    return 1
}

last_log_lines() {
    local lines="${1:-50}"

    if [[ -f "${LOG_FILE}" ]]; then
        tail -n "${lines}" "${LOG_FILE}"
    else
        echo "(no log file yet)"
    fi
}
