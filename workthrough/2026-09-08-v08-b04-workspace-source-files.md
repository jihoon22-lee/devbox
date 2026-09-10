# B04 Workspace Source and Files

WP #546 / R02–06, R08, R13, R16–21, R24–25 and S01. Draft PR #557 owns
Registry, Overview/Source/Files/Dependencies, legacy import and WSL-native LSP.
Runtime and Terminal/session owners remain B05/B06. B01/B02/B03 are complete (3/9);
this packet does not close #546, #541 or #542.

## Result and implementation boundary

The existing Workbench, Repo Manager and Code Pad UI/tests share
`packages/workspace-features`, with lazy product routes, retained drafts and scoped
CSS. Legacy entry points consume the same components. Native component builds
omit standalone bootstrap/web assets and use immutable generation-specific stores,
common snapshots/handoffs/caches. Opening the product does not migrate an old
identifier, activate legacy data or start LSP. v0.7's public topology remains intact.
The architecture contract is [v0.8 Workspace](../docs/architecture/v0.8-workspace.md).

- **Registry and definitions:** opaque Project/Worktree/Repo IDs, root/common-Git
  objects, aliases/rebind and target/distro binding are distinct from trust.
  One native writer owns the product directory/lock. Registration uses expiring
  one-time native previews and rechecks linked-worktree pointers/backlinks, bytes
  and physical identities. Pointer transport checks precede IO. Activation cannot
  replace an existing pointer; corrupt/future metadata and bounded prepared
  generations remain preserved. Project/local definition review shows before/after
  content, checks revisions and explicit removal/precedence, and revokes execution
  approval on changes. Export includes shared manifest fields only.
- **Source and Dependencies:** retained Git panels support status/diff/history,
  selected stage/commit, fetch/ff-only pull/configured-upstream push, worktree
  creation and explicit root/sibling cleanup. Diff lines open Files. Native review
  captures commands, config/includes/hooks, definitions and environment; actual Git
  children use that captured environment, including after a native chooser.
  Cleanup scope separately approves up to eight registered siblings and rejects
  unknown Git-returned paths before IO. Open editor grants block removal by
  physical ancestor identity. Cleanup preserves branches and Registry records.
  Dependency Lens keeps local graph/duplicate/stale-lock diagnostics and separately
  reviewed, bounded remote opt-in/redaction. Import/open does not trigger network,
  hooks, package installation or distro startup.
- **Files:** native project or OS-picker consent admits at most 64 documents/choices.
  Save/rename/delete check document revision, object, content and ancestor evidence;
  DOS aliases, symlinks/reparse points and foreign transports are checked before IO.
  CodeMirror, single-file mode, encoding/CRLF, bookmarks, two views, preview and
  recovery remain shared. Dirty drafts survive route switches and block context
  changes. Recovery binds context, entry bytes and file snapshot to one-use native
  tokens; raw legacy recovery mutation is unavailable in product mode.
- **Windows LSP:** installation/cache/archive import runs through a separate bounded
  queue. Expiring picker tokens grant no file/editor access; private pinned copies
  undergo catalog/digest/Node-lock verification. Context-local configuration uses
  native revisions and preserves corrupt/future evidence. Execution review binds
  executable/runtime/package/environment/definitions, including probes and retries.
  Owned Windows Jobs retain starting/retrying/cancelled children until confirmed
  retirement, including product exit. Native document authority mirrors every open
  buffer and guards WorkspaceEdit preview/atomic apply/rollback. Explicit journal
  recovery works with disabled/missing/corrupt settings, preflights all targets and
  backups, preserves partial results, and deletes only unchanged owned backups.
- **Concurrency:** context/filesystem permits span admission, queues and worker
  retirement after caller cancellation. Files and Git coordinate disk operations.
  Installation retains its own operation lock. Initialization now publishes the
  host once using `OnceLock`, eliminating a contended read mutex that could return
  `busy`; worker/admission/context limits remain unchanged.

## Data conversion and native settings

- Fixed Workbench/Code Pad identifiers own a bounded, cancellable snapshot job.
  It pins source objects/bytes and the fixed inventory, preserves exact data and
  publishes its manifest last. Restart catalog entries require byte revalidation;
  verified snapshots survive source loss. This detects observed changes, without
  claiming an app-wide JSON transaction. There is no SQLite copy in this importer.
- `com.devbox.codepad` and `com.workbench.codepad` remain independent sources,
  including when bytes match. Corrupt/future files and incomplete jobs are preserved.
  No source root is renamed/merged and no snapshot alone becomes file authority.
- Profiles preserve old IDs/full metadata alongside new Workspace IDs. Explicit
  new/reuse/keep-both/skip choices use one Registry CAS. Windows folder registration
  and profile binding commit together; selection/trust remain separate. Imported
  ports are defaults; environment/service references remain owner-specific metadata.
- Workbench templates reuse the original strict validator. Imported provenance,
  empty roots and WSL/Git/port/service defaults survive. Local create/edit/archive/
  restore use native IDs and Registry CAS; archiving preserves existing profiles.
  Repeat import preserves destination edits. Windows instantiation atomically
  registers the selected folder and new profile; WSL instantiation remains pending.
- Sessions keep original document IDs, cursors/bookmarks, two views and eligible
  recent files. Recovery keeps unsaved text/base hashes/timestamps, rejects capacity
  overflow without eviction, and preserves unrelated buffers. Closed tabs, context,
  one-use expiry and exact destination preimages guard import/restore. Metadata and
  receipts publish atomically; repeats preserve later edits/discards.
- Native session/recovery revisions prevent stale autosave from replacing newer
  data. Recovery chains revisions, preserves unavailable entries/backups and opens
  completed paths through normal native file admission after session hydration.
- LSP settings import preserves server/runtime preferences, retargets the current
  root and forces disabled state. Restore also disables execution. Closed tabs,
  context permits and one-use preimage checks guard replacement. Normal saves retain
  receipts. Offline metadata remains accessible; imports never resolve/probe/install
  a command or inherit execution approval.
- Verified last-workspace onboarding reads only the native snapshot job ID.
  Windows review requires separate registration/selection/trust; unsupported/WSL
  metadata does not automatically resolve paths or start a distro.
- Window snapshot v2 adds fixed `window-state-v1.json` inputs; v1 keeps its original
  inventory/content IDs. Native monitor/DPI adjustment and explicit replacement
  review preserve previous geometry in bounded immutable history. Each OS mutation
  checks deadline/cancellation. Partial OS failure is not a filesystem transaction.
  Primary-window mapping is implemented; secondary windows belong to B07.

Normal metadata publication and its preserved preimage are separate atomic steps.
No transaction across arbitrary files, Registry and external vaults is claimed.
The Windows atomic writer handles deep Korean/space generation paths through
extended native sibling paths, preserving write-through and bounded sharing retries.

## WSL native ownership and definitions

The same-source static helper is packaged privately with Workspace. Windows pins
its compiled size/hash and every resource ancestor, launches structured argv by
registered distro GUID, and checks Registry values/backing objects. Metadata-only
backing checks do not read a VHD. `Flags & 0x8` selects WSL2 independently of the
Registry filesystem format. Stopped distributions require an explicit start choice.
The lazy WSL form lists metadata without startup and uses expiring native
registration/rebind previews with final byte CAS; late UI results are cancelled.

- Linux observations retain root/Git ancestry and pointer/backlink evidence.
  Persistent IDs combine filesystem identity/type, inode and birth time. Only
  wslfs `0x53464846` plus statx ENOSYS admits its native NTFS file ID with a nonzero
  sequence. An owned Windows probe matched inode to actual backing file ID,
  verified restart stability and replacement differences. WSL1 lacks descriptor
  fdinfo; exact root device/fsid and unambiguous mountinfo remain mandatory.
- File contents must stay on the selected distro root filesystem. drvfs/9p,
  foreign mounts, links/devices and another context are rejected before content IO.
  Shared Code Pad grants retain revisions and exact bytes for read/save/rename/
  delete. Quick Open and previews share guarded, bounded traversal/image admission.
- Saves check cancellation/authority before staging and immediately before atomic
  replacement. WSL1 rename lacks renameat2: retained-parent `linkat` publishes a
  non-overwriting name, then authority/bytes/identities are rechecked before unlink.
  Interruption can leave both names; this is not atomic namespace rename. No retry
  or automatic destination deletion is performed.
- Windows retains only acknowledged WSL document metadata and private session/
  recovery buffers. Polling never changes dirty text or save authority. Events and
  cleanup retain the original context. Reviewed recovery uses current helper
  revisions and the same guarded encoding/CRLF save.
- Reconnection confirms retirement, admits the same Registry/root without startup
  and invalidates old revisions. Reopening preserves latest dirty text/encoding/
  cursor/bookmarks when disk bytes match. Changed/missing files retain buffers and
  enter external-change review. Late watcher reads from retired connections are
  ignored. Exit retains any owner whose retirement is unconfirmed.

The existing manifest/overlay schema and bounded project snapshots were moved to
`workspace-wsl` in mechanical commit `8e77836`. The Windows type adapter keeps its
native ProjectLease; semantic transport/write changes follow in this PR.

WSL definition reads and trust review now use helper-owned exact bytes, retained
objects and first absent components. Limits remain 130 files, 1,024 objects,
2 MiB per source and 8 MiB total. Ancestor/content reads check native admission,
cancellation and request deadline. Windows local overlays remain in the private
generation; one-use Registry trust revalidates both owners. Reading or approving
never executes a task, hook, package or language server.

Reviewed WSL manifest writes consume their native owner before IO, including
failure. Existing files use Code Pad's bounded conflict and encoding/CRLF policy
with a source/authority precommit guard. First creation uses retained directory
descriptors and complete, non-overwriting publication; only the newly created
parent's retained identity can replace an observed absence. Concurrent creation,
source/parent replacement and cancellation reject publication. Cleanup removes
only unchanged owned staging data. Failure can leave an empty .devbox directory or
a complete unacknowledged file. The Windows owner revokes trust before writing and
coordinates definitions with Git/editor filesystem permits; no write is replayed.

## Verification evidence

| Scope | Actual result |
|---|---|
| `01db848` general CI | [PASS](https://github.com/jihoon22-lee/devbox/actions/runs/34398314297), including Windows Rust and dependency policy |
| `01db848` Windows product acceptance | [PASS](https://github.com/jihoon22-lee/devbox/actions/runs/34398314206): Workspace native 141 tests, WSL1 Rust/helper 2.72 s, full packaged shell/journal/context clear, API Studio/Knowledge, installer coexistence |
| Packaged Workspace coverage in that run | Template editing, primary window import/restore, Source/Files, old/current Code Pad imports/repeat/restore, Rust/Node local archive/cache, LSP approval/hover/save/second hover and journal recovery |
| Shared definition extraction | Schema 6 + snapshot 4 PASS; affected all PASS 466.898 s, sampled RSS 4,429,033,472 B, cgroup 6,442,696,704 B / 8 GiB |
| New WSL definitions | Native write 5 + transport/context 3 + host definition 4 + shared IO exclusion PASS; Linux strict Clippy; focused run 55.991 s / sampled RSS 4,473,958,400 B |
| Final focused regression | Helper 35 + actual GNU pipe 5 + host snapshot 4 PASS, Linux strict Clippy; 50.936 s / sampled RSS 4,268,306,432 B |
| Windows compile/static helper | Full Workspace MSVC all-target strict Clippy and musl release build PASS 50.895 s / sampled RSS 1,978,994,688 B |
| Actual local Windows pipe | Owned WSL1 3.075 s and WSL2 3.977 s PASS: files, definition create/read, consumed-write rejection, separate editor changes invalidating definitions, EOF, restart identity/replacement and cleanup |

The first new native build missed the directory-kind argument; Windows Clippy
then required boxing the transport enum. Both were corrected and rechecked.
The first bare-WSL1 diagnostic used an unavailable UNC path; its replacement reads
persisted bytes through the real helper and propagates child failure status. All
fixture distributions/directories were removed. Existing user distributions/data
were not fixtures. These local pipe results are distinct from the expanded Windows
Registry/trust/local-overlay fixture. That expanded actual WSL1 Rust/helper fixture
now passed at `5571786` in **6.94 s** in [run 34404024294](https://github.com/jihoon22-lee/devbox/actions/runs/34404024294);
its [general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34404024291) also passed.
The product run passed native Workspace 135 tests, API Studio/Knowledge and
installer coexistence, but packaged LSP editor save failed as detailed below.

Final `pnpm verify:affected` selected all and passed in **498.179 s**, sampled RSS
**6,445,371,392 B**, cgroup **6,444,003,328 B / 8 GiB**. New CI results remain pending.
Prior failing intermediate runs are preserved in this PR's commit/Actions history;
they do not override the exact-commit PASS evidence above. Initial Workspace
bundle at `01db848` is 279,012/280,000 B, gzip 82,319/90,000 B; limits were not raised.

## Native Git engine preparation

Repo Manager's default standalone and Workspace desktop builds preserve Tauri and
its scheduler. The native library now reuses the same typed Source dispatcher and
Git/Dependency logic without GUI startup; native callers supply an execution
capability and runtime. Exact-root/closed-method validation precedes IO, and native
Source queries do not publish standalone repository snapshots. This does not yet
enable WSL command execution. No library version changed; Tokio was already in the
lockfile and notices were regenerated for its new direct dependency edge.

Headless **130 tests** and both headless/desktop Linux strict Clippy PASS
(**26.459 s**, sampled RSS **1,580,789,760 B**). Full Workspace MSVC all-target strict
Clippy, native musl dependency-tree checks and dependency policy PASS (**18.990 s**,
sampled RSS **1,751,560,192 B**). The native graph has no Tauri/GTK/WebKit runtime or
build dependencies. Initial unused desktop imports/entry points and a legacy test
wrapper's feature gate were corrected before these results.

Final affected all PASS: **680.021 s**, sampled RSS **5,809,029,120 B**,
cgroup **6,444,793,856 B / 8 GiB**.

An exclusively owned kernel probe on WSL1/WSL2 confirmed subreaper adoption after
double-fork/setsid and repeated waitid WNOWAIT followed by exact child reaping.
WSL1 has no pidfd_open; WSL2 provides it. All synthetic distributions/directories
were removed. This is capability evidence for the next process owner, not Git/LSP
execution acceptance. See [subreaper semantics](https://www.man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html).

## File save admission during LSP reads

At `5571786`, the packaged trace recorded `save_file` failing with `context_busy`
after 11 ms while LSP document work was active. No save began and the dirty buffer
remained. Earlier full `01db848` acceptance did not expose this scheduling race.

File and definition writers now wait for exclusive filesystem admission within
the original request deadline and existing bounded queue. The wait owns no Files
mutex/worker, does not repeat authentication or IO, and checks shutdown/expiry.
After admission the original context and deadline are checked again before normal
snapshot-guarded IO. Read operations and private recovery metadata keep their
existing admission. Cancelled/expired waits cannot begin a write.

Native regressions for retained readers, exactly-one admission, expiry and shutdown
plus Linux Clippy PASS (**75.762 s**, sampled RSS **4,303,204,352 B**). Generated
Windows fixture expression/request tests **9 PASS**, full Workspace MSVC all-target
strict Clippy PASS (**10.675 s**, sampled RSS **1,506,586,624 B**). The packaged
fixture now holds a real LSP hover read across Save and requires one successful
save before didSave/second hover; its actual Windows run is pending.

Final affected all PASS: **549.444 s**, sampled RSS **5,879,857,152 B**,
cgroup **6,444,339,200 B / 8 GiB**.

The owned kernel probe also verified `/proc/self/exe` reexecution and parent-death
SIGTERM delivery/reaping on WSL1 and WSL2. WSL1 lacks `/proc/self/task/<pid>/children`
as well as pidfd, so the future process owner must not depend on either. Both
probe distributions and owned directories were removed.

## Linux native process ownership

The private helper now has a per-job `--supervise` entry point, outside its pipe
method allowlist. It sets subreaper/parent-death handling before creating threads,
preserves stdio/cwd/environment and observes root exit without reaping. Retirement
proves each direct-child PID using waitid WNOWAIT before signalling/reaping, then
collects adopted double-fork/setsid descendants until ECHILD. WSL1's missing pidfd
and children file do not weaken ownership. An unconfirmed tree keeps its owner.

Native Git policies can opt into mapped-image `/proc/self/exe` reexecution before
sharing their capability. Cancellation sends TERM to that still-unreaped owner
and waits for retirement. Ordinary Linux Git groups now receive their final signal
before root reaping, closing the PID reuse window. Windows Job behavior is unchanged.
WSL Source/LSP methods and child-aware session shutdown are not yet enabled.

Git **25** and supervisor **5** regressions plus Linux Clippy PASS; the additional
ignored parent-death fixture is actually executed by its isolated subprocess test.
Static musl helper and full Workspace MSVC all-target strict Clippy PASS
(**101.081 s**, sampled RSS **1,474,158,592 B**). An initial combined invocation
passed tests/Clippy but failed static build because the local musl compiler path
was omitted; restoring the existing private toolchain resolved it.

Actual Windows→WSL1 **1.922 s** and WSL2 **2.523 s** ran the built Rust owner with
synthetic native children: normal root exit/status, double-fork/setsid retirement,
cancellation/other-job preservation, parent-death adoption, mapped-image reexecution,
invalid argv and final ECHILD all PASS. Owned distributions/directories were removed.
The combined fixture run used **4.989 s**, sampled RSS **40,435,712 B**. These are
process-owner observations, not project Git/LSP or packaged WebView acceptance.
Final `pnpm verify:affected` selected all and PASS (**570.759 s**, sampled RSS
**4,950,102,016 B**, cgroup peak **6,444,773,376 B** within the 8 GiB limit).

## Native WSL Git review

The shared helper library now owns the unchanged Git config parser and bounded
file/include/hook traversal. Windows injects its original path admission and native
environment resolver; Linux injects distro-native filesystem admission and retains
an observed root/context. Source approval still lives in Windows and combines
native Git files/environment with project-definition evidence. No pipe method
executes Git yet, and Windows-native Git rejects the WSL evidence variant.

Linux freezes command environment without transient launcher cwd/depth/interop
fields, skips relative/linked/foreign PATH entries, and observes executable bytes
without probing the tool. It explicitly binds system config to the distro /etc or
selected local prefix when no inherited override exists. Empty overrides retain
Git's disabled-config behavior. Reattach cannot clear changed sources. Four
48-MiB snapshots per pipe retain existing include/object/hook bounds.

Helper **45 tests**, Source focused tests and Linux all-target strict Clippy PASS
(**48.409 s**, sampled RSS **4,384,866,304 B**). Earlier extraction checks passed
helper tests but found one unmigrated cleanup-scope resolver call; fixing it passed
the intermediate native tests/Clippy (**62.768 s**). Full Workspace MSVC all-target
strict Clippy and static musl release helper PASS (**83.084 s**, sampled RSS
**1,733,296,128 B**). The existing musl cdylib warning excludes an unsupported
extra crate type; the executable is built successfully.

The product CI now provisions Git explicitly in its current run-owned disposable
WSL distro, then tests Windows Source review, approval, changed-hook rejection,
reconnect stability, revoke and unchanged original config/no hook execution. This
new actual Windows result is pending; it is not included in the local PASS count.
No user distro receives an automatic package installation.
Dependency notices regeneration/check and workflow YAML parsing PASS (**3.376 s**);
the actual Windows PowerShell parser also accepted the owned-fixture script.

## Verification fixture synchronization

The `ff43086` Windows product run failed in the older Source cancellation fixture:
it expected immediate `context_busy`, while the new bounded writer wait correctly
returned `request_expired`. The fixture now supplies an original 500-ms deadline,
asserts unchanged file bytes before cancelling Git, and verifies the later explicit
save. Its generated request retains that deadline; accepted failures never retry.
The new packaged LSP-overlap fixture was not reached in that run.

The same head's frontend job found two existing API Studio timing races. The
OpenAPI handoff test now settles asynchronous file/effect setup, controls the native
reply and asserts no close before it and exactly one close afterwards. The Knowledge
draft test settles the async action before checking the rendered completion, rather
than treating a mock invocation as completion. Product behavior is unchanged.

The first Git-review affected run (**594.233 s**, failed) reached Knowledge's
immediate vault-lock reacquire assertion while a parallel hot-journal test spawned
a subprocess. On Unix, fork copies refer to the same flock description until close
(see [flock ownership](https://man7.org/linux/man-pages/man2/flock.2.html)); isolating
the lease fixture after exec removes that inheritance interval from this lifecycle
assertion. The Windows test and production ownership implementation are unchanged.

Generated request **10**, API Studio **17** and Knowledge **37** focused tests PASS
(**37.725 s**, sampled RSS **2,485,501,952 B**). Final all-scope affected verification
PASS after these concrete fixes (**424.017 s**, sampled RSS **4,709,679,104 B**,
cgroup peak **5,004,349,440 B** within the 8 GiB limit). Neither prior failed run
is recorded as PASS. The `ff43086` general CI finished with only the two frontend
test failures above; Rust, Windows compile, catalog and dependency jobs passed.

## Native WSL Source execution

At `90f919f`, [general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34414940838)
and [product CI](https://github.com/jihoon22-lee/devbox/actions/runs/34414940852)
passed. The actual Windows WSL1 review/approval/revoke fixture passed in **6.17 s**.
Packaged Source verified deadline expiry without replay; both packaged LSP variants
verified a real overlapping file save waits for its reader. Expanded WSL native
Registry/definition checks also passed. These results resolve the pending checks
above, without claiming the later Source execution fixture was already run.

The native helper now reuses headless Repo Manager Source dispatch and its owned
Linux Git supervisor. A closed method allowlist excludes worktree create/cleanup
until destination and sibling capabilities exist. Each command requires a one-use
pipe ticket and revalidation of Windows private approval, context, definitions,
native root/Git objects and the original deadline. Unix file modes join the digest;
mount admission rejects the effective foreign/autofs mount before target IO.

A preparation acknowledgement guarantees a final retirement response even after a
partial command frame. Per-operation cancellation cannot poison later requests.
Partial frame decoding survives timeout; Windows never infers Linux retirement
from killing wsl.exe. Blocking execution and native descendants retain the context,
filesystem permit and admission owner until confirmed shutdown. The Windows callback
runs outside the pipe runtime and avoids reacquiring its own Git connection.

Local helper tests (**47 library**, **1 gate**, **5 file pipe**, **4 Source pipe**,
**5 supervisor**, with the isolated supervisor subfixture run by its parent), full
Workspace MSVC strict Clippy and static musl release passed (**96.942 s**, sampled
RSS **1,657,241,600 B**). An additional actual pipe test now rejects a forged approval
ticket without launching a hook or changing HEAD. All **5 Source pipe tests**,
helper strict Clippy, generated dependency notices/check and the static headless
dependency graph passed (**12.301 s**, sampled RSS **1,131,192,320 B**).
The new Windows Source host fixture
covers selected stage/commit, live revocation and cancellation of a detached hook
while the filesystem permit remains retained; actual CI execution is pending.

Final all-scope `pnpm verify:affected` PASS (**687.922 s**, sampled RSS
**5,915,459,584 B**, cgroup peak **6,445,281,280 B** within the 8 GiB cap).
The static helper is **4,674,592 bytes**; workflow YAML parsing also passed.

## Milestone scope check

The authorized finish line is the v0.8.0 milestone and that version's release and
deployment. No subsequent product version is authorized. On 2026-09-10, live
milestone/PR/release records and changed paths were checked against #541/#546:
B01–B03 are merged, B04 remains draft, and the latest public release remains the
pre-task v0.7.0. WSL Git/worktree/LSP are explicit WP04 requirements.

Ancillary changes beyond Workspace's immediate feature files were reported to the
user: the CI-blocking Vitest advisory update, API/Knowledge async and lease fixtures,
and Knowledge's owned hot-journal recovery fix (a production change). These address
v0.8 integration verification; they do not introduce a later release or a new
product feature. Newly discovered work outside this milestone must be reported
before proceeding.

The actual Windows→WSL1 Source execution fixture at `3011ef9` passed in
**11.07 s** ([run 34424819336](https://github.com/jihoon22-lee/devbox/actions/runs/34424819336),
`workspace-wsl-source-execution.log`). It confirms selected stage/commit, approval
revocation after preparation, cancellation with a detached hook and retained
filesystem ownership through child retirement. The overall product run and the
later linked-worktree expansion remain separate pending acceptance.

## Native WSL worktree creation

Windows and Linux now share the existing retained-parent/absent-target worktree
owner. Linux adds distro-native filesystem admission. A native preview keeps the
exact helper connection alive until Windows consumes its own one-use review token;
creation revalidates the original Git approval and destination before/after command
admission. Repo Manager still owns the actual worktree mutation. No partial result
is automatically retried or removed.

Source registration proposals now retain their Windows/WSL target. WSL proposals
use the same distro with `startStopped: false`; registration and context selection
remain separate from creation. The expanded Windows fixture creates/registers a
linked worktree, approves it separately, stages/commits within that context and
checks the original checkout remains unchanged. A cancelled preview creates nothing.

Shared helper **50 tests**, Workspace/helper Linux strict Clippy and Workspace
frontend build passed (**36.468 s**, sampled RSS **1,618,538,496 B**). Actual Source
pipe **8 tests** and Registry **10 UI tests**, plus full Workspace MSVC all-target
strict Clippy passed (**25.868 s**, sampled RSS **2,058,334,208 B**). The real-pipe
cases cover one-use creation, wrong-token consumption and a concurrently created
destination during command approval. After extending the Windows fixture, MSVC
and helper strict Clippy passed again (**6.244 s**, sampled RSS **1,400,930,304 B**).
Final all-scope `pnpm verify:affected` passed (**480.077 s**, sampled process-group
RSS **7,801,643,008 B**, enforced cgroup memory peak **6,405,210,112 B**).
Actual Windows execution of the expanded linked-context fixture is pending.

## Remaining acceptance and rollback limits

- WSL template instantiation, reveal, Git worktree cleanup, native LSP and full WSL2 Rust host/WebView
  acceptance remain incomplete. Windows and WSL WorkspaceEdit must retain identical
  preview/identity/conflict/rollback boundaries before WSL apply is enabled.
- Legacy references/provider handoff, R24 Run Manager baseline/PTY/integration
  comparison and maximum read-only file/frame parity remain required.
- B05/B06 own process/service and terminal/session lifecycle; B07 owns secondary
  windows and product-wide provider integration. B08/B09 own suite migration,
  activation, installer recovery and exact-main stable promotion.

Reverting code does not delete original snapshots, imported destination data,
preimages or new edits. Native journal recovery may preserve partial results.
Cancellation is cooperative; no unrun Windows/manual or preservation-only step is
counted as PASS. This draft does not close #546/#541/#542 or claim release readiness.
