# WSL Desktop usability reinforcement review

## Overview

The user asked for a reinforcement review of WSL Desktop's day-to-day usability.
This pass is read-only analysis: it adds one review report and a roadmap pointer
to it, and changes no application source, dependency, version, catalog entry or
release artifact.

## Context

- Target is `apps/wsl-desktop` at `e7862de`, the v0.6.0 stable source line.
- The roadmap has no open v0.7+ milestone. Candidates are promoted to issues and
  a milestone only after the user confirms priority, so the report is written as
  candidates, not commitments.
- Issue #518 still carries one physical observation: installed WSL Desktop
  zellij/terminal reconnect. One finding (P1-7) is the smallest app-side change
  that would make that observation readable from the UI instead of inferred.

## Changes made

### New review report

File: `docs/superpowers/reports/2026-09-02-wsl-desktop-usability-review.md`

Twenty-two findings across three priority bands, each with a file/line citation, an
impact statement and a bounded remedy, followed by four suggested PR-sized
groupings. The report opens with the safety boundaries it does **not** propose
relaxing — exact argv, opt-in read-only multiplexer detection, broadcast target
rules and backend validation, Log Lens one-time handoff, fail-closed profile
store, no network or runtime download.

Headline findings:

- **P1-1** A routine dashboard refresh switches broadcast off and disables the
  Docker actions, at least every 30 s and on every terminal/Docker lifecycle
  event, and does not restore either when the refresh succeeds.
- **P1-2** 15 native `confirm`/`prompt` sites, the highest count in the monorepo.
- **P1-3** First run, and every run after the last pane closes, opens with no
  terminal.
- **P1-4/5** The tab bar never scrolls the active tab into view; broadcast target
  panes carry no marker in the grid.
- **P1-6/7** Session-keeping mode and side-panel state are not persisted, and a
  pane never shows the mode it actually got or that it re-attached.
- **P2** Drag pane resize and the WebGL renderer are both named in the terminal
  design spec §4.3 and were excluded from #262's acceptance; they remain absent,
  along with pane zoom, search options, buffer commands, a fuller command palette
  and any in-app shortcut reference.

### Roadmap pointer

File: `docs/roadmap.md`

One line under the current-state list points at the report and repeats that its
items are candidates without a milestone. No checkbox, milestone or commitment
was added.

## Decision evidence

### P1-1 reproduced, not inferred

A throwaway Vitest probe rendered the real `App`, armed broadcast on two panes,
then held `getDashboardSnapshot` open for one refresh:

```text
pnpm --filter wsl-desktop exec vitest run src/App.probe.test.tsx
Test Files  1 passed (1)   Tests  1 passed (1)
```

The toggle cleared while the refresh was in flight and stayed cleared after it
resolved. The probe file was removed after the run; the report quotes the
assertion block so the result can be reproduced.

### The gate is not the safety boundary

`SessionState::terminal_counts_by_distro` is documented and implemented to expose
per-distro counts only, "without exposing session ids, pane keys, cwd, title or
command metadata" (`apps/wsl-desktop/src-tauri/src/commands/terminal.rs:29-52`).
Target identity comes from renderer `panes`/`tabs` state driven by
`terminal-closed`, and `broadcast` independently rejects any id the backend does
not hold (`:515-524`). So relaxing the `refreshing` gate to "last-good snapshot
inside TTL" does not weaken target-identity safety; the report keeps fail-closed
on `error`, `stale` and target-set change.

### Deferrals confirmed as deferrals

The terminal design spec's §4.3 usability table lists drag pane resize and
`@xterm/addon-webgl`, and its 2026-08-26 #262 implementation note states both
were excluded from that issue's acceptance. `package.json` and `pnpm-lock.yaml`
contain no webgl or canvas addon, and `rg` finds no splitter/zoom code in
`apps/wsl-desktop/src`. These are open deferrals, not regressions.

## Verification results

```text
pnpm install --frozen-lockfile                  PASS
pnpm --filter wsl-desktop test                  PASS (18 files, 138 tests)
git status --porcelain (probe file removed)     PASS (docs only)
git diff --check                                PASS
```

No Rust, frontend or configuration source changed, so `cargo test`/`cargo check`/
`pnpm build` gates carry no new obligation for this pass. The frontend suite was
run anyway to establish the review baseline.

## Next steps

- Ask the user which of the four suggested groupings to promote to issues. Only
  then open a milestone; the roadmap's no-open-v0.7 posture is deliberate.
- Grouping 1 (snapshot gating + confirmation load) is frontend-only and carries
  the highest daily impact per unit of risk; it is the recommended first PR.
- P2-2 (WebGL renderer) must be measured on Windows before and after. Do not
  quote a throughput number from a Linux development box.
- P1-7 pairs naturally with the remaining #518 zellij/terminal reconnect
  observation and should be done before that item is re-attempted by hand.
