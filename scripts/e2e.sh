#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source ~/.cargo/env
exec python3 scripts/verify-resources.py -- pnpm --filter app exec playwright test "$@"
