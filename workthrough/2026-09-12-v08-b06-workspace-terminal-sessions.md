# v0.8 B06 — Terminal, WSL management, Sessions and Problems

Refs #548, #541, #542; dependent consumers of #547 and #545. One B06 PR owns
Terminal extraction, native ownership, worktree Sessions, Problems, importers and
acceptance. Base is merged B05 main 41bb98b. No umbrella issue closes here.
Implementation, fixtures and all-scope local checks are complete; final CI and native Windows/WSL acceptance are pending.

## Scope and final behavior

The original 61 WSL Desktop frontend files were extracted byte-for-byte into
packages/workspace-features/src/terminal, with legacy reexports. Semantic changes
are separate commits. Existing xterm dependencies moved with the consumer; WebGL
and the terminal surface remain lazy.

The primary Terminal route manages profiles, context and Workspace-owned companion
windows. Native window identity and a fresh SessionGuard authorize exact PTYs;
renderer-provided labels/PIDs cannot grant ownership. Each companion retains its
immutable project/worktree/target, up to 32 panes and two concurrent starts.
Active panes restore first, failed slots retain topology, and native initial-command
receipts prevent a lost response from resending input. One native output reader
retains 512 KiB/512 frames with 64 KiB batches, sequence/gap/closed state and bounded
pull queues. Hide, unmount and reload preserve PTYs; actual stop waits for reader
and child retirement. Input/output, stop and slow metadata use separate workers.

Stopped/interrupted windows explicitly reconnect with the same durable window ID
and scoped tmux/zellij name but a new native owner/handshake/PTY. A generation and
operation CAS makes retries observation-only, including failed attempts. Window
destruction completes before reuse. This state-only window preserves start-command
definitions while native and renderer guards prevent their transmission. It does
not adopt a stale PID or implicitly kill an external multiplexer session.

Terminal profile/layout/preferences use native owner-local metadata and revisions.
Companions preload preferences and save each key against its previous value.
Settings describe the product's actual close-to-tray behavior. The native command
catalog exposes bounded profile IDs/names/revisions and active window identities,
never command text. Summon resolves show/focus/hide before its expiring receipt.
B07 owns the single global shortcut registration and authenticated external delivery.

Sessions persist reviewed intents and resource operation IDs before launching,
retain exact Runtime generation witnesses, and distinguish created/borrowed/shared.
Creator cleanup waits for the last shared holder; external resources remain owned
by their original caller. Cancel/partial failure retain pending witnesses until
retirement. Restart does not interpret interrupted intent as execution approval.
Stopped history can be archived only without held/pending resources; bounded
tombstones prevent old operation reuse.

Preflight reports native root/distro, definitions, task trust, environment presence,
declared toolchains and observed port conflicts. Unknown observations are not free
ports. Start rechecks the reviewed revisions/blockers. Restore-only skips task
execution validation and restores state without executing profile commands.
Profile hashing covers the selected profile rather than unrelated preference edits.

The WSL/Containers surface reuses WSL Desktop's collector and exact-CID Docker
actions, preserves last-good/stale data, and queries only running distros. Actions
retain/recheck distro registration, wsl.exe, engine and container identity.
Durable operation receipts prevent repeated actions. WSL file/journal requests
use an eight-entry, two-minute native one-time queue and existing Logs adapters;
events carry only an ID. Shell integration retains the same marker, backup and
second-read revision rules through native target admission.

Native WSL task sources use the existing pure task projection on actual POSIX
source bytes from the packaged helper. Source identity includes context/binding;
no UNC or fabricated Windows filesystem identity is used. Preview/apply/trust,
DAG planning, diagnostics and Sessions share the per-database native source owner.
Windows-hosted and standalone source behavior remains unchanged.

A native task retains distro registration and the exact executable through spawn,
handshake, membership, TERM/KILL and recovery checks. The helper's private
--task-exec verifies source/root evidence, opens the real cwd, uses fchdir and
execs existing Runtime supervisor argv. Source replacement does not redirect cwd;
source changes block new starts but do not invalidate an already retained stop
target. Reconstruction after restart fails closed without current source authority.
Source I/O/native target capture use blocking workers; the helper's private runtime
is isolated from entered Tokio contexts.

Problems aggregate LSP, Git/preflight, dependencies, matcher, run/health and Session
observations by source/context/revision. Bounds are 256 batches, 2,048 rows and
512 rows per batch. Late tickets cannot replace newer data; stale/unavailable rows
remain visible with navigation disabled. LSP actor epochs and document versions
invalidate old diagnostics, WSL paths retain case, and undeclared matcher column
encoding becomes line-only. Matcher log locations carry real retained byte offsets.
Navigation rechecks current evidence and uses native Files/Logs/task/port routes;
dirty editor handling remains with Files. Context status reads cached definitions,
existing owners and bounded snapshots without initializing Runtime or collectors.

The shared product-contract summary metadata contract has real Knowledge and
Workspace consumers. Workspace records actual Session interval timestamps and
256 immutable preview receipts. Missing/partial history yields unknown counts,
not fabricated totals. At most eight fixed problem categories are included;
commands, logs, paths and diagnostic text never enter metadata. B07 connects the
native delivery accessor to Knowledge's authenticated preview/insertion flow.

## Migration and recovery

The README lists every persistence source: profile JSON; six localStorage
preference keys; the seventh last-layout key; separately reviewed window state;
and excluded live PTY/buffer/broadcast state.

Preparation holds original JSON and exclusive closed LevelDB files through a
consistent copy. A suspended owned Windows worker runs only the export entry point;
nonce/readiness and COM-reported WebView2 data-directory identity precede access
to seven fixed keys. It never runs the regular product startup path. Missing,
ambiguous, invalid and future sources have explicit report entries.

Apply atomically commits mapped profiles, selected preferences and an import receipt
in the destination namespace. Original start-command definitions remain inactive;
last layout becomes an explicitly restorable profile. Repeated imports preserve
later edits/deletions. Exact preimages, mapping history and current/history revision
checks support explicit restoration. Apply/restore require closed companions.
Temporary-copy cleanup requires the recorded root identity and retired worker
marker; uncertain or replaced copies are retained.

Windows-specific browser/process adapters remain in app platform/command layers.
Only pure snapshot preparation lives in crates/data-migration. Workspace reuses
the actual API Studio/Playground adapter source; affected resolution now tracks
those explicit source edges and their consumer tests. This CI resolver change
requires one full verification at B06 completion.

## Requirement and acceptance mapping

All 37 WSL Desktop inventory entries now point to their product owner/route,
implementation and fixtures in apps/v0.8-feature-parity.json. Status stays pending
until evidence passes. The unused legacy arbitrary-command RPC is not exposed;
user-entered commands remain available through authenticated Terminal input.

| Requirement / scenario | Implementation and acceptance |
|---|---|
| R02–04: parity, direct routes, non-project use | Shared Terminal UI, global Terminal/WSL/Runtime/Logs; parity manifest, component admission and native scripts |
| R05–06 / S01: context, worktrees, trust | Registry/definitions, native task provider and WSL helper; two actual Git worktrees, changed source and scoped diagnostics |
| R07 / S02: Sessions | core/development_sessions.rs, Runtime session leases; shared/borrowed service, immediate cancel, partial failure and independent worktrees |
| R08–09 / S01–02: diagnostics and navigation | core/problems.rs, native providers, Files/Logs bridge; late/stale/encoding/offset tests and actual WSL Problem→file |
| R10 / S08: Terminal lifecycle | Shared input/restore/topology fixtures; Windows two panes, hidden output, reload, WebGL fallback, SIGINT, native/tmux/zellij reconnect |
| R12 / S04: selected summary | Shared session_summary fixture and native immutable preview; actual Knowledge transport/insertion is B07 |
| R15–16 / S08: review, cancel, keyboard, IME | Existing shared dialogs/input/broadcast/focus tests, Session progress/cleanup, native companion fixtures; final DPI/monitor suite evidence remains B09 |
| R17–19: authority and ownership | Component allowlists, per-companion principal, native launch leases, separate queues, Session witnesses and bounded summary metadata |
| R20–21: migration/recovery | Native snapshot/profile/import unit fixtures and real closed-WebView export/import/preimage fixture; no automatic execution |
| R24: bounds and performance | Replay/worker/cache limits, cold Terminal lazy load, inherited exact-machine startup/performance fixture |
| R25: CI and review | One B06 packet, pure extraction separated from semantic commits, affected cross-app source edges, one full final verification |

Prepared actual acceptance entry points:

- windows-product-foundation.mjs runs windows-workspace-terminal-sessions.mjs
  and windows-workspace-tasks-wsl.mjs, then the existing crash restart plus
  windows-workspace-terminal-import.mjs and exclusive PowerShell profile copy.
- windows-workspace-owned-wsl2.ps1 / .mjs verify exact artifact source/run/hash,
  create one disposable distro, run existing Runtime WSL2 and Session/task cases,
  then windows-workspace-multiplexer.mjs and windows-workspace-containers.mjs.
  The latter verifies actual engine/CID/published ports/actions, replacement-name
  rejection and WSL file→Logs UI. A stopped-distro query must not start it.
- The local runner requires an explicitly supplied rootfs archive/digest and packaged
  artifact, creates private app/data roots and confirms cleanup. It never provisions
  or shuts down a user distro. tmux/Docker/busybox use Ubuntu's signed apt packages.
  Fixture-only Zellij [v0.43.1](https://github.com/zellij-org/zellij/releases/tag/v0.43.1)
  uses upstream asset SHA-256
  541d98efef5558293ef85ad9acd29e4d920b6e881513b9e77255d8207020d75a.
  These tools are not product dependencies.
- Independent cleanup failures are collected with the original failure. Unconfirmed
  resource retirement preserves owned fixture roots for diagnosis.

## Verification and remaining work

Per-commit checks used diff/plan review and minimum syntax/type checks. The latest
Rust/Workspace TypeScript graph passed in 17.909 seconds at 298f2e2 (Rust 9.53s).
Earlier passing checks were retained; no detailed tests, Clippy, builds or affected
runs were done merely for commits, rebases or resumed work. New script edits passed Node syntax checks, and the two PowerShell scripts passed
Windows Parser syntax checks before their first acceptance run.

The single all-scope verification began after implementation/fixtures/docs were
complete. All frontend builds passed; the Workspace initial bundle exceeded its
280,000-byte ceiling by 1,865 bytes. Deferring ProjectDefinitions until selection
reduced it to 278,017 raw / 82,563 gzip bytes. Only Workspace was rebuilt; the other
18 app builds were retained. All frontend tests and additional typechecks passed.

Rust check passed in 50.93 seconds. Clippy exposed unit-return adapters, one
snapshot type alias and production items appended after test modules. Targeted
cleanup preserved behavior and the same workspace/test-fixtures feature graph.
Clippy and formatting pass. No frontend build/test or
completed full audit was repeated for these lint fixes. Native acceptance workflow
paths now include the extracted shared feature and actual Workspace native consumers.

All 71 Rust test executables completed with 2,594 passing tests and three ignored
environment/internal-fixture entries; no ignored entry is claimed as Windows
execution. The 1,853 frontend tests, additional typechecks and Rust doc tests pass.
The failed new task helper case used the wrong observe method; it now uses
observe_root, and the private launch entry accepts a valid executable with no
extra arguments. That case rechecked actual cwd, changed source and replaced root.

A filtered Cargo command unexpectedly changed its feature graph and was cancelled.
The original complete graph updated affected artifacts in 75.781 seconds. Only the
failed case and seven unrun executables then ran, in their original package working
directories with Cargo runtime paths and the shared resource wrapper (42.01s).
Their preceding passing cases were retained, including the other nine helper cases.
This is completion of the original full audit, not another full test run.

Additional native-fixture contracts pass: decoded browser expressions (including
all new B06 scripts), explicit long-request budgets, updated moved-source inventory,
and strict separate Terminal/exporter capabilities. Mutation fixtures reject wildcard
main access, root execute permission and remote exporter origins. These fixes did
not rerun compiler/tests outside their affected scope.

PR #559 first completion runs [CI 34717484766](https://github.com/jihoon22-lee/devbox/actions/runs/34717484766)
and [native acceptance 34717484729](https://github.com/jihoon22-lee/devbox/actions/runs/34717484729)
passed frontend/Linux checks and all four debug installer builds. They exposed
three Windows/metadata defects, fixed together before another CI run:

- Regenerated notices update only the two lockfile hash headers. Dependency audit
  reported no vulnerabilities. Workspace's two explicit shared platform imports now
  document their intentionally unused subset, and both native Project variants are
  boxed to satisfy Windows Clippy without enlarging the enum.
- Workspace library tests failed before test main with STATUS_ENTRYPOINT_NOT_FOUND.
  The product imports TaskDialogIndirect, absent from default ComCtl32 v5. Tauri's
  binary-only manifest missed library tests. The [upstream Tauri approach](https://github.com/tauri-apps/tauri/blob/dev/examples/api/src-tauri/build.rs)
  now links one Common Controls v6 manifest to application and test targets on MSVC.
- Native Session acceptance reached shared/borrowed/two-worktree/cancellation cases,
  then stalled on a naturally failing service. Runtime incorrectly treated the Job
  Object signal as an empty-tree witness; [Microsoft documents](https://devblogs.microsoft.com/oldnewthing/20130405-00/?p=4743)
  that ordinary process exit does not guarantee that signal. The exact owned Job
  now uses bounded active-process accounting, preserving fail-closed query errors
  and the original root exit code. A nonzero native regression case is included.

The exact downloaded debug artifact (source 567ffeb40cf62951239071a50eff7c0dc7a30413)
ran an isolated local Windows two-service diagnostic: the failed service stopped,
the steady service remained running and explicit cleanup completed. This did not
reproduce the CI stall, so it is diagnostic evidence, not a replacement for the
failed CI case. Owned app/data/temp cleanup was confirmed; no user distro was used.
Final PR CI, remaining Windows cases and owned WSL2 evidence are still required. The exported foundation artifact has a debug build profile;
it is not release packaged-runtime acceptance. A Windows compile is not execution
evidence.

B07 remains responsible for authenticated cross-product handoff/global shortcut
and summary delivery. B08 owns suite activation/quiesce/import report integration;
B09 owns final parity/performance/installer and exact-main stable promotion.
This packet does not claim those downstream acceptance criteria are complete.

## B05 dependency evidence

B05 PR #558 merged as 41bb98b after final-head CI 34702656787 and product
acceptance 34702656794. Packaged artifact source
fd942e67ea21a76a8d6985e2d104c6acfc341ead passed owned local Windows/WSL2
correlation, descendant/start-tick checks, redaction, group stop and backoff in
25.645 seconds, with app/data/distro/temp cleanup confirmed. WSL1 listener
correlation was explicitly unsupported. The final evidence/cleanup record is in
the existing B05 workthrough, committed with this dependent bundle. Rebase did not
trigger repeated validation or an extra docs-only PR. Host main remains untouched.
