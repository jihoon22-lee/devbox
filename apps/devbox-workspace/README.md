# Devbox Workspace

Hidden v0.8 B04 development product. The shared Overview/Source/Files UI and native component adapters are being integrated; product authority, registry, migration and LSP acceptance remain incomplete. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-workspace dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-workspace tauri dev` runs the native shell. Browser `?route=overview` selects a preview route. The Windows debug executable accepts `--route=overview` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.workspace` identity; startup does not open legacy data roots. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof remain separate from the focused Registry/storage checks.

Overview now provides explicit backup of fixed v0.7 Workbench profile/template and
Code Pad session/recovery/LSP JSON files and each legacy product’s main-window state. A bounded native background job validates
schemas and records, checks source identity/content again, and preserves exact
bytes under a content-addressed product-local snapshot. Unknown/future/corrupt
files have diagnostics without success counts. Cancellation leaves incomplete
snapshots uncommitted; retries verify existing bytes without overwriting changed
files. This step does not activate imported data or grant project/file/LSP access.
The persisted catalog is available after restart. Explicit revalidation checks all
saved bytes without reopening the legacy source; completion metadata alone is
not an integrity check. Incomplete or damaged entries and unknown folders remain
preserved. Verified Workbench profiles can be reviewed and imported with explicit new/reuse/
keep-both/skip choices. Their original IDs and complete metadata remain alongside
new Workspace IDs. A separate native Windows folder review commits registration
and profile binding together; imported ports are defaults below project/local
values. Unlink preserves both records. Files now reviews a verified Code Pad
session for the current project/native file choices and preserves original IDs,
cursor/bookmarks and both views. Closed tabs, a current native revision and an
explicit replacement choice protect existing state. Session and import receipt
commit together; repeated imports retain subsequent edits. A checked, bounded
history preserves the exact previous bytes before replacement and provides
explicit restoration. Editor autosave carries native revisions and stops on
conflict without discarding buffers. Imported paths still use normal Files
authorization when reopened. Verified recovery buffers now merge into the same
view's real recovery store. Explicit conflict review replaces only matching paths;
other buffers remain, and capacity overflow rejects the entire merge. Original
hash/time/content and import receipts survive normal writes and discards. Previous
raw metadata is preserved before commit and can be reviewed/restored. Hosted saves
and discards carry native revisions; the dialog chains them while preserving
unavailable entries and backups on conflict. Import itself never writes a source
file; applying its normal recovery preview remains a separate action.
Verified LSP settings now have a separate current-project before/after review.
Import and history restoration preserve server/runtime preferences while retargeting
the current root and forcing LSP disabled. Closed tabs and the native context write
permit prevent editor replacement or overlapping settings writes/starts. Receipts
survive normal settings saves; repeat import preserves later changes. Exact
preimages support checked history restoration, including offline roots. No command
is resolved, runtime installed or execution approval inherited by conversion.
Overview reads the single last workspace from a verified Code Pad session without
resolving its path. Explicit Windows folder review uses the native snapshot job ID
and existing Registry discovery/registration; it grants no selection or execution
trust. WSL/unsupported paths remain visible without automatic probes. WSL profile binding now requires its own native distro/root review. Repo Manager's
scan root and selection are transient React state, with no persistent preference
file to migrate. The explicit Code Pad (older version) source reads `com.workbench.codepad` independently
from `com.devbox.codepad`. Both fixed roots can coexist; source identity remains part
of the snapshot ID even for identical contents. Session/recovery/LSP/window review
uses the same verified-job boundaries, without renaming or merging the original roots.

The existing Workbench, Repo Manager and Code Pad UI/tests live in
`packages/workspace-features` and are consumed by their legacy entry points too.
Workspace lazily loads the three slices and retains visited routes. Other product
routes explicitly remain unavailable. The native engines expose typed component
adapters and opt out of standalone bootstrap/web assets for product consumers.
Native-only initialization selects immutable per-process data/snapshot/handoff/cache
roots; it neither invokes old identifier migration nor starts a language server.
The hosted LSP manager preserves existing rename journals during initialization
and denies execution before project path resolution. Server preparation now
separates path/argv inspection from runtime probes; native approval hooks cover
probes, server starts and automatic retries. Workspace waits for confirmed LSP
shutdown before exiting. Windows catalog install, explicit installed-index recovery,
removal and verified local archive/cache import now use an independent LSP worker
queue. Native picker choices are expiring single-use tokens with no editor grant;
selected source objects are checked before bounded private snapshots reach the
existing digest/dependency-lock verifier. Product shutdown cancels downloads and
waits for active LSP workers to retire.
Native lifecycle commands now use per-context execution review and an owned LSP
thread, including automatic retries. Review pins saved configuration, exact argv,
filtered PATH, native Windows SystemRoot, executable/runtime files and project execution definitions. Starts
retain context/filesystem/installation permits through initialization; installation
waits use their own lock. Cancellation retains probes and initializing children until
termination is confirmed. Configuration writes, revocation and context retirement
stop the owner before mutation. Windows Files now connects explicit execution review,
manual start/stop and document/WorkspaceEdit commands. Opening a document requires
an existing native Files grant and revision; save/reload additionally verify current
disk bytes and encoding. A native hash mirror includes every opened editor buffer,
even unsupported languages, and rejects dirty rename targets before preview/apply
and rollback. Successful atomic rename refreshes native grants and all affected open
buffers. Events are filtered by native project/worktree/revision/target.
Explicit journal recovery lists only private context metadata until a user requests
review. The native review pins current project authority, every target/backup and
an expiring one-time token; it works with disabled, missing or corrupt LSP settings.
Target editor tabs must be closed. Apply holds the filesystem write permit, rechecks
all files before the first replacement, and checks authority again just before each
atomic restore. Partial failure preserves the journal and backups for another review;
completed cleanup removes only known unchanged files. WSL-native transport remains
incomplete; full packaged Windows LSP/journal recovery acceptance is pending.
LSP settings now load and save separately for each registered context without
accessing its project files or resolving a server. Native revisions prevent stale
writes; explicit corrupt-file recovery preserves the original bytes, and future
schemas remain untouched. Offline project settings remain available.
The shared LSP engine retains native document authority across cloned sessions,
rename previews, disk apply and rollback. It checks incoming paths before resolution
and their canonical targets afterward. Revoked access preserves pending recovery
journals; execution approval alone defaults to no document access.
The product host now authenticates explicit startup and Registry commands. Its
native startup screen creates a blank generation only after a user action and
shows schema/owner failures without replacing existing data. Registry UI supports
native folder preview/cancel, explicit registration/rebind, rename and reviewed
metadata removal. Registration grants no execution trust. Blocking probes use a
separate bounded worker pool and cannot consume all metadata workers.

Explicit project selection rechecks native root/Git identity and binds the exact
Registry context to the product session. Stale revisions and replaced roots fail
without changing Registry bytes. Clearing selection grants no execution authority.
The shell refresh preserves the mounted feature subtree.

On Windows, Files now mounts the existing CodeMirror editor after native store
activation. A selected project or native file dialog admits file access; save,
rename and delete require an unchanged native document revision and filesystem
snapshot. Tabs remain mounted across routes, and dirty/busy/recovery states block
project switching. Session and recovery metadata are private to each context.
Recovery uses explicit native preview/apply tokens and preserves cancelled entries.

WSL large-file reads retain Code Pad's 5-MiB editable and 64-MiB read-only limits.
Large decoded text crosses the private helper pipe in bounded, ordered chunks;
only complete text with a matching digest becomes an acknowledged file. Context,
file replacement, expiry and replay checks remain active throughout the read.
The Linux subprocess fixture covers the exact 64-MiB boundary with worst-case
JSON escaping and Korean text; Windows host and WebView observations are recorded
separately in the B04 workthrough.

Project settings now inspect manifest/local-overlay definitions and their native
source snapshots, with explicit one-time approval, cancellation and revocation.
Changes to source files invalidate approval without changing editor context.

Project/local JSON edits and imports show native before/after definitions and
require explicit save. Concurrent changes preserve the original file and editor
draft. Saving revokes execution approval; shared export includes only the manifest.

Dependencies now lazily mounts the shared local parser and review UI for the
selected native project, including plain folders without Git. Analysis does not
launch package managers or Git. Remote OSV/deps.dev requests consume a one-time
context/input-bound review, with cancellation and a 30-second request budget.
Summary/cache writes use the private generation; corrupt/future cache files are
preserved, and a missing generation is never recreated by cache publication.

Source now offers separate native Git approval and the shared Git panels. It
pins the launcher/core executable, config/include/hook evidence and native
worktree arguments before execution. Host startup captures one private native
environment for Source; every review and Git child uses that same snapshot through
`env_clear`, so later UI/COM environment changes cannot change approved execution.
Restart Workspace to adopt a new process environment. Changed execution files and
project definitions still require review. Git and
editor disk operations coordinate while private recovery writes remain usable.
Commit drafts survive permission refreshes and block project switching. Generic
file editing cannot rewrite the products' native Registry/approval stores.

Source changes and diff lines open the retained Files editor. Worktree creation
uses a one-time native target review, then proposes Registry registration and
selection separately. Cancellation begins before queueing/evidence capture, and
local drive aliases are resolved before filesystem IO. Cleanup has a separate native review of explicitly selected registered siblings.
Its preview/apply checks preserve existing safety rules and block folders with
open native documents. Revocation works without contacting a sibling. Legacy
import, complete Windows LSP integration and WSL-native Git/LSP remain incomplete. The basic Source/worktree, Dependencies and Files packaged flows passed Windows
acceptance at `833854a`; broader cleanup and actual dialog acceptance are pending.
The `4139ea9` Windows run failed at concurrent history detail/diff loading before
reaching those new checks. Selected-store readers now share a read lock while
activation remains exclusive. History preserves the native bridge's fixed error
message. The `d612d4a` general CI passed, while its Windows fixture passed history
but timed out waiting for a later native request. The fixture now allows the native
29-second request budget and records bounded method/issue codes, duration and owned
dialog driver failures. New installer/cache and multi-file picker acceptance is
pending; local Windows disposable dialog cancel/single/multi selection passed.

Workbench template imports reuse the original strict schema and preserve empty-path
presets, old IDs and service/port defaults as separate Registry records.
The original bytes remain in the immutable legacy snapshot.
Reviewed choices add, reuse or keep conflicting copies without replacing v0.8 data.
The project form can instantiate an imported template against an explicitly checked
Windows folder; registration, a fresh profile with its template provenance and its
folder binding share the Registry commit. Environment data is absent and selection
and execution trust remain separate. Opening template management lazily loads its
editor and validation code. Overview can create, edit and archive templates,
then explicitly restore archived defaults. Edits use the opening Registry revision
and preserve the draft on conflict. Existing profiles and bindings remain unchanged;
repeat import from the same snapshot preserves destination edits and archive state.
New templates use native IDs and an explicit local origin without an invented
snapshot ID. Prior imported record JSON remains compatible; older parsers reject
new local/archive fields while preserving the store. The hosted original import and
creation UI passed at `870f8a8`; expanded editing acceptance and WSL instantiation
remain pending.

Verified main-window state from Workbench, Code Pad and Repo Manager now has an
explicit current/adjusted review using the existing monitor/DPI restoration logic.
The snapshot inventory is versioned: original v1 snapshot IDs and bytes remain
readable; new v2 snapshots include the fixed window file. Window import passes only
a native job/review ID. It preserves current native geometry in bounded history
before applying and rejects changed geometry/monitors, expired tokens and replay.
Previous states have the same explicit restoration review. Native geometry runs on
the UI thread and uses the existing coalescing window writer. A partial OS failure
can leave intermediate geometry; the saved previous state remains available.
This is not a transaction between window APIs and disk persistence. Only the
compatible main-to-main mapping is implemented; secondary-window roles belong to
B07. The hosted window UI/native fixture passed at `161d2af`, including explicit
apply, cancellation/replay rejection and restoration of previous native geometry.

The [B04 workthrough](../../workthrough/2026-09-08-v08-b04-workspace-source-files.md)
records actual extraction/build tests and the remaining Windows/WSL acceptance.

The private [WSL helper](native/README.md) now reuses root/Git filesystem
observations and has a bounded Windows pipe client. Its static ELF resource is
built in the product workflow from the same source SHA, checked before packaging,
and checked against the compiled digest before native launch. The client binds a
registered GUID/backing-directory and WSL2 backing-image objects, requires explicit stopped-distro start,
and retires the helper on EOF/timeouts. Actual local Windows-to-WSL pipe
and installed-resource-path probes passed. At `01db848`, the packaged Rust client
and hosted WSL1 Registry/Files/reconnect fixture also passed. A lazy WSL folder form now lists registered
GUIDs without startup, requires an explicit stopped-distro start choice, and uses
the existing native registration/rebind review and byte CAS. Late UI results are
cancelled. Selected WSL Files operations now have a separate native admission
path; Git and language-server delegation remain incomplete. Resource-file and ancestor leases prevent launch-path
substitution. Backing-image checks request metadata only, without reading disk
contents.

Saving an LSP document now carries the exact successfully saved text and native
revision. A delayed change from the previous disk revision cannot leave the
server at the old snapshot. The host checks the saved bytes against current disk;
newer unsaved editor text remains dirty and is synchronized after didSave.

The Windows host and Linux helper now share the file grant/conflict implementation.
The helper can attach an observed native project context, list/read files, retain
buffer metadata, preview Markdown/Mermaid and explicitly save/rename/delete against
its current revision and disk snapshot. Attached roots survive idle time and still
revalidate native objects on every operation.
Cancellation rechecks precede atomic replacement; owned staging files retain native
parent/file identities. Unacknowledged saves require reconciliation and are never
replayed automatically. It restricts file content to the selected distro's root filesystem;
Windows aliases and other mounts are not admitted through a POSIX spelling. The
WSL Files route delegates to the helper; language servers remain
unconnected. LSP text updates
now stay in the server document owner: NativeEditorMirror alone acknowledges UI
buffer hashes, so an older queued notification cannot clear a newer unsaved buffer.

Windows Files and the helper share guarded directory traversal and local preview
image admission. Rejected directories are not traversed, changed roots and expired
requests abort publication, and bounded output reports incomplete/truncated scans.
Linux rename uses one non-overwriting namespace syscall where supported. WSL1
instead publishes a hard link and rechecks authority/snapshots before removing
the old name. Interruption can preserve both names and requires reconciliation;
it never overwrites a concurrently-created destination or replays the move.

WSL Files keeps helper-owned document revisions beside private Windows session and
recovery metadata. Windows native chooser grants remain independent of POSIX paths.
The selected context is revalidated for file IO; metadata persistence accepts only
acknowledged documents. Five-second polling observes external leaf replacement
without changing the dirty buffer or save revision. Events and delayed tab cleanup
carry their original context, so equal POSIX names in another distro do not match.
Reviewed buffer recovery uses the helper's native encoding/snapshot and guarded
atomic save. User request deadlines bound the remaining pipe request. The app waits
for confirmed helper retirement on exit; failed retirement retains the owner.
Explicit WSL reconnection confirms retirement, observes the same registered root
again without automatically starting a stopped distro, and invalidates all old
file revisions. Session/recovery eligibility survives missing files. The editor
reopens files one at a time, preserves dirty text when disk bytes match, and sends
changed dirty files to external-change review. Failed reads retain buffers; saves
are never replayed. Late watcher reads from a retired connection cannot replace
the reconciled buffer. The reconnect button remains available when offline root
capability checks fail.
The `01db848` general and Windows product workflows passed: native Workspace
141 tests, actual WSL1 Rust/helper, full packaged shell/journal/context clear,
API Studio/Knowledge and installer coexistence. Registry mode detection uses the
VM flag independently of filesystem format. WSL1's missing statx/fdinfo use native
file-ID and mountinfo evidence while other filesystems retain the stricter checks.
Local Windows-to-WSL2 file pipe checks also passed; actual WSL2 Rust host/WebView
and the complete WSL Git/LSP workflow remain separate acceptance work.

WSL project definitions now reuse the Windows schema and bounded native snapshot
logic. The Linux helper owns manifest/source reads, exact-byte revalidation and
reviewed writes. The Windows generation owns local overlays and one-use Registry
trust. First creation never overwrites a concurrent file; changed source contents,
parent replacement and cancellation reject publication. Existing manifest edits
preserve encoding/CRLF through the shared Code Pad writer. Trust is revoked before
writing, and an unacknowledged write requires reopening rather than replay.
The host excludes concurrent Git/editor writes during definition operations.
Actual local WSL1/WSL2 pipe checks passed for definition creation/read/change/replay
boundaries. The expanded Windows Registry/overlay fixture passed at `90f919f`.
WSL LSP, references/providers and remaining R24 acceptance are incomplete.

File saves and reviewed definition writes wait for active filesystem readers before
entering their native worker, within the original deadline. Waiting holds no Files
mutex and never retries an executed operation. Expiry/shutdown rejects the wait;
the original context and snapshot checks still precede IO. This addresses the
`5571786` packaged save rejection during an LSP read; the forced-overlap packaged Windows
fixture passed at `90f919f`.

WSL Source review now reads Git tools/config/includes/hooks in the native helper,
shares Windows' bounded evidence checks and binds their digests to the Windows
private approval plus project definitions. Reading/approving runs no Git. Changed
sources invalidate approval, and transient launcher variables do not invalidate
an unchanged review on reconnect. The hosted WSL1
review/approval/revoke fixture passed at `90f919f`, as did the expanded definition
fixture and packaged file-save/LSP-read overlap checks.

Basic WSL Source status/diff/history/stage/commit/remote operations now use the
native Git owner. Every command rechecks Windows approval and native Git evidence;
cancellation retains the context and file-write exclusion until native descendants
retire. The static helper reuses Repo Manager's existing execution policies without
Tauri. Linux actual-pipe tests, Windows compilation and actual Windows WSL1
Source execution passed at `3011ef9` (11.07 seconds). WSL worktree creation now retains a one-use native destination review and offers
registration in the same distro. Its Linux execution and Windows compilation passed;
the expanded Windows host fixture is pending. WSL cleanup now admits explicitly
reviewed registered siblings through native repository/evidence checks, blocks open
Editor documents and observes scope revocation on every Git command. Its Linux
actual-pipe tests passed; the expanded Windows cleanup fixture remains pending.
WSL Dependencies now analyzes native lockfiles without Windows path fallback,
using the same parser and keeping summary/cache/remote approval on Windows. Linux
regressions and Windows compilation passed; the expanded host fixture is pending.
WSL LSP still requires its native adapter.

Explicit Explorer reveal now revalidates the currently opened WSL document through
the native helper. Only its exact acknowledged path is mapped to the retained
distribution's `wsl.localhost` share; `/mnt/c` never becomes a Windows drive alias.
Windows-incompatible Linux names remain editable but reveal returns an explicit
unavailable message. Neither the mapping nor Explorer supplies file IO authority.
The native grant and current Registry/distro binding are checked around the action.
Actual Linux admission tests passed; Windows host and Explorer UI evidence is pending.

The `fd48dd3` Windows WSL fixture found a missing worktree-preview method in the
Windows connection gate. The corrected gate is shared with actual pipe fixtures;
the next run (`1d75608`) passed Dependencies assertions and Registry/approval checks
but failed a later Source execution with `source_operation_unavailable`. The shared
fixture needs per-operation diagnostics before that failure can be located.


WSL onboarding can instantiate a saved template against an explicitly selected
distribution and Linux folder. The reviewed native name/path replace only the new
profile's selected WSL target; template defaults/provenance remain preserved.
Registration, the fresh profile and binding commit together with Registry CAS.
Template edits/archive invalidate a pending review. Cancellation, replay and stale
reviews never insert profiles or grant execution trust.

Imported WSL profiles can separately review their saved Linux folder in the current
matching distribution (ASCII name case is ignored, Linux path case is preserved).
Missing distributions remain unavailable; stopped ones need the existing explicit
start checkbox. The original profile and snapshot stay unchanged. Unlink/relink
preserves both profile and Registry objects. Local tests and MSVC checks passed;
the expanded actual Windows fixture is pending.

The Source execution fixture now records only its synthetic operation ID/method,
elapsed time and fixed issue code, so subsequent Windows failures identify the
operation without publishing paths, Git output or environment values.


The `e3f05c8` Windows fixture passed WSL templates/profile binding, Dependencies,
worktree creation/registration/linked stage and commit, reveal callback admission,
cleanup protection/revocation and successful cleanup. Its final main-worktree
`cancel-stage` failed before testing hook cancellation. The same sequence reproduced
on a dedicated local Windows/WSL1 Ubuntu fixture. A focused Linux pipe regression
identified the shared status parser rejecting Git's `nested-worktree/` directory
placeholder. Those entries are now display-only while ordinary selected-file stage
continues. Linux pipe/parser/UI tests and the complete actual Windows Rust host scenario passed
after the fix on owned WSL1 (71.37 s) and WSL2 (53.10 s), including final hook
cancellation and fixture cleanup. Packaged/WebView and LSP acceptance remain separate.
