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
