#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/.cargo/env"
profile="${1:-debug}"
case "$profile" in
  debug) flags=() ;;
  release) flags=(--release) ;;
  *) echo 'Expected debug or release' >&2; exit 2 ;;
esac
python3 scripts/verify-resources.py -- cargo build --locked -p devbox-cli --target x86_64-unknown-linux-musl "${flags[@]}"
