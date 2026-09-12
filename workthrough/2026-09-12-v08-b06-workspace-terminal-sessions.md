# v0.8 B06 — Terminal, WSL management, Sessions and Problems

Refs #548, #541, #542. One B06 PR owns the shared Terminal UI/native owner,
WSL/container surfaces, worktree sessions, Problems and settings import.
This worktree starts from B05 75b48c5 while B05 completes its final acceptance;
rebase only B06 commits onto the merged B05 main before publishing the B06 PR.

## Initial extraction

The original WSL Desktop UI, local APIs, CSS and tests move unchanged into
workspace-features/terminal. The standalone app reexports that same entry point.
The existing xterm dependencies move with their actual consumer; WebGL remains
lazy. Native transport/lifecycle changes are a separate semantic commit.
The pnpm lockfile changes only the existing xterm importer edges and the new
shared-feature dependency; no dependency version changes. The 61 moved files keep
their original bytes, with a one-line standalone reexport reviewed directly. Scope
fixtures include the new WSL Desktop consumer. No compiler/test run is repeated for
this mechanical commit while B05 uses the shared verification lock. Native wiring
and the complete B06 validation remain pending.

## Boundaries still to implement

- A native-created Workspace companion window owns Terminal rendering; the main
  Terminal route manages profiles/context. PTY lifetime survives hide and renderer
  detachment, with exact owner-window/session admission and bounded output replay.
- Running-distro and Docker management reuse existing collectors/actions. Logs
  handoffs remain fixed adapters; stopped queries must not start distributions.
- Sessions record created/borrowed/shared resources and Runtime operation/generation
  identity before starting work; stop/cancel/recovery preserve unowned resources.
- Problems use source/context/revision and existing file/run/log navigation checks.
- JSON terminal profiles and seven localStorage preferences/layout keys need separate
  preserved export/import paths. Live WebView databases are never edited in place;
  PTYs, buffers and broadcast armed state are not restored as executable state.
- Quick Summon registers with WP07's single shortcut owner; B06 adds no competing
  automatic global hotkey or additional public installer.

## Native Terminal component and PTY ownership

The WSL engine now has standalone/desktop features and strict component adapters.
Product initialization retains its selected directory identity, starts no PTY,
shortcut, tray or legacy snapshot writer, and keeps profiles in the product owner.
The unused arbitrary WSL command remains standalone-only.

A native-created TerminalOwner admits only its companion window and exact PTYs,
limits panes to 32 and parallel starts to two, preserves identical pane starts
across renderer reload, and scopes multiplexer names to companion identity.
Cancelled/failed starts remain visible as interrupted reservations. Product PTYs
have one native reader and a bounded 512-KiB/512-frame replay buffer; consumers
pull at most 64 KiB with sequence cursors and explicit loss/closed state. Product
output is never broadcast through legacy global events. Stop retains exact child
handles until exit and reader retirement are confirmed, allowing cleanup retry.
Fallible pipe acquisition now precedes child spawn to avoid an unowned child if
pipe setup fails. Existing standalone behavior still uses its original listeners.

The host companion factory, durable development-session journal, native context
admission, product UI transport, profiles/settings import and acceptance fixtures
are still to be connected; these adapters are not exposed as product IPC yet.
Minimal compilation (including test target syntax) passed in 5.245 seconds.
Replay regression fixtures are authored but intentionally await completed B06
implementation before execution. No detailed tests/build/Clippy/affected run was
added at this intermediate commit.

## Companion factory, renderer transport and private layouts

Workspace now lazily loads a Terminal manager and creates native-owned companion
windows with separate immutable SessionGuard/context bindings and capabilities.
The existing main-window guard still rejects companions; no route/URL parameter
grants authority. Main selection changes do not retarget an older companion.
Requests run in retained bounded workers with separate stop capacity. Close hides
a companion; product quit waits for its child/reader owners to retire. Durable
operation IDs are stored before window creation, and previous-process records
become interrupted without adopting old PTYs or replaying commands.

The product UI pulls sequenced output and waits for xterm's callback before asking
for more; missing replay bytes are shown explicitly. Detachment drops only that
subscriber. Initial-command receipts reserve before input and suppress repeated
mount writes. Layouts use native revisions and persist complete topology before
PTY creation. Reload hydrates the native layout and reconnects existing pane keys;
failed slots remain visible. Product localStorage keys are scoped to installation
and, for layouts, companion session. Native profile envelopes retain import receipt
space and reject stale concurrent saves/corrupt/future state; ordinary edits never
publish legacy snapshots.

Shared UI and Workspace TypeScript checks plus native all-target compilation passed.
The first new consumer graph compiled dependencies (139.298 seconds total); later
layout/profile integration reused it (21.211 seconds total, Rust 6.57 seconds).
No detailed tests/Clippy/app build/affected execution was started. New replay and
window-principal fixtures are prepared for the complete B06 verification packet.
The final legacy-only TermPane branch was inspected to preserve its existing mock
and direct-write behavior. Windows window/PTY behavior remains unexecuted.

Still pending: retained project/distro identity through PTY launch, explicit failed
pane retry/crash restore UX, Docker mutation identity/Logs consumers, shell updates,
single-owner summon integration, JSON/WebView migration, full Development Session
resource leases/phase coordinator, Problems and real Windows/WSL acceptance.

## Retained native launch admission

Terminal now reserves a pane before acquiring its native project/distro launch
lease. The lease retains Registry/root/backing/executable handles; all multiplexer
environment/version/session probes and PTY launch use the same GUID selector and
system WSL executable, with identity rechecks before/after probes and spawn. The
WSL project helper remains owned through PTY lifetime and explicit retirement.
A failed/cancelled start retains its lease in the failed pane until explicit
cleanup/retry. Confirmed PTY retirement also retires the helper before publishing
closed output. The Retry action clears only a retired failed pane; ordinary reload
never clears an interrupted reservation.

Shared TypeScript and portable Workspace all-target compilation passed in 17.905
seconds. Its unused-field warning exposed a missing final lease-retirement call;
the call was added and source/format checked. Windows-only launch admission awaits
B06 final Windows compilation/real execution. No detailed verification was added.

## Development Session transition/reference model

The pure session store separates preflight/review/restore/prepare/start/readiness,
active/stop/degraded states, reviewed plan revisions and phase deadlines. Native
resource acquisition has durable operation identity before effects, exact generation
references, and explicit created/borrowed/shared ownership. A creator's stop is
deferred while other sessions hold a service; external resources are never stop
candidates. Stop submission IDs are reserved separately, late starts remain owned
through compensation, and crash history cannot authorize reexecution. Five concrete
scenario fixtures cover two worktrees, external/stale resources, late cancellation,
crash/replay and restore-only/expired plans. Native Runtime lease/controller and UI
consumers are still pending; these pure records alone grant no process authority.

Terminal also preserves nonproject usage: companions may have no ProjectContext,
while retaining their exact native window/distro/PTY ownership. Project-bound
companions continue to use their immutable Registry context. The manager displays
project names and working folders, and its refresh no longer duplicates a list call.
Workspace TypeScript and portable all-target compilation passed (43.669 seconds).
The final small manager-label/refresh edits were source reviewed. Detailed scenario
tests and Windows-only nullable-context admission remain in the final B06 packet.

## Native Runtime leases and cancellation admission

Development Sessions now have a native-only Runtime facade for reviewed definitions,
exact job/DAG run references and atomic service create-or-borrow. Review digests omit
scheduler bookkeeping while retaining execution configuration/source revision. The
claim transaction rejects changed definitions and existing execution conflicts;
Session starts cannot inherit a global job's kill-previous/queue behavior. Once
reviewed, a second database read cannot substitute a changed command before spawn.

The actor creates a native start witness before submitting work. That witness retains
the allocated run/service generation even if later storage/receipt publication fails.
Renderer JSON and durable receipts cannot reconstruct the lease, and borrowed service
leases cannot stop the creator's resource. Private Session receipt method names are
stored by the owner without expanding the generic renderer Runtime control allowlist.

A start waiting for global capacity now publishes a cancellable native reservation.
Stop still serializes through the job mutex but observes start publication and wakes
only the matching run/generation; unrelated capacity holders are untouched. Starts
already crossing the adapter boundary remain owned until the ordinary exact stop
can confirm cleanup. DAG children attach their durable run before waiting for capacity,
so operation cancellation can find that exact pending child. Failed stops require a
new receipt only after the retained prior worker has settled; old receipt IDs and late
results cannot retire a replacement attempt.

Regression scenarios were authored for shared-service acquisition, exact generation
and queued-run cancellation, foreign-capacity waits, global kill-previous refusal and
failed-stop retry. They are reserved for the completed B06 PR packet. The minimum
Workspace all-target syntax/type check passed in 27.764 seconds; no B06 tests, Clippy,
application build, affected run or actual Windows acceptance was executed. The facade
still needs its Development Session controller/UI consumer; this is not WP06 completion.

## Development Session controller and review surface

The Terminal route now includes a native Development Session controller and review
surface. It lists bounded Runtime candidates, previews selected command/target/cwd
and environment presence, then separates restore-only from reviewed execution. Each
session retains an immutable project/worktree context and plan revision. Requests
persist intent/phase/operation identity before native effects; renderer loss does
not stop the accepted worker. Stop releases borrowed/shared references and targets
only retained creator generations, with separate retry receipts. Shutdown waits for
session workers before completing owner retirement; restart leaves old sessions in
review-required history without native lease adoption.

Resource publication is synchronous with Runtime claim admission. A Session must
persist its reference before a service borrow returns or a new process starts.
This closes the race between one session releasing a shared service and another
acquiring it. Failed publication retains the native witness for reconciliation;
a rejected borrowed reference creates no process and can settle without resource
ownership. DAG operation identity is published before its executor starts. Readiness
reuses Runtime's exact-generation process/TCP health check.

The first native compile found an incorrect operation-view module import; it was
fixed and the same syntax/type check resumed. Workspace native all-target check and
frontend typecheck passed in 26.359 seconds. UI fixtures were authored for explicit
restore-only approval and rejecting a late previous-worktree plan; they have not
been executed. No detailed B06 suite was run. Remaining B06 integration includes
Terminal profile/PTY leases in the development session, fuller environment/port
preflight and Problems/Knowledge providers, WSL/container/Logs actions and the
JSON/WebView settings importers. These are required before the PR validation packet.

Start admission now also has a sticky cancellation signal before the per-job lock.
A session cancelled while another request holds that lock allocates no run. Phase
timeout requests cleanup and retains an already-started native future until it
settles; it does not drop a future across process creation. Readiness waits for the
DAG root's actual run rather than its pre-spawn attachment record. Borrow publication
rejected during shared-resource release settles without taking ownership. The added
cancellation fixture remains deferred. A callback alias lifetime typo was corrected;
the final native/frontend syntax pass took 19.506 seconds without test execution.

## Session Terminal restoration and completed history

A Development Session can select a product Terminal profile. The native plan binds
its validated profile/store revision, writes the complete copied layout before
window creation, and retains an exact companion lease through late publication
failure. Readiness checks the saved pane keys' native PTYs, including failed/closed
slots, instead of treating a visible window as restored. Restore-only strips start
commands from the copied layout while preserving the original profile. Reviewed
execution retains the legacy companion's explicit start-command confirmation; the
review surface explains that separate confirmation. Stop retires only the exact
retained peer and its PTYs, including restoration completing after cancellation.
Application exit requests Session cancellation before waiting on other workers.

Completed Session details can be archived only after owned resources fully retire;
a creator with an outstanding shared service stays retained. UUID tombstones reject
old prepare replay, and unreferenced operation/resource detail is released. Terminal
window receipts retain up to 4,096 identities, with all live windows and recent
history bounded to 64 displayed records; the live companion limit remains eight.
Opening a window validates its Registry binding; the actual launch factory performs
and retains filesystem/distro admission, so an explicit Terminal start can reach a
stopped WSL target without using a read-only project probe as a hidden launch gate.

The native/frontend syntax check passed in 16.853 seconds. Subsequent source review
added reconciliation of a retained Terminal lease after metadata publication failure
and an unexecuted restore-only Terminal ownership fixture. Detailed restore, migration
and actual Windows/WSL cases remain in the final B06 acceptance packet.

## WSL/Containers and one-time Logs connection

Runtime now reuses the existing WSL distro/resource/Docker panel and collector,
without importing xterm or starting a PTY. It retains the last complete generation,
marks stale/error state and stops automatic refresh when its route is hidden. The
explicit distro terminal action copies a one-pane native layout before companion
creation. Docker start/stop/restart retain a native distro/executable lease, require
fresh full container identity, and record a bounded durable operation receipt before
the external effect. Ambiguous renderer retries retain the same receipt. Runtime
container stop uses this owner after endpoint revalidation; it never substitutes
an external PID kill. Published container IDs are no longer truncated.

Terminal and Runtime WSL file/journal requests enter a bounded expiring native queue.
Only a wake-up ID is emitted; authenticated main reads the matching context's source
and acknowledges consumption. Reload can consume the still-pending request. Paths
are neither command-line handoffs nor persistent saved-view values. The Logs slice
accepts the existing fixed WSL adapters and preserves source limits. Main WSL
management grants no raw PTY IO. Live input/output/resize have a separate bounded
request/worker pool from restoration and slow distro queries; shutdown accounts
for both pools and rechecks retirement after owner cleanup.

Minimal native syntax/type compilation passed (8.64 seconds). Frontend checking
found one return type widened unnecessarily by the WSL adapter union; retaining the
Runtime-specific discriminant fixed that error. The resumed frontend-only check
passed in 9.698 seconds; Rust was not repeated. Late-context/path/extra-field and
queue-saturation/authority fixtures were prepared for final B06 execution. Shell
integration, migration, preflight and Unified Problems remain in progress.

## Native shell-integration admission

The existing Bash/Zsh preview and update flow now accepts an explicit native
execution binding. Workspace companion requests retain the running distro's GUID,
registration/backing identity and native wsl.exe through every environment probe,
read, backup and reviewed mutation. The existing marker-aware no-op behavior,
expected content revision, second pre-replacement read and backup naming remain
unchanged. Read-only management capture refuses a stopped target and checks its
running state again when binding each command; explicit PTY creation still permits
a stopped target. A rejected-binding fixture is prepared without spawning a shell.

The minimal native check first found a Tauri State/reference conversion error;
a direct inner reference fixed it. The resumed native check passed in 8.401 seconds.
No frontend or detailed test suite was repeated. Actual Windows shell mutation and
backup/marker acceptance remain part of the completed B06 packet.

## Terminal migration and shared exporter preparation (in progress)

The actual second copied-profile consumer moves API Studio's closed LevelDB copier
and scratch cleanup, with their fixtures, into data-migration; the WebView2 actual
profile check moves into an optional product-shell-tauri feature. MCP stdio/native
export child ownership moves into the process crate's optional owned feature.
Existing API Studio and API Playground call paths reuse those implementations.
There are no new external package versions; Cargo lock changes are dependency edges.

Workspace's disposable exporter reads only seven inventoried localStorage keys after
actual copied-profile identity verification. A fixed legacy JSON read and browser
snapshot prepare a Terminal-owned review. Last layout becomes an explicit profile;
unsupported/missing keys remain individual notices. Native preferences now share the
profile envelope, allowing definitions/preferences/import receipt to commit together.
Companions preload these values and use per-key compare-and-set writes. The earlier
product-local preference namespace is adopted only for absent native keys.
Original profile IDs map to deterministic new IDs. Repeated apply cannot resurrect
later-deleted profiles or overwrite later preference edits. Exact preimages are
preserved before replacement. Import jobs retain cancellation/interruption states;
worker children retire before their owned browser copy can be cleaned.

The first minimal check across both real consumers took 54.083 seconds and found
calls to an assumed MetadataRoot::create_new method. The existing preserve method
was idempotent, so a strict create-new variant was added using the same atomic
hard-link write; exporter replay claims require strict creation. The resumed check
keeps the same Cargo feature graph. No detailed tests were run. Import cleanup
recovery, explicit history restoration, window migration integration and final
migration acceptance still need completion before this section is final evidence.

The extraction/native/frontend minimal packet now passes (20.525 seconds on the
final resume). Intermediate resumes corrected an orphaned attribute left by the
module move and the existing snapshot fixture's new WSL Desktop enum branch; one
attempt edited the wrong file and repeated that same 11-second compiler failure.
Those attempts are failures, not additional coverage. The Cargo graph was retained
through all resumes. Existing API Studio checked artifacts were reused. No tests,
Clippy, bundle build, affected run or Windows execution were added here. The shared
moves and dependency edges are committed separately from the Terminal consumer.

## Reviewed import recovery

Terminal import history now exposes the verified preimage's profile/preference
counts, binds both the current owner revision and exact saved preimage, and restores
only after a separate user action with companion windows closed. The current bytes
are preserved first. Import receipts survive restoration, so replay of an old source
still cannot recreate deleted profiles. Corrupt or changed preimages remain unavailable.

Copy cleanup has an explicit retry. The acquisition callback records the new scratch
root's native identity before copying; cleanup compares that identity and requires
worker retirement when a worker ticket exists. Replaced roots and unconfirmed worker
retirement preserve the copy. This extends the shared cleanup primitive without
changing API Studio's existing call behavior. A cleanup authority fixture and an
import/restore/repeat fixture are prepared for final B06 execution. Settings now
accept only the existing finite enum choices instead of silently normalizing unknown
values. A missing legacy data root has its own report code.

The changed native/frontend syntax packet passed in 20.106 seconds. No detailed
B06 tests or Windows build was run. Window-state migration is now connected through
the existing Overview snapshot/review importer, explicitly mapping the legacy WSL
Desktop main window to the Workspace main window. B05's separate final-artifact
WSL2 fixture runs only after this local packet has retired.

## B05 integration checkpoint

B05 merged as 41bb98b after final CI, packaged product acceptance and a focused
owned-local WSL2 pass. This branch rebased its thirteen commits onto that exact main
without conflicts. Passing B06 syntax evidence remains retained; the next check is
for the next implementation chunk, not for the rebase. B05's final merge/cleanup
record is appended to its existing workthrough with this dependent bundle.
