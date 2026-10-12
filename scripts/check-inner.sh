#!/usr/bin/env bash
# Internal sequence: caller owns the common-dir resource lock.
set -uo pipefail
failures=()
run() { "$@" || failures+=("$*"); }
run python3 -m unittest discover -s scripts/tests
run cargo fmt --all --check
run cargo fmt --manifest-path app/src-tauri/Cargo.toml --check
run cargo test --manifest-path app/src-tauri/Cargo.toml --locked --lib core:: -- --test-threads="${RUST_TEST_THREADS:-2}"
run cargo clippy --workspace --all-targets --locked -- -D warnings
run cargo test --workspace --locked --no-fail-fast -- --test-threads="${RUST_TEST_THREADS:-2}"
run cargo run -q -p xtask -- gen-ts --check
run bash scripts/banned-words.sh
run pnpm exec biome ci .
run pnpm --filter app typecheck
run pnpm --filter app test
run pnpm --filter app build
run pnpm --filter app exec playwright test
if ((${#failures[@]})); then
  printf 'Failed: %s\n' "${failures[@]}" >&2
  exit 1
fi
