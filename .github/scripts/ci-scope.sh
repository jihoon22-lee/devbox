#!/usr/bin/env bash

set -euo pipefail

base_sha=${1:?base SHA is required}
head_sha=${2:?head SHA is required}
output_file=${GITHUB_OUTPUT:-/dev/stdout}

if [[ $base_sha =~ ^0+$ ]]; then
  base_sha="${head_sha}^"
fi

# Compare from the common ancestor so a PR that is behind main is scoped only
# to its own changes, not to unrelated commits merged into main afterwards.
diff_base=$(git merge-base "$base_sha" "$head_sha" 2>/dev/null || printf '%s' "$base_sha")

declare -A frontend_apps=()
declare -A rust_apps=()
frontend_all=false
rust_all=false
changed_count=0

while IFS= read -r path; do
  [[ -n $path ]] || continue
  changed_count=$((changed_count + 1))

  case "$path" in
    .github/workflows/*|.github/scripts/*)
      frontend_all=true
      rust_all=true
      ;;
    package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig*.json|vitest*.ts|packages/*)
      frontend_all=true
      ;;
    Cargo.toml|Cargo.lock|rust-toolchain*|.cargo/*|crates/*)
      rust_all=true
      ;;
    apps/*)
      app=${path#apps/}
      app=${app%%/*}
      if [[ -z $app || ! -f apps/$app/package.json ]]; then
        frontend_all=true
      else
        frontend_apps["$app"]=1
      fi
      if [[ $path == apps/"$app"/src-tauri/* ]]; then
        if [[ ! -f apps/$app/src-tauri/Cargo.toml ]]; then
          rust_all=true
        else
          rust_apps["$app"]=1
        fi
      fi
      ;;
    docs/*|README|README.*|LICENSE|LICENSE.*|*.md|.gitignore)
      # Documentation-only changes do not need compiler work.
      ;;
    *)
      # Unknown workspace-level changes fail safe to the complete gates.
      frontend_all=true
      rust_all=true
      ;;
  esac
done < <(git diff --name-only --diff-filter=ACMRTUXB "$diff_base" "$head_sha")

if (( changed_count == 0 )); then
  frontend_all=true
  rust_all=true
fi

join_sorted_keys() {
  local map_name=$1
  local -n values=$map_name
  if (( ${#values[@]} == 0 )); then
    return 0
  fi
  printf '%s\n' "${!values[@]}" | LC_ALL=C sort | paste -sd, -
}

frontend_app_csv=$(join_sorted_keys frontend_apps)
if [[ $frontend_all == true ]]; then
  frontend_scope=all
  frontend_app_csv=
elif [[ -n $frontend_app_csv ]]; then
  frontend_scope=apps
else
  frontend_scope=none
fi

rust_package_csv=
if [[ $rust_all == false && ${#rust_apps[@]} -gt 0 ]]; then
  while IFS= read -r app; do
    package=$(sed -n '/^\[package\]/,/^\[/p' "apps/$app/src-tauri/Cargo.toml" \
      | grep -m1 -E '^[[:space:]]*name[[:space:]]*=' \
      | cut -d'"' -f2)
    if [[ -z $package ]]; then
      rust_all=true
      rust_package_csv=
      break
    fi
    if [[ -n $rust_package_csv ]]; then
      rust_package_csv+=,
    fi
    rust_package_csv+=$package
  done < <(join_sorted_keys rust_apps | tr ',' '\n')
fi

if [[ $rust_all == true ]]; then
  rust_scope=all
  rust_package_csv=
elif [[ -n $rust_package_csv ]]; then
  rust_scope=packages
else
  rust_scope=none
fi

{
  echo "frontend_scope=$frontend_scope"
  echo "frontend_apps=$frontend_app_csv"
  echo "rust_scope=$rust_scope"
  echo "rust_packages=$rust_package_csv"
} >>"$output_file"
