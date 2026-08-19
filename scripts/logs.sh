#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./common.sh
source "${SCRIPT_DIR}/common.sh"

follow=false
lines=50

while [[ $# -gt 0 ]]; do
    case "$1" in
        -f|--follow)
            follow=true
            shift
            ;;
        -n|--lines)
            lines="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: logs.sh [-f|--follow] [-n|--lines N]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ ! -e "${LOG_FILE}" && ! -L "${LOG_FILE}" ]]; then
    echo "No log file at ${LOG_FILE}."
    exit 1
fi

if [[ "${follow}" == "true" ]]; then
    tail -n "${lines}" -F "${LOG_FILE}"
else
    tail -n "${lines}" "${LOG_FILE}"
fi
