#!/usr/bin/env bash

set -euo pipefail

action=${1:?frontend action is required}
scope=${2:?frontend scope is required}
apps=${3:-}

case "$action" in
  build|test) ;;
  typecheck) ;;
  *) echo "unsupported frontend action: $action" >&2; exit 2 ;;
esac

if [[ $scope == none ]]; then
  echo "No frontend packages changed; $action is not required."
  exit 0
fi

filters=()
if [[ $scope == apps ]]; then
  IFS=',' read -r -a selected_apps <<<"$apps"
  for app in "${selected_apps[@]}"; do
    [[ -n $app ]] || continue
    filters+=(--filter "./apps/$app")
  done
  if (( ${#filters[@]} == 0 )); then
    echo "Scoped frontend run has no packages." >&2
    exit 2
  fi
elif [[ $scope != all ]]; then
  echo "unsupported frontend scope: $scope" >&2
  exit 2
fi

if [[ $action == typecheck ]]; then
  if [[ $scope == all ]]; then
    # CSS-only 패키지(packages/tokens 등)는 tsconfig가 없으므로 건너뛴다.
    for dir in apps/* packages/*; do
      [[ -d $dir && -f $dir/tsconfig.json ]] || continue
      (cd "$dir" && pnpm exec tsc --noEmit)
    done
  else
    pnpm "${filters[@]}" exec tsc --noEmit
  fi
elif [[ $scope == all ]]; then
  pnpm "$action"
else
  pnpm "${filters[@]}" "$action"
fi
