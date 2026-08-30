# Workthrough: Observability correlation and deterministic webhook workflows

**Date:** 2026-08-30

**Branch:** `feat/port-manager/observability-correlation`

**Issue:** [#489](https://github.com/jihoon22-lee/devbox/issues/489)

## Outcome

Port Manager can now correlate a live TCP listener with Run Manager service
declarations and Workbench expected ports, explain the confidence of each
match, navigate to the owning task/profile, and open retained Run output in
Log Lens through an identity-only one-time handoff. Its successful refreshes
also produce a bounded, session-only endpoint/owner timeline.

Webhook Lab now evaluates overlapping rules with a deterministic public
precedence contract, previews conflicts before mutation, creates data-only
rule drafts from bounded local OpenAPI documents, and exports one disabled Run
Manager service definition backed by a strict app-local service profile.

This is W08 PR 1 of issue #489. Webhook-to-Log-Lens sanitized capture,
Log Lens saved views/localization, and reconnect behavior remain in W08 PR 2.

## Implementation

### Shared port-binding and log-source contracts

`crates/integration` owns `snapshot:port-bindings/v1`, a strict named view with
at most 2,048 entries. Run Manager publishes only service/run identities, safe
labels, loopback address/port, target metadata, log availability, and an exact
Windows process identity when available. Workbench publishes only profile
identity, safe label, and expected port. Neither projection contains command,
cwd, environment, project path, credential, or log bytes.

Run Manager writes immediately on startup and every 60 seconds through its
existing snapshot loop. Workbench publishes after successful profile CRUD and
also refreshes every 60 seconds while the process is alive, preventing its
otherwise unchanged view from becoming stale while Workbench remains open.

`crates/applink` now owns the shared strict Run log-source reference:

```text
{ kind, sourceId, runId, stream }
```

Run Manager, Port Manager, and Log Lens use the same validator. Port Manager
can route the identity, but Run Manager remains the log owner; raw paths,
commands, environment, and output never cross the handoff boundary.

Catalog revision 16 declares Run Manager and Workbench as the only shared
port-binding producers and Port Manager as a `log-source/v1` producer for its
explicit Log Lens action.

### Port Manager correlation and timeline

The native correlation command reads Run Manager and Workbench independently.
Missing, invalid, future-dated, and older-than-180-second views are diagnosed
per producer without failing the listener snapshot or the other producer.

Correlation confidence is deliberately narrow:

- `verified`: exact Windows PID plus process creation identity matches.
- `declared`: Run target/source/distro and the configured loopback endpoint
  match, but exact native ownership cannot be proved.
- `expected`: a Workbench profile declares the observed TCP port; ownership is
  not implied.

Concrete `127.0.0.1` and `::1` declarations match only that exact listener
bind. Only `localhost` intentionally spans both loopback stacks. Correlations
are deterministically sorted, limited to 64 per row and 4,096 per snapshot,
and surface a truncation diagnostic.

Opaque action keys bind the current listener identity, endpoint, source,
target, and Run identity. A periodic producer heartbeat alone does not rotate
the key. Every owner/log click still re-collects listeners and re-reads the
current producer views; a changed listener or target is rejected as stale.
Log launch failure removes only the exact unpublished handoff.

The refresh timeline ignores the first successful baseline and failed polls.
It keeps the latest 256 opened/closed/changed/owner-changed events in memory
only. Each event stores only observation time, address, process name, and owner
labels; it does not retain commands, executable paths, process identities,
target ids, or action keys.

### Deterministic Webhook Lab rules

`ResponseRule.priority` defaults to zero for old payloads and is bounded to
`-1000..=1000`. Runtime selection, list order, and conflict preview share one
comparator:

```text
higher priority
→ exact path
→ method-specific
→ longer trailing-wildcard prefix
→ bytewise ascending rule id
```

Overlap detection covers method-any/method-specific, exact paths, exact versus
prefix, and nested prefixes. Preview assigns a stable candidate UUID, validates
the complete projected collection, reports winner/loser/reason, and does not
mutate storage. Save recomputes conflicts while holding the rules lock and
requires explicit confirmation. Editing resets only that rule's process-local
response-sequence cursor.

### Shared bounded OpenAPI parser and Webhook adapter

The parser previously embedded in API Playground is now the private workspace
package `@devbox/openapi`. It accepts JSON/YAML only within 4 MiB, depth 40,
50,000 nodes, 16,384-character strings, and 50 aliases; it rejects duplicate
or dangerous keys, cycles/custom graphs, unsafe integers, and normalizes into
null-prototype records. API Playground retains OpenAPI semantic transformation
and native URL-fetch responsibilities.

Webhook Lab reads only a user-selected local file in the renderer, then
projects at most 250 paths and 1,000 operations. The adapter carries only
method, printable ASCII path, and the lowest explicit 2xx response status
(default 200). It never projects server URL, auth/security, request body,
example, or credential. Parameterized, referenced, Unicode/non-ASCII, wildcard,
query/fragment, and otherwise unsafe paths remain visible but non-applyable.
Applying a selected operation requires confirmation and fills an unsaved rule
draft only.

### Disabled Run Manager service export

The renderer supplies no bind, port, rules, executable path, or command.
The backend revalidates an actively running `127.0.0.1`/`::1` listener while
holding the lifecycle lock, snapshots the current validated rule collection,
and writes a strict app-local profile under `service-profiles/<uuid>.json`.
Profiles are limited to 64 files and 8 MiB each, reject links/unknown fields,
use atomic writes, and reject credential-shaped id/method/path/header/body or
sequence content.

The downloaded Run Manager schema-v1 document contains one Windows service,
no jobs, environment, cwd, rules, runtime identity, or response data. It is
always `enabled=false`, `autoStart=false`, and `restartPolicy=never`, with a
loopback health check and a fixed `--service-profile <uuid>` command. Service
mode accepts only that exact argument pair, loads the app-owned profile,
starts a hidden loopback listener without the interactive single-instance
plugin, and cannot overwrite interactive window geometry. Interactive mode
keeps single-instance as its first plugin.

## Review findings incorporated

The implementation review and full gates exposed and fixed these additional
issues:

- Run Manager's concurrency test sampled a start counter before the active
  counter; it now waits for the property being asserted instead of flaking
  under a busy parallel runner.
- Workbench needed a heartbeat, not only CRUD-triggered publication, to remain
  fresh while open.
- Concrete loopback declarations initially correlated by port too broadly;
  address matching is now exact.
- Correlation output and session timeline retention needed explicit global,
  per-row, and metadata-only bounds.
- Binding action keys to `generatedAt` made unchanged actions randomly stale
  after producer heartbeat; semantic identity now remains stable.
- Webhook service-profile startup briefly violated the repository rule that
  interactive single-instance must be the first plugin; plugin order is
  restored.
- Unicode OpenAPI paths passed the adapter but were rejected by the native
  ASCII matcher; they are now preview-only and non-applyable.
- Service-profile secret checks now cover rule id/method/path, response headers
  and bodies, sequence steps, query assignments, and JSON field assignments.
- The full Rust suite found a stale Devbox Manager catalog expectation after
  revision 16; the consumer test now asserts Workbench's new capability.
- The dependency policy correctly detected stale lockfile hashes after adding
  the shared package; `THIRD_PARTY_NOTICES.md` was regenerated.

## Verification

The following checks passed in the dedicated worktree:

```text
pnpm install --frozen-lockfile
pnpm -r --workspace-concurrency=2 test
pnpm -r --workspace-concurrency=2 build
bash .github/scripts/run-frontend-scope.sh typecheck all

cargo check --workspace -j2
cargo test --workspace -j2 --quiet
  PASS: all workspace tests; one existing ignored Run Manager test
cargo clippy --workspace --all-targets --all-features -j2 -- -D warnings
cargo fmt --all -- --check

bash .github/scripts/check-catalog.sh
python3 .github/scripts/check-dependencies.py check
python3 .github/scripts/test-check-dependencies.py
python3 .github/scripts/test-build-manifest.py
python3 .github/scripts/test-validate-release-input.py
pnpm audit --audit-level moderate
cargo deny --locked check
git diff --check
```

Focused evidence after final review fixes also passed:

```text
Port Manager: 33 Rust tests, 45 frontend tests
Webhook Lab: 86 Rust tests, 79 frontend tests
Workbench: 130 Rust tests
Run Manager: 266 passed, 1 ignored
Log Lens: 53 Rust tests
applink: 83 Rust tests
integration: 23 Rust tests
catalog: 11 Rust tests
API Playground OpenAPI: 14 frontend tests
@devbox/openapi: 2 frontend tests
```

`cargo deny` reports the repository's already-allowed duplicate/yanked
dependency warnings while completing advisories, bans, licenses, and sources
successfully. No new audit failure was introduced.

## Remaining physical acceptance

WSL cannot prove packaged Windows process identity, WebView download behavior,
hidden service startup/stop, native app launch, or installed app discovery.
The following remain explicit v0.6.0 Windows acceptance items:

- Run/Workbench snapshot publication under `%LOCALAPPDATA%` and Port Manager
  verified/declared/expected rendering against real listeners.
- Owner navigation and Port Manager → Log Lens stdout/stderr handoff.
- OpenAPI file picker/download behavior in WebView2.
- Importing the downloaded disabled service in Run Manager, enabling it, and
  starting/stopping the hidden Webhook Lab service process.
- IPv4/IPv6 loopback binding and rejection of LAN export.

Linux source checks, unit tests, and frontend builds must not be interpreted as
completion of those installed Windows checks.
