# Workbench Services and Ports Input Workthrough

- Date: 2026-08-26
- Issue: #280 `feat(workbench): services·ports 입력`
- Branch: `feat/workbench/services-ports-input`
- Base: `6189c1da37dc7a446d25b65afe0475dbdadb6087`
- Target: Workbench 0.2.0 / v0.5.0 P1-09-17
- Status: implementation, direct review and local PR-wide gates complete; GitHub Actions pending

## Outcome

Workbench profile editor now owns a draft instead of mutating the persisted `ProjectProfile` DTO. A user can type
an incomplete port list, add or remove stable service rows, inspect per-field failures and either fix or cancel the
draft without changing the saved profile. Only a normalized DTO which passes frontend validation crosses IPC, and
the Rust boundary repeats the authoritative validation before any file mutation.

The profile store was hardened as part of the same save boundary. A missing file is the only state interpreted as
an empty store. Corrupt JSON, unsupported versions, unknown fields, invalid paths, duplicate IDs or project
identities, unsafe links and size-limit failures stop the operation without replacing the original. CRUD writers
share a process-local lock and save a fully validated candidate only when the raw bytes read before editing still
match, followed by the existing atomic filesystem replacement.

Run Manager remains the owner of service definitions and lifecycle. Workbench stores only selected service IDs and
reads the existing v1 integration snapshot for health. A genuinely missing snapshot means the configured services
are not known to be running; a malformed or unsafe snapshot is a distinct unavailable state and can no longer be
silently collapsed into an empty running set.

## Scope

### Included

- comma-separated expected-port input with raw editing-buffer preservation
- Run Manager service ID row add, edit, delete and order preservation
- stable React row keys which never enter the persisted DTO
- frontend parse, normalization, bounds and field-level errors
- Rust IPC/storage validation for the complete `ProjectProfile` and store
- missing-versus-corrupt profile file handling
- collision-safe update and bounded canonical-identity validation
- process-local writer serialization, raw-byte CAS and atomic replacement
- strict, bounded Run Manager `activeServices` snapshot decoding
- stale list and health request rejection
- Enter submit, IME-safe Escape cancel, autofocus, labels and ARIA error relations
- generic UI/native errors which do not echo raw path, credential or subprocess details
- README, Workbench design, roadmap, native-first plan and this workthrough

### Excluded

- creating, editing, starting or stopping Run Manager services
- writing the Run Manager database or integration snapshot
- WSL Desktop runtime snapshot production and automatic proposal acceptance (#410/#281)
- project environment preflight, dependency health and template wizard
- OS-wide advisory locking for arbitrary non-cooperating external writers
- changing project files, environment files, shell configuration or network state
- changing Start/Stop Workspace ownership semantics

## Existing Boundary and Problem

`ProjectProfile` already contained `expectedPorts` and `runManagerServiceIds`, but the UI directly edited the
persistence DTO. Port input was reconstructed with `join(", ")` on every keystroke, so partial values jumped or
disappeared. No service-row UI existed, and the frontend sent `runManagerServiceIds: []` for new profiles.

The native store also treated read/parse/version failure as an empty store. A later create or update could therefore
replace a corrupt but recoverable user file with one new profile. Update deleted the old profile before checking the
replacement's canonical identity, which made a collision path capable of losing the edited record in the candidate.
Independent IPC mutations were not serialized across their read-modify-write sequences.

The existing health reader extracted whatever string IDs happened to be present in `activeServices`, ignored bad
entries and treated every error or missing field as an empty set. That made a corrupt snapshot look exactly like a
healthy producer with no running services.

## Data and Validation Contract

### Draft-to-DTO pipeline

```text
ProfileDraft
  name/windowsPath/wslDistro/wslPath/gitRoot raw strings
  expectedPortsText raw string
  serviceRows [{ React-only stable key, raw value }]
        ↓ parse and normalize on every render for feedback
        ↓ save only when there are no errors
ProjectProfile DTO
        ↓ Rust whole-profile and whole-store validation
validated candidate store
```

The draft preserves invalid intermediate text. Trimming occurs only while constructing the DTO, so pressing Cancel
does not mutate any object in the loaded profile array. `draftFromProfile` generates stable row keys once when an
editor opens; subsequent value changes map by key and deletion removes exactly one row.

### Bounds

| Field | Rule |
|---|---|
| profile name | trimmed, non-empty, no control characters, at most 120 Unicode scalars |
| profile ID | normalized, non-empty, no control characters, at most 128 scalars |
| Windows/WSL/Git path | normalized and accepted by the 4 KiB safe-project-path boundary |
| WSL distro | paired with WSL path, at most 128 scalars, common argv-safe character rule |
| expected-port source | at most 8 KiB of editor text |
| expected ports | unique integers in 1..=65535, at most 128 |
| service IDs | trimmed, non-empty, unique, no control characters, 128 scalars each, at most 128 |
| profile store | version 1, at most 512 profiles and 4 MiB serialized JSON |

`serde(deny_unknown_fields)` is applied to the store, profile and WSL record. The persisted service IDs remain
references only; service configuration, commands, environment and secrets are not copied into Workbench.

## Persistence Decisions

### Fail closed on existing data

The command layer first uses `symlink_metadata` and bounded byte reads. Only `NotFound` returns an empty document.
A directory, symbolic link/reparse point, invalid UTF-8, invalid JSON, unsupported schema or invalid complete store
returns a fixed error. The original bytes are retained in `ProfileStoreDocument` and are never included in errors.

### Validate a candidate before replacing

Create and update clone the validated store. Update locates the existing ID and checks canonical collision against
all other rows before replacing it in the candidate. Full-store validation then runs again. No deletion is applied
to the persisted file before the candidate succeeds.

### Serialize app writers and detect ordinary external edits

All create/update/delete requests and startup Life Log absorption hold one `ProfileStoreState` mutex across:

```text
bounded load → strict parse → candidate mutation → whole-store validation
→ compare current raw bytes with originally read bytes → atomic write
```

The CAS detects another request or ordinary external edit made before the final comparison. The filesystem helper
uses a unique temporary file and atomic replacement. A non-cooperating external process can still write between the
last comparison and rename; adding a portable OS advisory lock is explicitly outside this PR and remains a Windows
W1 observation rather than an overstated guarantee.

## Run Manager Snapshot Boundary

The v1 producer payload is decoded into a typed `activeServices` array. The consumer rejects the complete payload
when the field is missing or has the wrong type, there are more than 128 entries, an ID violates the profile service
ID rule, an uptime is negative or an ID is duplicated. Unknown top-level producer metrics remain forward-compatible.

```text
read error / unsafe envelope / invalid data → service status unavailable
snapshot file absent                         → configured services are not running
valid bounded array                          → compare exact configured IDs
```

No raw snapshot value is returned in an error or rendered in the UI.

## Async and Accessibility Decisions

- list and health requests each receive a monotonically increasing generation
- only the newest list generation mutates profiles/run/selection
- health results must match both generation and selected profile ID
- a failed newest list read clears stale profiles, selection, health and run ownership from the actionable UI
- a synchronous ref blocks double submit before React can commit `busy`
- save disables all editor fields, add/delete row actions, Cancel and global create/refresh actions
- the inline editor is a form, so Enter follows native submit semantics
- Escape cancels only when no save is active and no IME composition is in progress
- the name field receives initial focus; every input has an explicit label and errors are linked with
  `aria-invalid`/`aria-describedby`
- service rows live in a fieldset and deletion has an indexed accessible label

## Privacy and Ownership Review

The UI catches backend failures and renders fixed Korean messages. It does not echo raw Tauri errors, project paths,
credentials, arbitrary service metadata or subprocess stderr. Native profile/path/open failures are also mapped to
stable messages. Existing explicit health details for configured paths and service IDs remain unchanged; this PR
does not add logging, telemetry, clipboard actions or persistence of those values.

Workbench writes only its profile file. Run Manager data is read-only, Life Log absorption continues through its
versioned integration view, and no project/environment file is accessed by the editor. Start/Stop Workspace keeps
its existing run/profile ownership gate.

## File Changes

### Frontend

- `apps/workbench/src/lib/profileEditor.ts`
  - draft model, stable service row keys, port parser, normalization and validation
- `apps/workbench/src/lib/profileEditor.test.ts`
  - raw buffer, CRUD/order, duplicates, bounds, ID and WSL distro fixtures
- `apps/workbench/src/App.tsx`
  - draft editor, service rows, form semantics, request generations and safe errors
- `apps/workbench/src/App.test.tsx`
  - editor CRUD, invalid buffer, submit/cancel/IME, busy, stale/failure and privacy fixtures
- `apps/workbench/src/App.applink.test.tsx`
  - existing app-link delivery assertion adapted to the new input-owned draft
- `apps/workbench/src/App.css`
  - field help/error, service rows, focus-visible and screen-reader label styling

### Native

- `apps/workbench/src-tauri/src/core/profile.rs`
  - strict store parsing, whole-store/profile bounds and candidate replacement
- `apps/workbench/src-tauri/src/commands/workspace.rs`
  - bounded file document, writer gate/CAS, CRUD adaptation and snapshot decoder
- `apps/workbench/src-tauri/src/commands/profile_actions.rs`
  - validated profile IDs and safe fixed command errors
- `apps/workbench/src-tauri/src/lib.rs`
  - writer state registration and startup absorption through the same document contract

### Documentation

- `apps/workbench/README.md`
- `docs/superpowers/specs/2026-08-14-workbench-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `docs/roadmap.md`
- this workthrough

## Failure Fixtures

Native fixtures cover corrupt/version/unknown-field/oversize input, duplicate IDs and identities, profile and
collection bounds, invalid ports/service IDs/paths/distro, collision-preserving update, stale raw-byte documents,
complete Run Manager payload validation and distinct missing/corrupt health states. Existing Life Log snapshot,
launch request and run ownership fixtures continue to pass through the changed store API.

Frontend fixtures cover invalid port text preservation, stable service rows and order, empty/duplicate/bounded
rows, normalized save DTOs, Enter submit, IME-safe Escape, save de-duplication, disabled state, stale list and health
results, failed refresh stale-action removal, fixed error privacy and both cold/hot app-link draft delivery paths.

## Validation

Focused validation completed with heavy frontend work isolated to an exact Linux-native mirror and two workers:

```text
cargo fmt --all -- --check
  passed after formatting
cargo test -p workbench --lib
  44 passed
pnpm --dir apps/workbench test -- --maxWorkers=2
  4 files, 34 tests passed
pnpm --dir apps/workbench build
  TypeScript and Vite production build passed
```

The first focused frontend run correctly exposed five fixture adaptations: three ambiguous edit-button queries and
two app-link assertions which looked for draft path text instead of the new labelled input value. After making the
queries exact, TypeScript also exposed a missing test-only `ProjectHealth` type import. These were test defects caused
by the new accessible editor structure, not product failures; the complete focused suite and production build then
passed. The final native review also added an explicit crossed profile-ID/canonical-identity collision fixture and
reran all 44 Workbench native tests.

PR-wide gates then completed:

```text
cargo test --workspace
  all workspace unit, integration and doc tests passed
cargo check --workspace
  passed
cargo clippy --workspace --all-targets -- -D warnings
  passed
cargo fmt --all -- --check
  passed
pnpm --workspace-concurrency=2 -r test -- --maxWorkers=2
  all 17 frontend/package projects passed
pnpm --workspace-concurrency=2 -r build
  all 17 frontend/package projects passed
bash .github/scripts/run-frontend-scope.sh typecheck all ''
  passed
catalog consistency, dependency-policy and manifest regression scripts
  passed
pnpm audit --audit-level moderate
  no known vulnerabilities
cargo deny --locked check
  advisories, bans, licenses and sources passed; allowlisted duplicate warnings only
```

Clippy's first pass identified three mechanical issues in new code (`manual_contains`, a collapsible startup save
condition and a test-only useless `format!`). They were corrected and the warning-as-error gate passed. Windows-only
compile and packaged behavior remain assigned to GitHub Actions and W1 rather than being claimed from WSL.

## Windows W1 Checkpoint

- create and edit profiles with partial port strings and multiple service rows
- confirm Cancel preserves saved values and successful Save preserves service order
- verify duplicate/empty/out-of-range/bounded input errors and keyboard focus visibility
- verify Enter submits, IME composition Escape does not cancel and ordinary Escape does
- attempt rapid double save and confirm one mutation
- corrupt the profile JSON and confirm list/actions fail without replacing the file
- create a competing external edit before save and confirm conflict without lost data
- observe the documented last-CAS-to-rename residual race separately
- verify missing Run Manager snapshot shows configured services as not running
- verify corrupt/malformed Run Manager snapshot shows status unavailable
- verify packaged startup Life Log absorption still uses the same serialized writer boundary
- inspect logs and UI for absence of raw paths, credentials, service metadata and subprocess stderr
- confirm no Run Manager lifecycle, environment preflight, template or external download action was added

## Follow-up

#410 makes WSL Desktop the bounded runtime snapshot producer. After that producer merges, #281 can consume its
container/port/terminal suggestions in Workbench with explicit user acceptance. Webhook Lab #282 and #283 are
independent P1 work and continue in parallel.
