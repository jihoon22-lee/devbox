#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
mode=${1:-generate}
case "$mode" in generate|--check-only) ;; *) echo "usage: $0 [--check-only]" >&2; exit 2 ;; esac
if [[ $mode == generate ]]; then
  cargo test --locked -p devbox-knowledge -p devbox-api-studio --test typescript
fi
# Cargo's ordinary test run already executes the exporters. Canonicalize once
# before comparing both tracked and newly generated files with the index.
directories=(packages/knowledge-features/src/generated packages/api-studio-features/src/generated)
pnpm exec biome format --write "${directories[@]}" >/dev/null
git diff --exit-code -- "${directories[@]}"
if [[ -n $(git ls-files --others --exclude-standard -- "${directories[@]}") ]]; then
  echo "Untracked generated bindings; review and add the generated files." >&2
  exit 1
fi
