# B05 — Workspace Runtime and Logs

Refs #547, #541 and #542. B05 combines Run Manager, Port Manager and Log Lens
under Workspace's Tasks & Services, Runtime and Logs routes. B04 supplies the
Project/Worktree/Target and definition/reference interfaces. B06 owns PTY and
Development Session orchestration; B07 owns the complete global integration.

## Shared feature extraction

The existing UI, API adapters, state models and tests move into
`packages/workspace-features/src/tasks`, `runtime` and `logs`. The three standalone
apps render those same feature entries. CSS selectors are scoped to each feature
container so loading another Workspace route cannot restyle it. No command,
scheduler, process action, log source or persistence behavior changes in this
extraction. Native product routing and owner initialization are subsequent semantic
changes in the same B05 bundle.

No new runtime dependency is added. The existing package and reverse consumers
remain visible to affected verification through their workspace dependencies.
The shared feature typecheck and **508 tests in 56 files** passed, along with
all three standalone frontend builds and their unchanged bundle budgets. The
first bundle command omitted its scope argument; the corrected check passed.
The first affected run stopped in the scope regression: its expected Editor and
catalog consumers predated the three added workspace-package consumers. The
expected closures now include them, with direct coverage of all three moved
feature paths; the scope regression passed. Final `pnpm verify:affected` selected
the whole repository and passed in **1001.431 s** (including fresh native test
compilation in this worktree), sampled RSS **4,808,962,048 B**, cgroup memory peak
**6,445,027,328 B**, within the shared verification budget. These checks do not
claim native product routing or Windows UI acceptance for B05.

## Required ownership and migration work

Runtime must keep its own scheduler, definitions, operations/runs, owned processes
and execution-time secret access. External listener termination remains a separate
identity-checked Process Action boundary. Logs must read Runtime-owned descriptors
instead of resolving the legacy Run Manager app-data directory. Imported definitions
must remain inactive until reviewed activation; live processes are not adopted from
persisted PIDs. WAL-consistent imports preserve original schemas, references and
source-owned log boundaries.

B04 merged as #557 (`e09e5f4`) after final CI and Windows acceptance. B05 native
integration is in progress: generation extension preserves B04 stores; dedicated
Runtime/Processes/Logs initialization and retirement, Runtime-owned log leases,
process observation providers and closed role admission are being connected.
UI activity controls preserve views while stopping hidden polling. Importers,
complete internal navigation and final Windows/WSL acceptance remain outstanding.
The PR is not ready for detailed completion verification.

## Verification cadence correction (2026-09-12)

Repeated per-edit Clippy/test runs and the extraction-only full affected run
spent verification time before B05 implementation was complete. At the user's
request, AGENTS, CONVENTIONS, verification operations and repository skills now
require only minimal syntax/type/scope checks before commits. Detailed regression,
affected and Windows/WSL acceptance runs are grouped after the complete PR scope
is implemented. Existing passing evidence is repeated only after a related change,
failure or new risk; checks already included in affected are not pre-run separately.
Final CI and release gates remain required. These policy edits were reviewed as
text and diff changes; no compiler or test run is needed for them.

## Native host and shared views in progress

The Workspace host now selects dedicated runtime/processes/logs stores without
resetting a B04 generation. Existing engines are embedded without standalone
bootstrap; execution, external process actions, observation and log read commands
have separate closed roles. Runtime starts with the selected product, independently
of route visits. Normal exit retains scheduler/log-reader ownership through cleanup;
tray hide preserves background work. Product Runtime publication is suppressed in
legacy snapshot namespaces; native projections supply process correlations.

Runtime log leases bind the database/run/directory identities and are revalidated
before and after reads. A separate `runtimeRun` source never falls back to the legacy
Run Manager root. Product private stores are excluded from ordinary local log sources.
The Run/container source wire-field correction accepts previous persisted spellings
and preserves their existing opaque source hashes. Tasks/Runtime/Logs are lazy routes;
hidden views stop polling/read work and preserve drafts, filters and cursors. Native
navigation checks the current project and initiating view before applying a late result.

Per-commit syntax/type checking uses Workspace `tsc --noEmit` and `cargo check --lib`.
The first typecheck found the new native-only source missing from the manual source
form switch; it was corrected. The follow-up typecheck and Rust check passed. No test,
Clippy, build or affected run was added after the verification-cadence correction.
New navigation and owner fixtures remain queued for PR completion validation.

Still required within B05: diagnostics/internal artifact delivery, inactive
WAL-consistent import with history/log/secret mappings, and final Windows/WSL
acceptance of the integrated execution and recovery paths. Temporary unavailable responses in the development host are not acceptance
completion. This work has not been merged or released.

## Durable controls and process ownership

Product-only SQLite receipt tables reserve each closed start/stop/restart request
before execution. The frontend persists only its opaque request ID and bounded
ID/boolean arguments before IPC, retains them after transport loss, and clears them
after confirmed completion. Replays return the existing result; native restart marks
unfinished submissions interrupted. The recovery view requires explicit state review
and unsettled owned runs block review. Standalone source schema 4 is unchanged.

DAG writers now retain a scheduler lease through terminal recording. Service stop
retains its generation and run reference after failed termination, stops the exact
linked run, and does not schedule backoff after intentional stop. The process broker
queries retained execution handles: Windows Job Object membership uses exact creation
FILETIME; WSL membership rechecks the owner marker, process start tick, PGID and SID.
Owned descendants route to Tasks; only unowned processes reach the external kill
boundary, which rechecks its deadline immediately before mutation.

Queued regressions cover concurrent reservations, replay after reopen, interrupted
requests, future-schema byte preservation, storage failure before IPC, unresolved
service cleanup and descendant ownership. They have not been executed at this stage.
Workspace TypeScript checking passed. Rust compilation found and corrected two fixture
argument omissions and a SQLite row-iterator lifetime; the affected Workspace/Run
Manager `cargo check --all-targets` then passed. No tests, Clippy, full build or affected
run was added. Windows-only membership code still awaits the PR completion gate.
