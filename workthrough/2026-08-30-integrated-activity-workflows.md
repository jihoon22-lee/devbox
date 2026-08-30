# Integrated activity workflows and daily snapshots

## Overview

API Playground, Log Lens, and Devbox Launcher can now send only the text that a user explicitly
selected to Developer Toolbox through a bounded one-time handoff. Developer Toolbox previews the
incoming text before applying it to Smart Workflows, and any visible tool result can be sent through
a separate preview-first Knowledge draft flow. Knowledge Base and Run Manager also publish bounded
local-civil-day activity sidecars that Life Log joins only when the requested day boundaries match
exactly.

## Context

The catalog described several useful cross-app destinations, but selected response/log text still
lacked a complete claim/preview/ack receiver. Life Log could show only latest Run Manager and
Knowledge provenance, so it could not safely attribute those values to a requested day, week, or
month. Reusing latest values would have made historical digests misleading. The integration needed
one shared size/redaction boundary, strict producer and target identities, and a date contract that
handles 23-, 24-, and 25-hour civil days without fixed-day arithmetic.

## Changes

### Selected text to Developer Toolbox

- `crates/applink` defines strict `toolbox-text/v1` payload construction, producer allowlists,
  512 KiB/256,000-character bounds, control-character rejection, line-level credential redaction,
  and strict claim parsing.
- API Playground sends only a current non-binary response-body selection. It rejects selections
  outside the body or from a stale response render.
- Log Lens sends only explicitly selected records from the current source generation. It uses the
  same deterministic bounded export as explicit export and rejects truncation instead of silently
  widening the selection.
- Devbox Launcher revalidates its confirmed selection and uses the same redacting payload builder.
- Developer Toolbox claims one cold- or hot-start handoff into an explicit modal. Cancel restores
  the claim; Apply acknowledges it and puts the text only into renderer memory. No producer or
  consumer falls back to clipboard transport.

### Developer Toolbox to Knowledge

- A visible Toolbox result has an explicit local preview action for `knowledge-draft/v2`.
  Publishing redacts credentials, checks the installed target, launches Knowledge with only an
  opaque id, and revokes the pending handoff if launch fails.
- Knowledge accepts the existing Life Log `knowledge-draft/v1` and the distinct Toolbox v2 contract.
  It shows producer-specific preview metadata and creates a unique no-overwrite Journal note only
  after Save. Claim tokens and storage paths never cross the renderer boundary.
- Expiry and regeneration messages now refer to the sending app, so the same safe wording is valid
  for both producers.

### Exact daily activity in Life Log

- `crates/integration` provides validated local civil-day boundaries for an IANA timezone and
  accepts contiguous 23-, 24-, or 25-hour days, up to 366 rows.
- Knowledge Base publishes daily note-modification counts; Run Manager publishes succeeded/failed
  run counts and the last run timestamp. The named `daily-activity.json` sidecars contain no path,
  body, command, environment, log text, or record identifier.
- Life Log export and digest documents use schema version 2. Each requested day is joined only by
  exact date, timezone, start, and end boundaries. Missing, mismatched, partial, or still-open stale
  days remain nullable and are never replaced by a latest/today value. A stale sidecar may retain an
  already-closed historical day only when that day predates the effective snapshot cutoff.
- Markdown, CSV, JSON, digest cards, daily rows, source explanations, and the existing Life Log to
  Knowledge v1 payload all preserve the same nullable/provenance contract.
- Catalog revision 15 records the new handoffs, static actions, and shared daily snapshot capability.

### Regression findings fixed while testing

- Updated Devbox Manager's embedded catalog assertion for Knowledge's new v2 receiver.
- Made Knowledge regeneration messages producer-neutral and added the missing `producerId` to a
  typed frontend fixture.
- Stabilized the Knowledge modal focus test by waiting for the documented default Cancel focus
  before checking the Tab wrap.
- Corrected browser-preview documentation to describe daily rather than latest activity snapshots,
  and kept all Windows packaged acceptance explicitly pending in #493.

## Files changed

- `crates/applink`, `crates/integration`, `crates/catalog` — shared handoff, redaction, civil-day,
  and catalog contracts
- `apps/api-playground`, `apps/log-lens`, `apps/devbox-launcher` — explicit selected-text producers
- `apps/developer-toolbox` — text receiver, Smart Workflow input, and Knowledge draft publisher
- `apps/knowledge-base`, `apps/run-manager` — v2 draft receiver and daily activity producers
- `apps/life-log` — strict daily sidecar consumer plus export/digest schema v2 UI and documents
- `apps/catalog.json`, `apps/devbox-manager` — catalog revision 15 and embedded contract checks
- affected app READMEs plus `docs/architecture.md` and `docs/roadmap.md` — user, security, and
  compatibility contracts

## Verification

The following checks passed in the dedicated worktree before the PR:

```text
cargo check --workspace -j2
  PASS
cargo clippy --workspace --all-targets -j2 -- -D warnings
  PASS
cargo fmt --all -- --check
  PASS
cargo test --workspace -j2 --quiet
  PASS (0 failed; one pre-existing ignored Run Manager test)
pnpm -r --workspace-concurrency=2 test
  PASS (all 19 buildable frontend/package workspaces)
pnpm build
  PASS (all 19 buildable frontend/package workspaces)
bash .github/scripts/check-catalog.sh
  PASS (catalog revision 15 and packaged smoke configuration)
python3 .github/scripts/check-dependencies.py check
  PASS (notices match Cargo.lock and pnpm-lock.yaml)
python3 .github/scripts/test-check-dependencies.py
python3 .github/scripts/test-build-manifest.py
python3 .github/scripts/test-validate-release-input.py
  PASS
pnpm audit --audit-level moderate
  PASS (no known vulnerabilities)
cargo deny --locked check
  PASS (advisories, bans, licenses, and sources; existing duplicate/yanked warnings remain tracked in #493)
git diff --check
  PASS
```

GitHub Actions, the Windows compiler job, and physical packaged Windows/WSL acceptance remain
separate PR and #493 release gates and are not claimed by this workthrough.
