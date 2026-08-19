#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./common.sh
source "${SCRIPT_DIR}/common.sh"

if ! is_running; then
    echo "ChainSentinel is not running."
    rm -f "${PID_FILE}"
    exit 0
fi

pid="$(get_pid)"
echo "Stopping ChainSentinel (PID ${pid})..."

kill -TERM "${pid}"

if wait_for_exit "${pid}" 15; then
    echo "ChainSentinel stopped."
else
    echo "Graceful shutdown timed out. Sending SIGKILL." >&2
    kill -KILL "${pid}" 2>/dev/null || true
fi

rm -f "${PID_FILE}"
