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

LSP product initialization preserves applying and committed rename journals without
reading their external targets or deleting backups; standalone Code Pad retains
its existing automatic recovery. Manager-owned execution policy now runs before
root IO, before managed Node version probes and again before server spawn, including
automatic restart paths. The hosted metadata manager has no execution authority.
Managed command preparation is process-free; the explicit probe uses the resolved
workspace cwd. Workspace exit awaits the manager's existing confirmed shutdown
boundary and retries failed shutdown instead of abandoning owned children.
These are prerequisites for the pending context-specific native LSP owner.

Focused LSP checks passed **125 tests and strict Clippy** in **29.586 seconds**
(cgroup peak **2,153,611,264 bytes**, 8 GiB cap). Fixtures verify applying/committed
journal preservation plus standalone recovery, denied root/process execution and
the separate managed probe boundary. Workspace's **82 Rust tests and strict Clippy**
also passed in **36.831 seconds** (cgroup peak **4,293,656,576 bytes**).
Final affected all passed in **403.909 seconds** (cgroup peak **6,442,516,480 bytes**,
8 GiB cap). No new Windows execution result is claimed for these LSP changes.

The [4139ea9 Windows run](https://github.com/jihoon22-lee/devbox/actions/runs/34291364099)
passed native authority/WAL, API Studio, Knowledge, installer coexistence and the
baseline, but packaged Workspace stopped at concurrent history detail/diff loading.
Its generic UI error did not identify the failing boundary; cleanup/dialog checks
were not reached. Host selected-store reads held an exclusive try-lock across
filesystem validation, allowing simultaneous read requests to reject one another.
The host now shares read leases and keeps activation exclusive; pointer and object
checks remain intact. A threaded regression verifies overlapping readers and denied
activation. History shows only fixed native bridge errors, while arbitrary engine
errors remain sanitized. The fixture records bounded history method/issue codes and
duration, without arguments or response contents. Focused host/Clippy, **9 history
UI tests**, product build and **4 generated-request tests** passed in **37.678 seconds**
(cgroup peak **4,411,543,552 bytes**). Final affected all passed in **355.968 seconds**
(cgroup peak **5,172,359,168 bytes**, 8 GiB cap). Windows retry is pending.

The chooser driver also failed an isolated local Windows test: its UI Automation
provider exposed a native Button as a Pane without InvokePattern. The driver now
locates the filename editor/button by native class and ID, checks their HWND parent
and exact process, and uses bounded [Windows messages](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw).
The 1.5-second sends retain the executable/start-time/owned-path guards and do not
broadcast messages. The new `windows-workspace-file-dialog.test.ps1` compiles an
owned disposable dialog, checks cancellation and a Korean filename selection, then
confirms child exit before cleanup. Both actions passed on local Windows; this is
driver evidence, separate from packaged Tauri acceptance. CI runs the same test
before product builds and watches all Workspace fixture scripts. Final affected all
passed in **314.733 seconds**, cgroup peak **1,576,943,616 bytes** under 8 GiB.
The packaged retry remains pending.

The shared Windows LSP process owner now creates servers and managed Node version
probes [suspended](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags),
assigns the kill-on-close Job, then resumes the exact child's sole thread. Failed
assignment/resume reaps the child without a process-only fallback. Probe success,
errors and timeout terminate and confirm the Job; cancellation closes its owned Job.
Server exit confirmation requires a reaped root and an empty Job. A protocol failure
state alone no longer makes manager shutdown report success. The existing Git
Windows implementation supplied the validated thread-resume pattern; no new runtime
or dependency package was introduced.

Windows fixtures verify that a suspended child cannot write before assignment, then
use retained descendant process handles to check normal/hung server shutdown and
probe success/output limit/failure/timeout/cancellation. Those Windows checks await
CI. Portable LSP **125 unit + 6 process tests** and strict Clippy passed. Final
process tests, Workspace **83 Rust tests** and both strict Clippy checks passed in
**60.332 seconds**, cgroup peak **5,522,358,272 bytes** under 8 GiB. Final affected
all passed in **392.131 seconds**, cgroup peak **6,443,929,600 bytes** under 8 GiB.

Windows LSP installation now runs through its own bounded request/worker pool.
Catalog, verified cache/local archive import, installed-index recovery and removal
reuse the existing native installer. Long installs hold neither the Files mutex nor
a project-selection permit. Shutdown signals download cancellation and waits for
actual workers; cancelled caller futures cannot release the native worker early.
The installer keeps its usual cleanup path after cancellation, and synchronous
promotion/index work finishes before retirement. Per-context configuration and
lifecycle commands are connected below; document/WorkspaceEdit integration is pending.

The shared engine now carries a persistent document authority through workspace
clones, rename plans and backups. It checks paths before canonicalization and the
canonical result before file inspection; rename preview/apply/rollback recheck
retained targets before IO. Revocation before apply leaves the file unchanged;
revocation before rollback keeps post-write data and its journal/backup for recovery.
Hosted execution authority defaults to denying document access. Standalone behavior
and hosted no-IO startup recovery remain intact. **130 LSP unit + 15 manager integration
tests and strict Clippy passed**, **65.667 seconds**, cgroup peak **4,518,932,480 bytes**
under 8 GiB. An initial test-only missing import was fixed before this passing run.
Final affected all passed in **392.420 seconds**, cgroup peak **6,200,475,648 bytes**
under the shared 8 GiB limit.

Context-specific LSP configuration now uses private per-worktree metadata and
native revision checks through atomic replacement. Load/save remain usable when
the project is offline and perform no executable or project-definition IO. Explicit
corrupt recovery preserves raw bytes before writing complete UTF-8 JSON; recursive
future-schema detection prevents destructive downgrade. The shared dialog reloads
the native revision after saving. Initial focused verification passed **92 Workspace
Rust tests, strict Clippy, 26 LSP UI tests and product build** in **26.278 seconds**
with a **2,114,584,576-byte** cgroup peak. Final focused settings verification passed
**92 Rust tests, strict Clippy, 28 LSP UI tests and product build** in **36.068 seconds**,
with a **3,868,147,712-byte** cgroup peak. It includes native revision round trips
and disabling recovery for future schemas. The preceding run passed **130 LSP unit
and 6 process tests plus both Rust Clippy checks** but failed a test-only JSX path
literal, corrected before the passing UI run. Final affected all passed in
**385.208 seconds**, cgroup peak **6,443,745,280 bytes** under the shared 8 GiB limit.

[General CI at `2eb2bf0`](https://github.com/jihoon22-lee/devbox/actions/runs/34297918376)
failed both new Windows descendant checks. A disposable local Windows harness using
the actual Job module reproduced inherited-pipe EOF timeout and zero accounting
before the descendant process handle was signaled. Version probes now wait for the
root concurrently and retire its Job before draining EOF. Completion uses a private
port associated before child admission and the
[Windows tree-completion notification](https://devblogs.microsoft.com/oldnewthing/20130405-00/?p=4743).
Ten disposable Windows runs using the actual revised Job module confirmed EOF and
an already-signaled descendant handle in **21.759–36.5554 ms** after fixture release.
The [37d1478 general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34301256774)
subsequently passed every job, including the Windows process tests. A local full
cross-build was blocked by a missing `clang-cl` tool and was not reported as a pass.

[Windows acceptance at `2eb2bf0`](https://github.com/jihoon22-lee/devbox/actions/runs/34297918386)
passed history and reached the sibling-document picker, then failed to find its
owned dialog. A parented disposable Windows Forms chooser reproduced the failure.
The driver now discovers visible native HWNDs for the exact fixture process before
using UI Automation for controls; existing process/path/control checks remain.
The parented fixture's Cancel and Open both passed with the revised driver.
The packaged cleanup and installer steps still require a successful retry.

The [37d1478 Windows product run](https://github.com/jihoon22-lee/devbox/actions/runs/34301256761)
passed the owned chooser and file open, then rejected cleanup with
`source_review_required`. A disposable Windows probe using the actual Git evidence
modules and rfd did not reproduce a change in files, environment or cwd. Native
approval now stores optional component digests and returns fixed changed-evidence
categories, without environment values; old approval records remain readable.
The fixture checks trust both before and after the sibling picker to identify the
remaining cause without bypassing review. Cleanup and installer acceptance remain pending.

Native LSP lifecycle now has context-bound one-time review, cancellation, replay
checks and offline revocation. Review pins saved settings, definitions, exact argv,
ordered admitted PATH, executable/runtime objects and managed code trees. Streaming
limits are 256 MiB/file, 512 MiB total and 20,000 objects. Foreign installed-index
paths are rejected before canonicalization. The shared manager consumes immutable
reviewed configuration and process specifications rather than re-reading unreviewed
config/index paths before native authority checks.

Each active context uses an owned OS thread/Tokio runtime for sessions and retries.
Startup retains context/filesystem/installation permits; installation waits use a
separate lock and retire a context whose installation is changing. Configuration
saves now acquire the context write permit (the earlier classification mistakenly
placed the method in Source's filesystem exclusions). Save, revocation, definition
changes and context retirement stop the owner before mutation. Exit waits for worker,
thread and process retirement. Native startup IDs are published before the first
renderer await, with bounded native cancellation intent and deadline checks. Probes
and initializing children complete cleanup before cancellation returns. Events carry
the native context; the next section connects the native editor/document bridge and review controls.

Focused portable verification passed **132 LSP unit + 15 manager integration tests**.
After correcting a test-only empty argv value, startup wait budget and old allowlist
expectation, **100 Workspace Rust tests, both strict Clippy checks, 6 process tests,
32 API/LSP UI tests, product build and fixture syntax** passed in **69.275 seconds**,
cgroup peak **3,396,943,872 bytes** under 8 GiB. Actual Node peers tested normal actor
retirement and cancellation during initialization, including independent child-exit
checks. Final startup/installation exclusion verification passed **132 LSP unit,
15 manager integration and 101 Workspace Rust tests plus strict Clippy** in
**107.545 seconds**, cgroup peak **5,121,277,952 bytes** under 8 GiB. Final affected
verification selected all and passed in **467.721 seconds**, cgroup peak
**6,442,582,016 bytes** under 8 GiB. These new actor checks have not yet passed Windows CI.

### Native LSP editor and Windows runtime follow-up

Windows Files now exposes explicit execution review and manual lifecycle controls.
Reviews display exact command arguments, workspace and child environment; settings
mount performs no executable inspection. Revision changes/unmount retire pending
reviews, and approval does not auto-start. Native events are matched by all context
fields. Manual stop clears document mirrors; ready events reopen current buffers.

Document commands require native Files grants/revisions. Save/reload verify disk
identity, encoding and mtime/size/hash, and native baseline hashes decide dirty state.
An ordered mirror acknowledges every editor buffer, including unsupported languages;
failed flushes block rename. Native rename preview, disk apply and rollback retain
this whole-editor guard. Atomic writes refresh grants/revisions and all affected open
buffers, including other languages. Committed results survive a failed editor refresh.
Cancel/discard bypass the document mutex. Explicit journal recovery and WSL-native
execution remain incomplete; this is still draft B04.

The [b913c9a Windows product run](https://github.com/jihoon22-lee/devbox/actions/runs/34308418112)
and [general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34308418104) failed
three new native LSP tests; the product run did not reach Source/installer acceptance.
Local Windows probes reproduced Node initialization abort without SystemRoot and a
Node entrypoint failure with a verbatim path. The shared resolver now obtains
SystemRoot from the Windows API and checks canonical equivalence before passing
ordinary script spelling. The document approval fixture now uses the canonical path
returned by native project registration instead of TEMP's short-path alias.
A small MSVC executable using the actual runtime/process/transport/Job modules passed
Node version probing, initialize, shutdown and confirmed process exit on Windows.
Its cross-build passed in **24.009 seconds**, peak **872,325,120 bytes**. This does
not replace the pending full Windows product retry or resolve Source cleanup yet.

Portable checks passed **104 Workspace Rust, 132 LSP unit, 15 manager integration
and both strict Clippy checks**. The real Node document fixture checks missing/stale
grants, false clean claims, save revision rotation, hover/diagnostics, dirty unsupported
target rejection before/after preview, CRLF disk rename, refreshed grants and replay
rejection. Its focused run passed in **69.659 seconds**, peak **2,968,313,856 bytes**.
The first full UI run found an incomplete test mock for the new mirror API; adding
that mock fixed it. **337 shared UI tests, product build and both strict Clippy checks**
then passed in **50.930 seconds**, peak **1,638,375,424 bytes**. All checks used the
shared 8 GiB limit. The packaged fixture now exercises review/cancel/approve, manual
Node start, CodeMirror document sync/edit/save/hover and explicit revocation.
Final affected verification selected all and passed in **496.966 seconds**,
cgroup peak **6,444,388,352 bytes** under 8 GiB. Full Windows retry remains pending.

Follow-up review found that native document fixtures still built some requests from
Windows TEMP's short spelling. The fixture now exposes the Registry's canonical
root for every document path. Read-only inspection buffers keep the clean baseline
already recorded by native open; the editor does not retransmit those large buffers
through the smaller editable mirror budget or block unrelated rename flushes.
Focused **17 native LSP Rust tests, strict Clippy, 5 mirror tests and product build**
passed in **90.443 seconds**, cgroup peak **4,418,715,648 bytes** under 8 GiB.
Final affected verification for this follow-up selected all and passed in
**410.567 seconds**, with a **4,967,952,384-byte** cgroup peak under 8 GiB.


### Explicit native rename-journal recovery

Hosted startup remains preserve-only. A separate explicit metadata list and native
one-time review allow recovery even with missing/disabled/corrupt server settings.
The current Registry lease, protected storage, transport and canonical identity
admit every recorded target before use. All target tabs must be closed; preview
and apply recheck that boundary. Current target and original backup bytes, identities,
mtime/size/hashes are retained. Unknown/future/corrupt journals, sensitive paths,
links, duplicate targets and external changes are preserved. Listing is bounded to
16 private entries and never follows recorded target paths.

Apply retires the server and retains the filesystem write permit, validates every
file before the first replacement, and checks native authority again after temporary
restore bytes are ready. Partial failure reports confirmed restores and keeps its
journal/backups; another review resumes entries already equal to their original.
Cleanup removes only known unchanged backups and leaves unknown children intact.
The UI reads only after an explicit action, displays current/original excerpts,
consumes failed approvals and cancels unmounted or context-retired previews.

Focused recovery validation passed **6 shared engine + 4 native owner tests and both
strict Clippy checks in 60.487 seconds**, peak **4,795,949,056 bytes**. The **36 approval/
recovery/control-panel UI tests, shared typecheck and product build** passed in
**23.217 seconds**, peak **1,620,819,968 bytes**. The packaged Windows fixture now
checks open-editor denial, cancel/replay, external changes and explicit UI CRLF
restoration without server approval. Its execution remains pending.
The broader pass completed **210 Code Pad + 15 manager integration + 108 Workspace
Rust tests, strict Clippy, 342 shared UI tests and product build in 141.355 seconds**,
peak **4,825,878,528 bytes**. A subsequent adversarial test reproduced an external
write during the new final authority callback (6.428-second expected failure).
The restore helper now validates the target snapshot after that callback too.
Recovery responses retain committed/partial results if later metadata checks fail,
and report pending cleanup separately from successful restoration.
The final focused correction passed **7 recovery engine + 4 native owner tests,
strict Clippy, 4 recovery UI tests and product build in 42.095 seconds**, peak
**3,156,873,216 bytes**. Six literal Windows renderer expressions and the fixture's
Node syntax parsed successfully. Final recovery affected verification selected all and passed in **453.937 seconds**,
with a **6,442,975,232-byte** cgroup peak under the shared 8 GiB cap.

Archive chooser results are private one-time UUID capabilities: 32 files/512 MiB,
180-second expiry, held file/parent identities and content metadata, no links,
no protected product stores and no editor grant. Import validates all tokens before
consumption, copies bounded private snapshots, then applies the existing exact
catalog SHA-256/size/Node-lock verification. Cancel/unmount/late picker results release
choices. Snapshot cleanup removes only the owner's known files and preserves unknown
entries or changed roots. Focused **29 installer tests, 89 Workspace Rust tests,
strict Clippy, 26 LSP UI tests and product build** passed; the combined final run was
**59.385 seconds**, cgroup peak **4,563,910,656 bytes** under 8 GiB.

[d612d4a general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34294469016)
passed. Its [Windows run](https://github.com/jihoon22-lee/devbox/actions/runs/34294469013)
passed the chooser driver and history UI, then a later native request exceeded the
fixture's 10-second CDP deadline. The truncated expression did not identify the exact
method. Native fixture calls now allow 35 seconds for the 29-second native admission
budget, with bounded method/state/issue/timing records and owned-dialog diagnostics.
No cleanup or packaged dialog completion is inferred from that failed run.

The new Windows installer fixture confines the app's HTTPS traffic to a local failing
proxy, verifies Files editing while a download waits, then imports catalog-hashed
Rust and Node archives through the actual chooser. It checks explicit UI uninstall,
cancelled install confirmation, cache reinstall with no proxy requests, Node closure
multi-selection and no server auto-start. Actual packaged execution is pending.
Local Windows disposable chooser **cancel, single selection and Korean multi-file
selection all passed**. Two earlier multi-selection driver attempts failed on
PowerShell collection unrolling; explicit typed input collection fixed that failure.
Final focused Rust/Clippy/UI/build and **5 Node fixture regressions** passed in
**19.295 seconds**, cgroup peak **1,561,645,056 bytes** under 8 GiB. Final affected
all passed in **390.914 seconds**, cgroup peak **6,216,245,248 bytes** under 8 GiB.

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

## Windows acceptance corrections

The [09a52be Windows run](https://github.com/jihoon22-lee/devbox/actions/runs/34247438817)
passed native authority/WAL tests, API Studio workflows and installer coexistence,
but failed first private-overlay publication and Knowledge vault review. Files
execution was not reached. General CI for that commit passed.

Private generation paths plus the definition temporary filename can exceed
Windows MAX_PATH. The shared identity opener now uses Rust OpenOptions with the
same no-follow/access/share flags, retaining long-path conversion before querying
the same native handle. A Windows native API probe without long-path opt-in
reproduced ordinary-path error 3 at 306 characters while the extended spelling
opened the same owned directory. New regressions cover long-directory identity, retained
handles after replacement and complete private-overlay publication. The packaged
Workspace result still requires a new Windows run.

Knowledge vault preview could reject a hot rollback journal left by process exit
as `import_database_invalid`. A subprocess regression reproduces the exact error
with the old read-only connection. The selected product DB now permits SQLite
recovery without CREATE or migration, enables query-only before validation, and
still rejects setting writes. It never opens the legacy DB writable or edits vault
files. The regression confirms rollback of uncommitted rows, the original binding,
query-only enforcement and no missing-database creation. This correction belongs
to the failing cross-product acceptance of this B04 PR, not a new feature bundle.

Focused regression checks and strict Clippy passed in **69.116 seconds** under
the shared 8 GiB cap (cgroup peak **4,646,440,960 bytes**). Windows-specific Rust
checks and packaged workflows remain pending on the corrected commit. Final
`pnpm verify:affected` selected all and passed in **571.894 seconds**, under the
shared 8 GiB cap (cgroup peak **6,445,330,432 bytes**).

## Source execution and cross-owner IO

Source now connects the shared Git panels through a separate native approval
owner. The exact Git launcher/core executables, config/include/hook evidence,
project execution definitions and environment bind a versioned approval. A fresh
native snapshot precedes each operation; another root or changed evidence cannot
reuse permission. Revocation needs no external config read. Task/LSP trust is not
granted by Git review. `gix-config =0.60.0` supplies the pure parser; the dependency
decision is in the Workspace architecture and notices are regenerated.

Native `--git-dir`/`--work-tree` arguments prevent config redirection. Async poll
scopes and inherited worker scopes retain ownership without leaking between
tasks. Context-scoped cancellation reaches the existing process-tree supervisor.
Git/file IO share a reader/writer boundary; private recovery remains writable
while Git runs. The editor rejects the products' native authority-store paths,
including when a broad project/picker scope would otherwise cover them. Commit
drafts remain mounted across review/revocation and routes.

The [50c5253 Windows run](https://github.com/jihoon22-lee/devbox/actions/runs/34253708553)
completed definition/local-overlay assertions and the full Knowledge workflow,
including vault rebinding/recovery. Dependencies then failed before its engine:
its component was missing from the authority catalog. Source/Dependencies are now
registered explicitly in catalog revision 7, with a regression against the native
allowlist. The baseline artifact upload separately failed with service HTTP 403.
The [general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34253708525)
passed frontend and both Rust platforms but failed the stale notices digest;
generation and the local dependency-policy/Cargo deny checks now pass.

Focused checks passed 68 Workspace, 129 Repo Manager, 24 Git and 2 catalog tests
plus strict Clippy in 45.062 seconds. Subsequent Source UI approval/cancel/draft
regressions (3 tests), Workspace build and four generated-request/decoded-script
checks passed in 15.46 seconds. Final combined focused checks passed 70 Workspace + 130 Repo Manager + 24 Git +
2 catalog tests, strict Clippy, shared UI 317 + Workspace UI 14, build and Node
regressions in **95.628 seconds**, under 8 GiB (peak **4,161,253,376 bytes**).
The final monotonic-budget correction passed **71 Workspace tests/strict Clippy**
in 19.291 seconds. Queue, observation and Git execution share one monotonic expiry;
a wall-clock change cannot extend the execution capability. These local results
do not establish Windows Source/Dependencies/Files parity.

The first full run exposed a Source UI test interaction before approval mount
effects settled. The test now awaits React completion and an enabled fieldset
before entering its draft; all 14 Workspace UI tests passed in 4.115 seconds.
Final `pnpm verify:affected` selected all and passed in **946.342 seconds**,
under the shared 8 GiB cap (cgroup peak **6,445,232,128 bytes**).

The Windows fixture adds explicit/cancelled/stale Git approval, no pre-approval
fsmonitor execution, config worktree-redirection rejection, UI selected stage and
reviewed commit, changed-hook rejection, owned Git cancellation, concurrent file
write rejection and continued editor recovery. Each completed Workspace feature
now records independent evidence before the next fixture. Native rejection codes
are retained in bounded renderer diagnostics instead of an uninformative Object.

## Source worktrees, Files navigation and early cancellation

Source change rows and new-side diff line positions now open the retained Files
editor after hydration. Reopening preserves a dirty buffer and one watcher;
context switching stays blocked until a closed tab releases its watcher.
Worktree creation consumes a native review of the branch, canonical target,
existing parent identity, absence and current Git approval. It rejects Git and
product authority storage before mutation. Success offers Registry review without
automatic registration, context selection, task execution or trust transfer.
Cancellation reaches requests before native queue/evidence work as well as Git.
Context-scoped cancellation intent is bounded and expires; caller cancellation
retains running worker ownership. Git cancellation does not roll back mutations.

The shared no-link check now visits ancestors first. Workspace checks Windows
DOS drive mappings before IO, resolves local SUBST prefixes and rejects
WSL/network/unknown device aliases. A Windows-only fixture creates, queries and
removes a unique DOS object without opening its synthetic WSL target. The file
protection policy is now shared by Files and worktree creation within Workspace.

[cdeaa37 CI](https://github.com/jihoon22-lee/devbox/actions/runs/34268405267) passed
frontend, Linux Rust, catalog and dependency policy, but its Windows test failed
because the new file-protection fixture passed a TEMP alias as the project root.
The production Registry canonicalizes that root; the fixture now matches that
boundary, as the existing project-document fixture already did. The same failure
stopped [Windows packaged Source/Dependencies/Files](https://github.com/jihoon22-lee/devbox/actions/runs/34268405227)
before execution. API Studio, Knowledge, installer coexistence and performance
baseline continued successfully. The Windows outcome is not a packaged PASS.

Focused checks passed **78 Workspace + 131 Repo Manager + 24 Git + 20 filesystem
Rust tests**, strict Clippy, **320 shared UI + 18 Workspace UI tests**, Workspace
build and four decoded-script/request regressions in **67.236 seconds**, under
8 GiB (cgroup peak **3,331,039,232 bytes**). The native Windows device test remains
pending on Windows. The expanded Windows fixture covers unsaved Source→Files
navigation across commit, worktree review cancel/concurrent target, separate
registration/selection, and linked-worktree Files→stage→commit.

Review found that folding the directory case in Git-returned paths could admit
a distinct NTFS worktree. Matching now preserves directory case while normalizing
drive/UNC-authority spelling and separators. The regression and all **79 Workspace
tests/Clippy passed in 45.543 seconds**. The in-flight affected run was stopped
before completion to include this correction; it is not reported as a pass.
Final affected verification selected all and passed in **536.407 seconds**, under
the shared 8 GiB cap (cgroup peak **6,444,761,088 bytes**).

## Remaining acceptance

Reviewed sibling cleanup awaits Windows acceptance. Importer mappings, Windows
LSP integration and WSL-native transport remain incomplete. Files currently serializes native IO under
one state mutex; remote independent cancellation needs work before WSL acceptance.
Native file-dialog selection and its Windows fixture are implemented; actual
dialog execution remains pending. Strict metadata parsing is an activation prerequisite, not a completed
legacy importer. No R/S or completion issue is closed by this draft.

## Test tool advisory correction

CI at `833854a` found the newly indexed
[Vitest mock redirect advisory](https://github.com/advisories/GHSA-82fw-gwwq-j7x9).
All eight declared Vitest consumers now require `^4.1.11`; the lockfile resolves
Vitest/mocker and their companion packages to 4.1.11. This updates development
tooling only, preserving the production dependency graph. `pnpm audit
--audit-level=moderate` reported no known vulnerabilities; notices generation and
policy checking passed in **3.847 seconds**, with a **204,197,888-byte** cgroup
peak. The notice package rows remained unchanged; only the pnpm lockfile digest
changed. Full affected
verification with the updated test runner passed in **546.214 seconds**, with a
**6,446,292,992-byte** cgroup peak under the shared 8 GiB limit.


## Explicit sibling cleanup scope

Cleanup now reviews registered sibling IDs separately from selected-root Git
permission. Metadata enumeration does not probe siblings; chosen members bind
native contexts, Git/config/hook/tool evidence and project definitions to a
one-time approval, with eight-member/64 MiB evidence limits and the shared preview
bound. Invalid/future metadata is preserved. Basic Source operations never load
this broader scope; private revocation works when a sibling is offline or removed.

The native policy rejects unknown Git-returned paths before IO. Open Files grants
block worktree admission by physical ancestor identity, including OS-picker files;
the existing write permit excludes new opens during cleanup apply. The shared
cleanup safety/revision/final-confirmation behavior remains in charge of deletion.
Scope changes clear only the cleanup panel's previous preview, preserving commit
and editor drafts. A removed worktree leaves its committed branch and Registry
record intact for separate review.

The first focused run passed **82 Workspace Rust tests**, then failed only a
Clippy style check in a new test. After correction, **82 Rust tests, strict Clippy,
21 product UI tests and build passed in 36.412 seconds** (cgroup peak
**3,427,184,640 bytes**). The UI regressions cover default-unselected explicit
review, token-only approval, cancellation, offline revocation and late-preview
retirement; the native tests cover foreign/stale/duplicate IDs and native picker
document protection. The final pass after removing duplicate evidence
revalidation and disabling scope changes during another Source action passed
**82 Rust tests, strict Clippy, 320 shared UI + 21 product UI tests, build and
fixture syntax in 59.131 seconds**, with a **3,617,853,440-byte** cgroup peak.

The extended Windows fixture checks missing scope, cancelled/replayed approval,
changed sibling-only execution evidence, continuing basic Git, reapproval and
final UI cleanup while preserving the branch. All **37 literal renderer
expressions** parse successfully. Actual Windows cleanup execution remains pending.

Final affected verification selected all and passed in **364.856 seconds**,
with a **5,667,094,528-byte** cgroup peak under the shared 8 GiB limit.


## Windows checkpoint and native file dialog

For PR head `833854a` (tested merge `509f090`, base `82e257e`), [Windows product acceptance](https://github.com/jihoon22-lee/devbox/actions/runs/34287998647)
passed native authority/WAL tests, packaged Source/Dependencies/Files/definitions,
worktree create/register/select/edit/stage/commit, API Studio, Knowledge and
installer coexistence. The legacy performance baseline passed too. The corrected
TEMP alias fixture and Windows DOS mapping test now pass. [General CI](https://github.com/jihoon22-lee/devbox/actions/runs/34287998474)
passed every job except the Vitest advisory, corrected by `0547ffa`.

The next fixture drives the actual Windows file chooser through UI Automation.
It matches the fixture process creation time and canonical executable, restricts
selection to the owned temporary directory, and invokes only that process's
chooser controls. It tests cancellation without a grant, one explicitly chosen
out-of-project file, rejection of another file, and native save. Source also opens
a sibling document through the actual picker, verifies that it blocks cleanup,
then closes it before final worktree removal.

Windows PowerShell parsed the helper, loaded the UI Automation assemblies,
compiled its canonical-path helper and exercised that helper on a Windows
directory successfully. Node syntax checks and all four generated
Workspace-request regressions passed. Actual dialog execution and
the new cleanup flow await the next Windows run; no dialog PASS is claimed yet.

Final dialog affected verification selected all and passed in **312.107 seconds**,
with a **1,613,025,280-byte** cgroup peak under the shared 8 GiB limit.
