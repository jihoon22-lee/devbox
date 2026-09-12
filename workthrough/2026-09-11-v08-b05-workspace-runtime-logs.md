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

## Inactive Runtime import and diagnostic navigation

Tasks diagnostics now use the Runtime-owned run/log lease and verified task source.
The host checks the selected native project and sends only a relative file/range;
Files retains its normal grants and dirty-buffer handling. Late context/route events
and malformed ranges have fixtures queued for the PR completion gate.

The native import worker preserves a WAL-consistent schema-4 snapshot with the WP01
harness, plus bounded unchanged log files and original rotation manifests. Source
SQL is read-only; original commands, ciphertext and metadata stay in the private
snapshot. Exact IDs are retained in the separate product namespace and recorded
in owner-local mappings. Existing ID/source-identity collisions reject the entire
transaction. A completed same-snapshot import replays without overwriting subsequent
edits; changed snapshots require conflict review. Jobs/services are disabled, task
trust is cleared, active historical records become interrupted terminal history,
and no process identity is adopted. Old enable/auto-start intent is retained.
Ciphertext is not unsealed or installed; database launch/enable gates require an
explicit encrypted environment replacement or clear before those jobs can execute.

Retention and import publication share a database-owned mutex through final file
publication and SQL commit. Cancellation/failure preserves legacy data and private
snapshots; native workers remain owned after renderer loss and are joined at exit.
The Tasks panel offers preserve, review, inactive apply, cancellation and explicit
revalidation of retained snapshots. Missing/orphan logs are reported, with no silent
archive claim. Port preferences/Logs saved-view conversion still remains in B05.

Minimal Workspace TypeScript checking passed. Rust all-target compilation found two
new type errors (digest formatting and a fixture Debug bound), corrected both, then
passed in 15.516 seconds. This compiled fixtures without running them. No tests,
Clippy, application build or affected verification ran for this commit. Actual
Windows/WSL acceptance and the complete PR verification remain pending.

## Runtime preferences and saved-view import

The fixed legacy JSON inventory now includes Port Manager preferences and Log Lens
saved views/window state. Runtime and Logs provide preserve/revalidate/review/apply
panels. A short-lived native review pins the source bytes, destination revision and
owner. Existing settings require explicit replacement; the prior settings and import
receipt are committed with the new settings in one owner-local atomic record.
Normal saves keep receipts, and repeated imports preserve later edits. Corrupt/future
product records never fall back to defaults and overwrite the saved file.

Legacy Run sources become Runtime references only through the Runtime importer ID
map. Missing runs/logs retain disconnected source definitions with an unavailable
revision; explicit reconnect can resolve them after Runtime import. Filter source
IDs are rebased along with descriptors, while unmatched filters stay unmatched.
Saved-view loading starts no I/O. Source generation is checked across explicit
reconnect before changing the mounted view; standalone adapters remain in place.

Queued fixtures cover receipt preservation, revision conflicts, malformed records,
nonpersistable sources, capacity and filter rebasing. Workspace TypeScript checking
passed. Rust all-target compilation exposed an intended Port component export
missing from the public boundary; after that export was fixed, the affected four
crates compiled in 12.805 seconds. No detailed test/build/Clippy/affected run was
performed. Internal cross-product Artifact delivery remains WP07 integration;
container action ownership and Development Session consumers belong to WP06.

## Native execution acceptance packet

Runtime listener projections now cover actual owned descendants and process-mode
jobs without a configured health port. The execution owner supplies the exact
active task/run reference only after retained-handle membership and current-owner
revalidation. Observation and action hashes bind source identity, endpoint, run
and current context; Windows remains verified and WSL remains declared. Enrichment
is bounded and preserves independent source observations when ownership is unsettled.

Imported jobs expose secret reconnection status in both editors, with explicit
replace/clear followed by normal save. Preparing actual service acceptance exposed
an existing execution-layer lookup restricted to kind=job: services could not fetch
their own environment ciphertext. The owner lookup now supports both kinds and a
regression checks ciphertext stays out of read DTOs. Import acquisition additionally
retains the original root/database objects across log preservation. Complete files
publish without replacement from private scratch space, avoiding partial final logs
that would block a cancelled import's retry.

New runner-owned Windows fixtures cover real child listeners, source confidence,
creation/start-tick mismatch, native receipt replay after renderer reload and native
crash, Job Object/group cleanup, terminal log leases, WSL service secret redaction
and stopping retry backoff. A Python owner holds committed WAL pages while the native
importer preserves and converts jobs/services/history/logs; the fixture compares source
DB/WAL bytes and checks inactive state, secret review, repeat-import edits and saved-view
receipt behavior. Only the run-owned WSL distro receives Python for these fixtures.
These are authored scenarios, not executed acceptance evidence yet.

Workspace TypeScript checking passed. Native all-target compilation found one new
fixture argument using Option instead of EnvironmentCiphertextUpdate; it was fixed.
The affected native compilation then passed in 12.160 seconds, and the first three
Windows scripts passed syntax parsing. Two standalone-only imports were subsequently
cfg-scoped by source review. The added crash script still needs its syntax check.
No tests/Clippy/application build/affected run has been added at this implementation
stage. Parity/data inventories point to the new implementations and retain pending
verification status. Container/session/Problem consumers and cross-product Artifact
transport remain explicitly assigned to WP06/WP07, without a false completion claim.
