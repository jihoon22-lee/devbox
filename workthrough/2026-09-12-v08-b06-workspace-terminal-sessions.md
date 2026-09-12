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
