#!/usr/bin/env bash
set -euo pipefail
code=true
if [[ $EVENT != workflow_dispatch && ! ( $EVENT == push && $BASE == 0000000000000000000000000000000000000000 ) ]]; then
  if [[ -z $BASE ]] || ! git cat-file -e "$BASE^{commit}"; then
    echo "Scope gate: missing or unreadable baseline commit: $BASE" >&2
    exit 1
  fi
  # Count both sides of renames; docs/config.md and THIRD_PARTY_NOTICES.md are code (tests read them); errors fail closed.
  git diff --no-renames --name-only "$BASE" HEAD > "$RUNNER_TEMP/devbox-changed-files"
  status=0
  grep -vP '^(docs/(?!config\.md$)|(?!THIRD_PARTY_NOTICES\.md$)[^/]+\.md$)' "$RUNNER_TEMP/devbox-changed-files" > /dev/null || status=$?
  case "$status" in
    0) ;; 1) code=false ;;
    *) echo "Scope gate: file classification failed (grep exit $status)" >&2; exit "$status" ;;
  esac
fi
echo "code=$code" >> "$GITHUB_OUTPUT"
