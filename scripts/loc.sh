#!/usr/bin/env bash
# SC8 source observation (not a budget gate).
set -euo pipefail
exec python3 "$(dirname "$0")/../xtask/loc.py" "${1:-60000}"
