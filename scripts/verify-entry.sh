#!/usr/bin/env bash
# Full checks always use the common verification lock; dogfood may run inside an acceptance owner.
set -euo pipefail
cd "$(dirname "$0")/.."
entry=$1; shift
options=()
if [[ $entry != dogfood ]]; then options=(--wait); source ~/.cargo/env; fi
while (($#)); do
  case "$1" in
    --wait) options+=("$1"); shift ;;
    --lock) [[ $entry == dogfood && $# -ge 2 ]] || exit 2; options+=("$1" "$2"); shift 2 ;;
    --no-lock) exit 2 ;;
    --) shift; break ;;
    *) break ;;
  esac
done
case "$entry" in
  check) command=(bash scripts/check-inner.sh); ((${#@} == 0)) || exit 2 ;;
  e2e) command=(pnpm --filter app exec playwright test "$@") ;;
  dogfood) command=(python3 scripts/dogfood.py "$@") ;;
esac
if ((${#options[@]})); then exec python3 scripts/verify-resources.py "${options[@]}" -- "${command[@]}"; fi
exec "${command[@]}"
