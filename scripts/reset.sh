#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./common.sh
source "${SCRIPT_DIR}/common.sh"

if is_running; then
    echo "ChainSentinel is running. Stop it before resetting."
    exit 1
fi

echo "This will remove the local SQLite database and PID file."
echo "Your .env file and source code will not be touched."
read -r -p "Type 'reset' to continue: " answer

if [[ "${answer}" != "reset" ]]; then
    echo "Aborted."
    exit 0
fi

rm -f "${DATA_DIR}"/*.db
rm -f "${DATA_DIR}"/*.db-wal
rm -f "${DATA_DIR}"/*.db-shm
rm -f "${PID_FILE}"

echo "Local data reset. Start ChainSentinel again with ./scripts/start.sh."
