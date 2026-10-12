#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source ~/.cargo/env
# Release the verification lock before starting long-lived development children.
python3 scripts/verify-resources.py -- cargo build --locked -q -p devbox-cli
exec python3 scripts/dev-session.py
