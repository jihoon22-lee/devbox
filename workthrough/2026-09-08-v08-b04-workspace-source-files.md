# B04 Workspace Source and Files

WP #546 / R02–06, R08, R13, R16–21, R24–25 and S01. This bundle owns
Project/Worktree Registry, manifest/local trust, Overview/Source/Files/Dependencies,
legacy import and WSL-native LSP. Runtime and Terminal/session owners remain B05/B06.

## Behavior and ownership

Workbench, Repo Manager and Code Pad share their UI/tests through
`packages/workspace-features`; legacy entries consume the same components.
Workspace lazily loads Overview/Source/Files and retains visited routes. CSS is
scoped to each feature container. Three legacy entry axe smokes remain in place.

The engines expose 111 typed adapters with immutable native-selected generation
and private common snapshot/handoff/cache roots. Component-only builds omit
standalone web assets/bootstrap. Initialization does not migrate an old identifier,
absorb old profiles or start LSP. The Files manager/installer/watcher and Windows
cache/archive behavior remain available for host integration. The host must await
confirmed owned LSP termination on exit. Cross-product runtime/session providers
remain unavailable until their owners exist. Raw legacy `apply_recovery` is denied
in component mode until an identity-bound preview/apply flow is supplied.

The product Registry separates opaque Project/Worktree/Repo IDs, native root/common
Git objects, Windows/WSL distro target, aliases/rebind, legacy mappings and trust.
One writer holds directory/lock identities; strict bounded reads and revision/byte
checks prevent corruption, future schemas and stale edits from becoming empty data.
Project manifest/local-overlay validation defines precedence, explicit removals,
conflicts and source-digest trust in the [architecture](../docs/architecture/v0.8-workspace.md).

The native project probe holds actual root/Git/pointer objects, checks linked
commondir/backlinks and rejects changed identities/bytes or link traversal. Pointer
transport checks precede IO, so repository metadata cannot start WSL or contact a
new UNC host/share. The one-time preview owner checks revision, expiry, cancellation
and replay before register/rebind; registration writes only Registry metadata and
leaves trust absent. Public WSL admission still requires its native distro owner.

The new activation owner holds the product root and OS writer lock. UUID generation
directories keep Registry/Overview/Files/common stores separate; preparation is
unselected and activation cannot replace an existing pointer. At most 32 prepared
or selected generations are retained; reaching the bound refuses preparation rather
than deleting recovery data. Startup/activation rejects corrupt/future Registry,
Overview profiles/templates and Files session/recovery/LSP configuration using the
existing engine schemas. No referenced repository, `.git`, document or distro is
opened by these byte validators. Unreadable user metadata is preserved, including
unsaved recovery content. Engine dependencies reuse the existing local consumers;
no external package version or dependency family was added.

## Native Files behavior

The native Files route uses Code Pad's editor and encoding/line-ending engines.
FileOwner admits a current project file or explicit OS-dialog choice, retains
native file/ancestor objects and bounds documents/choices to 64. Save/rename/delete
require an opaque native document revision plus unchanged object/content evidence.
Persisted picker paths restore consent only; a restart creates a fresh snapshot.
Session/recovery metadata is scoped to the private worktree or project-free view.
Metadata IO checks held generation/component/view identities and preserves corrupt
files. Project changes retire pending session saves; partial restoration does not
automatically remove unavailable tabs from the saved session.

Files keeps dirty buffers across routes and ignores hidden-route global shortcuts.
Registry changes are disabled while editing, hydrating or reviewing recovery. Native
recovery binds one-time preview tokens to context, file revision and entry bytes;
cancel preserves entries, and late/unmounted previews release temporary state.
The raw legacy recovery writer is blocked. Preview assets use bounded no-link
reads inside the admitted project. Windows LSP remains disabled in the product
until execution trust is integrated; existing installer/cache engines are retained.

## Actual verification

The extracted shared UI passed 307 tests and the original native suites passed
475 tests/component-only strict Clippy. Registry/store/probe/context changes passed
focused regressions and final affected all in **566.209 seconds** under 8 GiB.
[General CI at be043f2](https://github.com/jihoon22-lee/devbox/actions/runs/34219530369)
passed. Its [Windows product run](https://github.com/jihoon22-lee/devbox/actions/runs/34219530355)
passed actual Workspace Registry/selection on both installations; the overall run
failed the older API Studio lifecycle boundary fixed in B03. B03 is now integrated
from main `82e257e`; its final general CI and complete Windows product acceptance
both passed.

Current Files owner/host changes passed **44 Workspace Rust tests**, **5 Code Pad
preview tests** and strict Clippy. The first shared UI run found duplicate
StrictMode watcher cleanup; cleanup now deduplicates restored/current document
paths. The corrected Files UI/API/recovery suite passed **39 tests**, followed by
Workspace build and **5 UI tests including axe**, in **24.640 seconds**. New tests
cover hidden-route save ownership, clean-context pending session retirement,
unexpected dirty-context preservation, one-time recovery approval and late preview
cancellation. Generated native Registry request regression and Files script syntax
passed. Notices were regenerated from lockfiles (only the Cargo.lock digest changed).

Native review found a context-change race while file IO was queued. The first
affected run was stopped to include an owned context permit spanning admission,
queues and workers; select/clear/rebind/remove cannot overlap file operations.
Worker clones retain the permit after caller cancellation. **46 Workspace Rust
tests and strict Clippy passed in 30.498 seconds**, including both permit lifetime
regressions. The next affected run stopped at Rust build after **264.961 seconds**:
the shared worktree target reused an older Workspace build script missing the
Workspace plugin permission. Cleaning only that package's Cargo artifacts restored
all-target check (**9.004 seconds PASS**). No capability grant was widened to work
around this cache issue.

Main `82e257e` brings the completed B03 implementation. Integration preserves both
products' component roles, source-graph consumers, Windows readiness and native test
sets. The generated request, metadata and CI scope regressions passed after resolving
those shared-file conflicts; notices were regenerated from the combined lockfiles.
Final combined affected all passed in **832.640 seconds** under the shared
8 GiB cap (cgroup peak 6,444,875,776 bytes), including all frontend/Rust checks.

The new Windows Files fixture exercises actual CodeMirror open/edit/save, draft
retention/context lock, outside-project denial, replaced objects/stale revisions,
CRLF save/rename/delete and recovery cancel/replay/apply. It uses only synthetic
owned files. Its Windows execution on the pushed head remains pending.

## Project definition review

Native project settings now read the strict manifest and private local overlay,
merge effective definitions and retain bounded native source/object snapshots.
The main window requests a one-time preview before recording or revoking trust.
Source/overlay changes invalidate approval; missing sources stay readable but cannot
be approved. Trust updates its own Registry CAS revision/digest while preserving
worktree binding revision, so overlays and editor sessions remain valid. This grants
no Git/task/LSP dispatch; their additional execution evidence is still required.
Shared private metadata IO moved into an app-local helper and uses an atomic writer
that requires the already-pinned parent rather than recreating a missing one.

The db6f04a general CI failed two tests: a saved-query deletion test observed only
the backend call before asserting the completed UI update; Windows TEMP used an
8.3 alias while the file-owner fixture expected Registry-canonical spelling. The
UI test now waits for removal, and the filesystem fixture uses canonical wire paths.
The Windows product job failed that same Rust fixture and skipped basic Files UI
acceptance, so no Files Windows PASS is claimed for db6f04a.

Corrected definition/file owner focused validation passed **52 Rust tests and
strict Clippy**, Workspace build and **8 UI tests**, generated Windows request and
metadata checks in **27.224 seconds**. The saved-query regression's **25 tests**
also passed. The new native definition fixture checks cancel, changed-source denial,
approval/revocation, stable editor context and absence of command execution. Its
Windows execution remains pending. The canonical native-root fix also applies
to the Windows UI fixture, which verifies the returned root against its owned
directory before writing fixture files. Final affected all passed in **430.582
seconds** under the shared 8 GiB cap (cgroup peak 5,652,332,544 bytes).

## Reviewed definition authoring

Project settings now edit/import JSON with native before/after and effective diff,
then save only the one-time reviewed token. A native load revision detects stale
editor bases. Shared export excludes the local overlay. Local editing preserves
Secret/API owner references; arbitrary JSON cannot grant their authority.

One target is written per review. Save durably revokes trust before file IO, so a
failed write can leave the prior definitions untrusted. Existing files reuse Code
Pad's bounded object/mtime/size/hash writer and preserve CRLF. New files are synced
and published without overwrite; unsupported hard-link volumes fail, and a failed
first save may leave an empty `.devbox` directory. Source and parent changes reject
pending saves; the UI preserves drafts on conflict. No automatic commit/execution
or cross-file transaction is claimed.

Focused validation passed **58 Rust tests + strict Clippy**, Workspace build and
**11 UI tests** in **37.531 seconds** under the shared cap. Regressions cover concurrent
creation, changed bytes/replaced objects, parent links, CRLF, expired validation,
local owner references, explicit review/save and late-token cleanup. The Windows
fixture now additionally exercises native manifest creation, stale/cancel/replay
rejection, UI save and local precedence; this authoring version has not run on
Windows yet. Final affected all passed in **411.265 seconds** under the shared
8 GiB cap (cgroup peak **5,826,940,928 bytes**).

The [f21e13f general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34243283346)
passed all jobs, including Windows Rust. Its [Windows product run](https://github.com/jihoon22-lee/devbox/actions/runs/34243283401)
passed native Rust, API Studio/Knowledge workflows and installer coexistence, but
the packaged shell fixture failed at the first definition-review selector: a quoted
Korean `aria-label` produced invalid JavaScript inside `Runtime.evaluate`. It stopped
before the Files scenario, so this does not establish native Files acceptance.

The selector now uses unambiguous quote boundaries. A regression parses actual
fixture call sites with the pinned Workspace TypeScript parser and compiles their
decoded browser expressions. It reproduced the old failure and passes after the
fix, together with the existing generated-request test. `node --check` alone did
not inspect those nested strings. Corrected final affected all passed in
**323.806 seconds** under the shared 8 GiB cap (cgroup peak **1,620,996,096 bytes**).

## Dependencies native integration

The shared Dependency Lens now mounts lazily against the exact native Registry
context. Plain folders can be analyzed without Git; standalone Repo Manager keeps
its Git admission. Both consumers use the existing bounded parsers, graph/stale
lock diagnostics, summary builder and reviewed OSV/deps.dev client. Renderer paths
cannot select another root. Context/input changes, cancellation, expiry and token
reuse reject remote execution. Selection stays locked while busy or reviewing.

Native generation metadata and root leases are rechecked around blocking IO and
network. The product allows at most 30 seconds per request, drops pending network
futures at expiry, and retains context/single-flight guards until blocking workers
finish. Corrupt/future caches are preserved, missing generations are not recreated,
and offline analysis does not depend on cache health. This remains bounded
cooperative filesystem cancellation; an active OS filesystem call can outlive the
caller. Summary/cache writes are derived state, not a multi-file transaction.

Focused checks passed 58 Workspace + 129 Repo Manager Rust tests, strict Clippy,
317 shared UI tests and 11 Workspace UI tests in 94.998 seconds under the shared
8 GiB cap. Subsequent focused regressions also cover guard retention after caller
drop. Three Node request/renderer checks pass. The new Windows fixture checks
plain-folder analysis, path rejection, reviewed/cancelled/stale-lock requests and
UI analyze/review/cancel without contacting public APIs; it is not yet Windows
execution evidence for this change.

Final store checks passed 24 integration tests, 11 focused enrichment tests and
strict Clippy. The dependency UI's 15 focused tests and Workspace build also passed.
Final `pnpm verify:affected` selected all and passed in **604.15 seconds**, under
the shared 8 GiB cap (cgroup peak **6,445,027,328 bytes**). The authoritative
dependency-policy audit remains the final-commit CI gate.

## Remaining acceptance

Source routing, complete Git/LSP execution-trust evidence, importer mappings,
Windows LSP integration and WSL-native transport remain incomplete. Files currently
serializes native IO under one state mutex; remote independent cancellation needs
work before WSL acceptance. Native file-dialog selection is implemented but has no
actual Windows dialog fixture yet. Strict metadata parsing is an activation
prerequisite, not a completed legacy importer. No R/S or completion issue is closed
by this draft.
