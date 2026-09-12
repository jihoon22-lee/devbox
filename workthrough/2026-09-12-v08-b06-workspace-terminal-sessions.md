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
