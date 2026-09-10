#!/usr/bin/env bash

set -euo pipefail

action=${1:?Rust action is required}
scope=${2:?Rust scope is required}
packages=${3:-}

case "$action" in
  check|clippy|fmt|test) ;;
  *) echo "unsupported Rust action: $action" >&2; exit 2 ;;
esac

if [[ $scope == none ]]; then
  echo "No Rust packages changed; $action is not required."
  exit 0
fi

package_args=()
if [[ $scope == packages ]]; then
  IFS=',' read -r -a selected_packages <<<"$packages"
  for package in "${selected_packages[@]}"; do
    [[ -n $package ]] || continue
    package_args+=(-p "$package")
  done
  if (( ${#package_args[@]} == 0 )); then
    echo "Scoped Rust run has no packages." >&2
    exit 2
  fi
elif [[ $scope != all ]]; then
  echo "unsupported Rust scope: $scope" >&2
  exit 2
fi

# The native integration fixture is a test-only executable, excluded from
# ordinary helper/product builds. Enable it whenever its package is verified.
feature_args=()
if [[ $scope == all || ",$packages," == *,workspace-wsl,* ]]; then
  feature_args=(--features workspace-wsl/test-fixtures)
fi

test_threads=()
if [[ -n ${DEVBOX_VERIFY_RUST_TEST_THREADS:-} ]]; then
  [[ $DEVBOX_VERIFY_RUST_TEST_THREADS =~ ^[1-9][0-9]*$ ]] || { echo "Invalid test thread count" >&2; exit 2; }
  test_threads=(-- "--test-threads=$DEVBOX_VERIFY_RUST_TEST_THREADS")
fi

case "$action" in
  check)
    if [[ $scope == all ]]; then cargo check --workspace "${feature_args[@]}"; else cargo check "${package_args[@]}" "${feature_args[@]}"; fi
    ;;
  clippy)
    if [[ $scope == all ]]; then
      cargo clippy --workspace --all-targets "${feature_args[@]}" -- -D warnings
    else
      cargo clippy "${package_args[@]}" --all-targets "${feature_args[@]}" -- -D warnings
    fi
    ;;
  fmt)
    cargo fmt --all --check
    ;;
  test)
    if [[ $scope == all ]]; then cargo test --workspace "${feature_args[@]}" "${test_threads[@]}"; else cargo test "${package_args[@]}" "${feature_args[@]}" "${test_threads[@]}"; fi
    ;;
esac
