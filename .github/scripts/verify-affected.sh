#!/usr/bin/env bash

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

python3 .github/scripts/test-ci-scope.py
python3 .github/scripts/test-ci-scope-runners.py

case ${1:-} in
  --all)
    [[ $# -eq 1 ]] || { echo "usage: $0 [--all|base-ref]" >&2; exit 2; }
    scope_output=$(python3 .github/scripts/resolve-ci-scope.py --all)
    ;;
  --*)
    echo "usage: $0 [--all|base-ref]" >&2
    exit 2
    ;;
  *)
    [[ $# -le 1 ]] || { echo "usage: $0 [--all|base-ref]" >&2; exit 2; }
    if [[ $# -eq 1 ]]; then
      scope_output=$(python3 .github/scripts/resolve-ci-scope.py --working-tree "$1")
    else
      scope_output=$(python3 .github/scripts/resolve-ci-scope.py --working-tree)
    fi
    ;;
esac

scope_value() {
  printf '%s\n' "$scope_output" | sed -n "s/^$1=//p"
}

frontend_scope=$(scope_value frontend_scope)
frontend_packages=$(scope_value frontend_packages)
frontend_apps=$(scope_value frontend_apps)
rust_scope=$(scope_value rust_scope)
rust_packages=$(scope_value rust_packages)
dependency_scope=$(scope_value dependency_scope)

printf '%s\n' "$scope_output"

if [[ $frontend_scope != none ]]; then
  bash .github/scripts/run-frontend-scope.sh build "$frontend_scope" "$frontend_packages"
  node .github/scripts/check-frontend-bundles.mjs "$frontend_scope" "$frontend_apps"
  bash .github/scripts/run-frontend-scope.sh test "$frontend_scope" "$frontend_packages"
  bash .github/scripts/run-frontend-scope.sh typecheck "$frontend_scope" "$frontend_packages"
fi

if [[ $rust_scope != none ]]; then
  bash .github/scripts/run-rust-scope.sh check "$rust_scope" "$rust_packages"
  bash .github/scripts/run-rust-scope.sh clippy "$rust_scope" "$rust_packages"
  bash .github/scripts/run-rust-scope.sh fmt "$rust_scope" "$rust_packages"
  bash .github/scripts/run-rust-scope.sh test "$rust_scope" "$rust_packages"
fi

if [[ $dependency_scope == all ]]; then
  echo "Dependency policy inputs changed; the CI dependency gate will run the authoritative audits."
fi

if [[ $frontend_scope == none && $rust_scope == none ]]; then
  echo "No compiler or test work is required for the detected changes."
fi
