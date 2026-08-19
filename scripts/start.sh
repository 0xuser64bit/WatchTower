#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# shellcheck source=./common.sh
source "${SCRIPT_DIR}/common.sh"

cd "${PROJECT_ROOT}"

if is_running; then
    echo "ChainSentinel is already running (PID $(get_pid))."
    exit 0
fi

if [[ ! -f "${ENV_FILE}" ]]; then
    if [[ -f "${ENV_EXAMPLE}" ]]; then
        cp "${ENV_EXAMPLE}" "${ENV_FILE}"
        echo "Created ${ENV_FILE} from ${ENV_EXAMPLE}. Edit it before first launch."
    else
        echo "Missing ${ENV_FILE}. Create one before starting ChainSentinel." >&2
        exit 1
    fi
fi

if ! grep -Eq '^TELEGRAM_BOT_TOKEN=.+' "${ENV_FILE}" \
    || grep -Eq '^TELEGRAM_BOT_TOKEN=(replace_me)?\s*$' "${ENV_FILE}"; then
    echo "TELEGRAM_BOT_TOKEN is missing or still set to replace_me in ${ENV_FILE}." >&2
    exit 1
fi

mkdir -p "${DATA_DIR}" "${LOG_DIR}"

if [[ ! -x "${BIN}" ]]; then
    echo "Building ChainSentinel release binary..."
    cargo build --release
fi

echo "Starting ChainSentinel..."
nohup "${BIN}" >>"${BOOTSTRAP_LOG}" 2>&1 &

pid=$!
echo "${pid}" >"${PID_FILE}"

if ! kill -0 "${pid}" 2>/dev/null; then
    echo "ChainSentinel failed to start. Check ${BOOTSTRAP_LOG}." >&2
    rm -f "${PID_FILE}"
    exit 1
fi

echo "ChainSentinel started (PID ${pid})."
echo "Logs: ${LOG_FILE} (symlink to latest daily log)"
echo "Watch logs: ./scripts/logs.sh -f"
