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
The second completion runs (head 3b52d11; CI 34720915215 / native 34720915212)
passed Windows compilation/Clippy and the owned WSL helper/source/execution cases.
Native library tests now run: 172 Workspace cases passed, with one real URI-path
failure and three explicitly ignored cases. Windows was interpreting Linux LSP
file URIs through host-specific Url::to_file_path. URI decoding now follows the
bound target rules, including case-sensitive POSIX paths, Windows drive/UNC roots,
percent-encoded UTF-8 and rejection of malformed/foreign/query/fragment locations.

The packaged Runtime stop case exposed an additional ordering boundary: Job
accounting can reach zero before the root process handle signals. A zero-time root
wait falsely rejected an already terminating tree. The root signal now uses the
remainder of the same bounded stop deadline. An owned root/two-descendant native
regression fixture covers this boundary. The exact second artifact
3aca46830b90830d672027167a4ded87acea2e5b reproduced the failed stop locally on its
first disposable descendant run; explicit retry retired it. The failure was reported
as storage-failed by the scheduler's recovery fallback, not evidence of SQLite
corruption. This observation is not a pass for the corrected code.

The scheduler now preserves the durable stopping intent when a late cleanup
witness arrives after a failed stop, rather than fabricating a storage error when
there is no secondary terminal error. Existing recorded errors retain precedence.
The same full Cargo feature graph rebuilt test artifacts in 357.111 seconds
(previous notice-header changes also invalidated Tauri resource consumers). Only
the two URI cases and five affected stop/wait regressions then ran: all seven
passed in 0.782 seconds under the shared resource wrapper. Other passing tests
were retained. Windows-specific root/descendant execution remains a CI requirement.
The disposable local app, data and final temp directory were all removed.

Both Windows jobs reported No cache found and their failed runs skipped cache
saving. They now preserve compiled dependency caches on failure, retaining the
compiler/manifest/environment key and default exclusion of workspace crates.
This changes cache retention only; test results and final-head gates are unchanged.
The behavior is documented in verification operations; changed workflow YAML and
cache inputs parsed successfully with the other cache defaults retained. No passed full audit is
repeated solely for this cache setting; URI tests and changed Windows retirement
cases are the required rechecks.

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

## Remaining native companion failure

Final-head CI 34723990415 passed for e75bf83. Foundation 34723990438 passed
Windows authority/native tests and installer builds, then reached the new companion
fixture after Runtime and shared/borrowed/two-worktree/partial-failure Sessions.
Both panes failed before PTY admission. Its exact exported debug artifact
26680aa9a726ec5e816b7c16d6508b089349f523 reproduced the same failure locally.
The owned WSL2 Runtime correlation/group-stop/backoff checks passed; Sessions,
Terminal, multiplexer and container acceptance are not yet complete.

A minimal companion-only reproduction captured the native panic at
wsl_distro.rs:399: a synchronous WSL lease attempted to start its own runtime while
the retained Terminal worker was driving an async PTY future. The adapter now
joins synchronous capture, validation, running-state checks and final lease/runtime
retirement outside the caller's async context. Its existing request/cleanup permits
remain held until the joined work completes. Pure regression passed (one test,
0.521-second runner) after only the changed same-feature Workspace test artifact
was rebuilt (35.148 seconds). No passing full audit was repeated.

Synthetic Windows runners now retain bounded native stderr, so a panic no longer
appears only as a later generic renderer timeout. The owned WSL2 runner installs
Git before checking it, allows the first newly imported distro up to 90 seconds
to boot, and preserves that start result. The Session fixture uses its own /home
folder so distro idle shutdown cannot discard its /tmp owner/cwd. All disposable
local app/data/distro/temp resources were removed. Diagnostic runs are not Terminal
PASS evidence. JavaScript and PowerShell syntax checks passed; final Windows
execution of this repair and the previously unreached cases remains required.

## Local Docker incident and execution boundary

On 2026-09-13 the user reported that this work's local Docker testing affected
existing services and removed some iptables state. The runner installed Docker
and started dockerd in a new WSL2 distribution; unique distro/data ownership did
not isolate shared host networking. Deleting the fixture therefore does not prove
preservation of existing network state. Previous local cleanup evidence proves
only removal of owned files/processes/distro registrations, not network safety.

No local acceptance worker was running when this report was checked. Local Docker
and this WSL2 provisioning runner are now blocked before setup, with independent
PowerShell, Node entrypoint and container-function gates. Existing private local
reproduction entrypoints were disabled. AGENTS, CONVENTIONS, verification operations
and both development/review skills now require disposable hosted CI or a separately
verified independent VM for network-changing tests and prohibit spoofed environment
variables, private-copy bypasses and automatic host-service/firewall recovery.
The Docker/WSL2 acceptance items remain unrun after the latest repair until a safe
isolated environment executes them. This incident is not an application test PASS.

The two pure execution-guard cases passed (0.272-second runner), and an actual
local PowerShell invocation was rejected before provisioning. The guard cases
are included in the existing Windows fixture-script test entry. No Docker or
existing service command was run to verify this prevention change.

## Fast-exit result retention and isolated CI continuation

CI 34727358256 passed on 2ab3837. Foundation 34727358186 again hit the
intermittent early service-exit case before reaching Terminal. The shared Windows/
WSL execution actor used watch::Sender::send after its initial receiver was dropped;
a fast process could finish before the scheduler subscribed and permanently lose
its terminal/cleanup witness. Both actor publishers now use the same retained-value
publication helper. Pure late-subscriber wait/stop and confirmed-error fixtures
cover the gap without starting a process or touching Docker/network state.

The remaining WSL2/real-container suite is moved to a separate disposable
windows-2025 GitHub-hosted job after the preceding native job passes. It consumes
that run's exact Workspace artifact and does not compile the products again. The
paired runner checks the actual hosted environment and artifact source/run before
provisioning. Unsupported WSL2 remains a failed/unrun gate; no local fallback or
network-isolation bypass is supplied.

The same-feature test artifact refresh took 61 seconds; the three affected pure
execution-result tests passed in a 0.266-second runner. Existing all-scope build,
Clippy/frontend/test evidence remains preserved. The new isolated job YAML parsed
and contains no Cargo or product build step. These are targeted supplements for
the reproduced race and changed execution boundary, not another full local audit.

### Retained-artifact terminal diagnosis after fifth CI

Final implementation CI [34729198146](https://github.com/jihoon22-lee/devbox/actions/runs/34729198146)
passed. Foundation [34729198195](https://github.com/jihoon22-lee/devbox/actions/runs/34729198195)
passed native authority, service cancellation/fast failure and product builds, then
failed waiting for two companion panes. No native panic was recorded. Hosted WSL2
acceptance did not run because its prerequisite job failed; this is not a pass.

A dispatch-only hosted diagnostic reuses that exact private executable/helper
artifact, verifies its manifest/digests, and records bounded companion UI/native
state without Cargo, frontend builds, Runtime acceptance or Docker. Artifact source
and diagnostic script source are recorded separately. The shared CDP adapter is
side-effect free on import; local execution still fails at the hosted-only guard.
Ordinary terminal acceptance now captures companion state before cleanup on failure.
Only script/YAML syntax and the no-build diagnostic boundary were checked locally.
This diagnostic-only commit suppresses automatic full CI; final implementation
changes still require CI before merge. No local WSL or service operation is used.

The first retained-artifact diagnostic (34731262944) could not attach CDP before
its startup deadline. The hosted runner is elevated, so WebView2 ignores the process
debug-port override; the established packaged-shell harness already handles this
through a uniquely named image's temporary policy. Both new hosted runners now reuse
that exact inspect/install/restore adapter and record the actual artifact source.
The failed diagnostic did not exercise PTY restore and is not acceptance evidence.

Retained-artifact diagnostic 34731448607 reached the companion and showed failed
resource/Docker dashboard hydration, an empty PTY list, and a valid current distro
list. Diagnostic 34731767885 then started both exact profile panes through the native
owner successfully (s1/s2), confirming that dashboard hydration—not PTY creation—
blocked the UI restore. Both diagnostics used the same built source 96976ba8fccd6b15dfb9a0a9c7d3aeb347298d84
on disposable hosted WSL1, and confirmed their app/data cleanup. They are diagnostic
evidence, not full Terminal acceptance.

The product companion now obtains a fresh read-only distro list when initial
telemetry fails. That permits an explicitly opened saved profile to reach existing
native binding admission; it does not guess a default target or re-enable broadcast.
A failed distro read still blocks restore, and legacy behavior is unchanged. The
five existing snapshot-control cases passed; the two new product cases passed after
repairing their missing native-storage fixture setup (2.901 seconds). No unrelated
passing test suite or Cargo audit was repeated. Changed feature/product type checks
passed (18.180 seconds) and were the only other local verification before the final CI repair commit.
