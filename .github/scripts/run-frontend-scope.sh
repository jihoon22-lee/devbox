#!/usr/bin/env bash

set -euo pipefail

action=${1:?frontend action is required}
scope=${2:?frontend scope is required}
packages=${3:-}

case "$action" in
  build|test|typecheck) ;;
  *) echo "unsupported frontend action: $action" >&2; exit 2 ;;
esac

if [[ $scope == none ]]; then
  echo "No frontend packages changed; $action is not required."
  exit 0
fi

selected_directories=()
filters=()
if [[ $scope == apps ]]; then
  IFS=',' read -r -a requested_directories <<<"$packages"
  for directory in "${requested_directories[@]}"; do
    [[ -n $directory ]] || continue
    if [[ ! $directory =~ ^(apps|packages)/[a-z0-9]+(-[a-z0-9]+)*$ || ! -f $directory/package.json ]]; then
      echo "invalid scoped frontend package: $directory" >&2
      exit 2
    fi
    selected_directories+=("$directory")
    filters+=(--filter "./$directory")
  done
  if (( ${#selected_directories[@]} == 0 )); then
    echo "Scoped frontend run has no packages." >&2
    exit 2
  fi
elif [[ $scope != all ]]; then
  echo "unsupported frontend scope: $scope" >&2
  exit 2
fi

package_script() {
  python3 -c 'import json, sys; print(json.load(open(sys.argv[1], encoding="utf-8")).get("scripts", {}).get(sys.argv[2], ""))' \
    "$1/package.json" "$2"
}

if [[ $action == typecheck ]]; then
  # App and typed package build scripts already run tsc. Only cover tsconfig
  # packages whose build step did not perform a TypeScript check.
  if [[ $scope == all ]]; then
    selected_directories=(apps/* packages/*)
  fi
  checked=0
  for directory in "${selected_directories[@]}"; do
    [[ -d $directory && -f $directory/package.json && -f $directory/tsconfig.json ]] || continue
    build_script=$(package_script "$directory" build)
    if [[ $build_script == *tsc* ]]; then
      continue
    fi
    (cd "$directory" && pnpm exec tsc --noEmit)
    checked=$((checked + 1))
  done
  echo "Additional TypeScript checks completed for $checked package(s)."
elif [[ $scope == all ]]; then
  pnpm "$action"
else
  pnpm -r "${filters[@]}" "$action"
  echo "Frontend $action completed for ${#selected_directories[@]} selected package(s)."
fi
