#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    echo "ChainSentinel process manager"
    echo
    echo "Usage: manage.sh <command>"
    echo
    echo "Commands:"
    echo "  start     Start ChainSentinel"
    echo "  stop      Stop ChainSentinel"
    echo "  restart   Restart ChainSentinel"
    echo "  reset     Remove local data after stopping"
    echo "  status    Show process status"
    echo "  logs      Show recent logs"
    echo "  follow    Follow logs live"
}

case "${1:-}" in
    start)
        "${SCRIPT_DIR}/start.sh"
        ;;
    stop)
        "${SCRIPT_DIR}/stop.sh"
        ;;
    restart)
        "${SCRIPT_DIR}/restart.sh"
        ;;
    reset)
        "${SCRIPT_DIR}/reset.sh"
        ;;
    status)
        "${SCRIPT_DIR}/status.sh"
        ;;
    logs)
        "${SCRIPT_DIR}/logs.sh" --lines 50
        ;;
    follow)
        "${SCRIPT_DIR}/logs.sh" --follow --lines 50
        ;;
    -h|--help|help|"")
        usage
        ;;
    *)
        echo "Unknown command: $1" >&2
        usage >&2
        exit 1
        ;;
esac
