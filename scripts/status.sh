#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./common.sh
source "${SCRIPT_DIR}/common.sh"

if is_running; then
    pid="$(get_pid)"
    echo "ChainSentinel is running (PID ${pid})."
else
    echo "ChainSentinel is not running."
fi

echo "Log file: ${LOG_FILE}"
echo "Data dir: ${DATA_DIR}"
