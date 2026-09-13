# v0.8 B07 — Global commands, search and product workflows

Refs #549, #541, #542. One B07 bundle covers Launcher/shortcut ownership,
federated providers, cross-product artifacts, Operations/Review and preference
migration. This branch starts from B06 02ba916 while its final acceptance runs.
Rebase only B07 commits onto merged B06 before publishing B07.

## Implemented first slice

The shared product-contract command descriptor names an owner/component, typed
route/entity reference, exact revision, context requirement, disabled reason and
review/destructive flags. Requests contain operation/command/revision/context and
an optional selection ID, never argv or content. Native owners retain authority;
a descriptor or preview is not execution approval.

Legacy Launcher now consumes the same ordered exact/prefix/substring field matcher.
Its field priorities, query normalization and bounded pre-cap ranking remain
unchanged. Control Center's bounded command index consumes Catalog v3's actual
feature command IDs and revision evidence, rejects duplicate provider identities,
and retains unavailable owners visibly. The new native command plugin authenticates
its local product session and exposes search/current-command preview only.

The Control Center products surface uses the native catalog, bounded debounced
queries and late-response rejection. A validated local route opens through the
existing shell navigation. Unconnected external providers remain unavailable;
this slice has not implemented cold/hot cross-product dispatch or global shortcuts.
Browser data stays explicitly labelled by the existing product-shell fixture mode.

## Native connection slice

All four products now consume the same application-platform pipe/installation
adapters and a lazy shared connection review surface. B07 defines a bounded
installation declaration with fixed suite/portable paths, exact version/digests
and no argv/environment/data-root fields. Native capture pins root ancestors,
manifest and executable objects against reparse/replacement; a declaration alone
starts no listener. Each running product requires an explicit, expiring review
of its own package before it accepts this generation's connections. Disconnect
revokes the capture and retires its listener/client tasks.

The Windows pipe owner rejects remote clients and duplicate registration, caps
connections/frames/deadlines and binds requests to a fresh native session. Both
ends identify the actual OS pipe process/image in the pinned installation;
blocking identity tasks own duplicated pipe handles so timeout cannot redirect a
reused handle. Foreign namespace/generation, unsupported protocol, stale session
and replay are rejected before owner dispatch. Only Control Center can currently
query/preview; other roles can describe their installed peer. The server exposes
its own catalog metadata and preview, with no cold launch or claimed navigation.

Catalog index logic moved into product-contract when the four product owners
became real consumers. Windows adapters remain in the app platform layer through
explicit source includes; affected CI records all four consumers. No new external
package version is introduced. The initial approval is process-local; B08 package
activation and durable reviewed installation selection remain separate work.

## Federated commands and received navigation

The Control Center now queries each connected product's command catalog separately.
Per-source deadline/errors and late-generation rejection preserve other results;
foreign same-name commands keep owner-scoped IDs. Preview resolves the current
remote descriptor. A destination-owned, bounded navigation queue separates
awaiting review, opening, opened, rejected and expired. Exact operation/content
receipts prevent duplicate opening; an ID reused with different content is rejected.

Each product's shell receives a notification and lets the user open or reject the
requested route without replacing project context or discarding a mounted draft.
The receiver acknowledges only the selected route after UI navigation. The sender
retains an unknown delivery receipt when a reply fails, allowing an explicit status
check without automatically repeating the command. Entity/context navigation still
requires its actual domain adapter and is explicitly unavailable in this slice.
Pure queue fixtures cover duplicate/conflict/reject/expiry/revocation and incorrect
acknowledgement; a UI fixture covers a slow source and stale-generation response.

## Reused Launcher and shortcut owner

The actual legacy Launcher UI now lives under product-shell/launcher with an
injected adapter. The legacy entry supplies its unchanged native API/clipboard
behavior and retains its existing UI tests. Scoped CSS prevents palette styles
from changing other product screens. Control Center consumes the same keyboard,
IME, selection, stale-review and favorite controls in a lazy modal while retaining
its current route. Product queries stream independently through the adapter;
unsupported owners remain visible and cannot be opened accidentally.

The existing ID-only Preferences implementation and its tests moved to
product-contract for the second native consumer. Control Center stores favorites
and local route recents in its own installation namespace, requires current
remote metadata before adding a favorite, and permits removing an unavailable
favorite without reopening its provider. Corrupt settings are not overwritten.

The existing Windows hotkey worker now accepts a bounded binding list and callback
while its legacy entry preserves the same three allowed Launcher accelerators.
Control Center uses one worker and an OS existence lease so another v0.8 install
cannot simultaneously own global shortcuts. Configuration requires an approved
package connection, attempts previous-key restoration after registration failure,
reports restoration/foreign-owner failures, and retires on disconnect/exit. It
adds no startup entry. The initial product binding opens the actual Launcher;
Terminal/Capture/current-project bindings remain rejected until their domain
handlers are connected. No Ctrl+C binding is registered. The palette checks active
modal/IME/editing state and distinguishes an already-focused host from an external
activation before changing focus.

## Workspace project/repository provider

The shared suite adapter now accepts an actual product-domain handler and declares
its supported sources. Workspace Projects and Repositories/Worktrees query its
activated native Registry, with two retained reader permits. They read metadata
without starting Runtime, probing project folders or waking a distro. Same-name
registrations retain separate command IDs and native contexts; revisions bind the
current Registry/worktree/name, and exported rows contain no root paths.

Dynamic preview re-resolves the current Registry. A typed entity descriptor must
name an existing owner review route before it enters the common navigation queue.
The shell carries its target/context as an in-memory review hint. Workspace refreshes
and highlights that exact Registry entry; the existing project-select button and
native binding/trust checks still own the actual context change. Missing/stale
entries remain an explicit review failure. A route switch grants no project/file
access. Fixtures cover same-name distinct targets, path omission, stale rename
and entity review requirements. Workspace/Control Center minimum TypeScript checks
passed in 11.666 seconds; no detailed tests were run for this slice.

## Verification and remaining work

Regression fixtures are authored for stale/disabled/forged references, required
review/context/selection, inherited ranking, duplicate commands and late UI queries.
Rust syntax parsing/formatting and the Control Center TypeScript check passed in
1.813 seconds under the shared resource wrapper after the first slice was wired.
The connection slice also passed Rust syntax parsing and the Control Center/shared-shell
TypeScript check in 2.878 seconds. The next navigation/federation slice passed the
same minimum TypeScript check after correcting an unused test import; Rust files
were syntax-parsed/formatted. Its regression fixtures have not run yet. The shared Launcher/host passed the two
consumer TypeScript check in 4.484 seconds; Rust changes were syntax-parsed. A
missing direct Tauri dependency was resolved through the existing shared adapter
boundary, without adding a package version. Subsequent presentation changes were
checked with the same minimum typecheck, not tests/builds. No B07 Cargo compilation, tests, build, Clippy or affected run has occurred. Detailed
verification waits until the whole B07 bundle is implemented. Node dependencies
were installed from the existing offline lock/store; no package versions changed.

Next: cold launch and durable approved installation selection; one shortcut owner and diagnostic settings;
federated metadata/content providers; source-owned artifact claim/restore/ack and
Knowledge summary/capture; shared operation/review projections; legacy preference
mapping; native end-to-end fixtures. B08 owns final installer/activation topology.

## Federated owner queries

Request UUIDs now identify native cancellation independently of display generation.
A pre-arrival cancellation remains effective; Commands and Launcher cannot cancel
one another through overlapping counters. Disconnect cancels outstanding queries,
and a dropped Knowledge query retires its own search lease. Knowledge keeps the
existing aggregate worker/object limits while separating product and federated
job eviction/cancel rights; no new query can revoke the other consumer's references.

Commands now offers Projects, Repositories/Worktrees, Tasks, Services, Recent Runs,
Indexed Files, Notes and Saved Queries. Knowledge reuses its bounded filename/content
reader and opaque references, exporting labels, availability/staleness and partial
state without paths/snippets/body. Content search is explicitly selected. The receiver
revalidates references before Notes/default-app opening; file-to-Workspace Editor
handoff still remains below. Saved queries reuse the actual legacy validator, native
store-generation revision and owner-side apply review; their text/filter remains
inside Knowledge. No query/body is added to shared recent history.

Runtime metadata uses a producer-owned read-only SQLite transaction with a 1.5-second
progress deadline and row/label caps. It selects no command/cwd/environment and starts
no scheduler, migration or legacy publication. Native root/database/sidecar handles
and schema checks surround the read. Tasks/services use existing selection/dirty
editor handling; a run opens its current owner record and existing Logs handoff.
Fixtures cover source-consumer isolation, early cancellation, same-name targets and
metadata exclusion. They are prepared for the completed B07 PR's detailed run.

This slice passed Rust syntax/format parsing and the three changed product TypeScript
checks (12.682 seconds). No Cargo tests, Clippy, frontend tests/builds or affected
verification were run while B07 remains incomplete. B06's independent final failure
repair is not a reason to repeat B07 checks.

## Reviewed reconnection and cold product routes

Connection review can explicitly remember permission in that product's own
installation data. Startup captures its native package again and reconnects only
when root identity, manifest generation/member digests and product version match
the stored bounded receipt. Unknown/corrupt/replaced receipts never auto-approve;
manual disconnect removes persistence and wins over a late startup capture.
The first connection in each product still requires its own visible review.

Control Center catalog availability now comes from its approved, pinned package
members. Selecting an available cold product route launches only that member's
exact native image and waits for an authenticated pipe peer. Per-product launch
locks prevent duplicate concurrent cold launches. Only readiness Describe probes
repeat; the real command is sent once, with ambiguous replies retaining the existing
operation receipt. Search queries never start an unavailable product. No command,
query, file path or payload is put in argv and no startup entry is added.

This slice passed Rust syntax parsing and the changed Control Center TypeScript
check (2.345 seconds). Native lifecycle/namespace fixtures remain pending. Shortcut restoration, remaining
shortcut commands, registry/Editor provider and Artifact integration still follow
within this same B07 PR before detailed verification.

## Four shortcut commands and shared product settings

The single native registration worker now binds Launcher, existing Terminal
visibility, Knowledge Quick Capture and current-project review. Non-Launcher
callbacks no longer focus Control Center before the destination acts, preserving
Terminal show/hide behavior. The command host resolves current native descriptors
and sends the command once; an unavailable/ambiguous terminal opens its existing
chooser instead of starting a shell or selecting another worktree. Existing-window
visibility reserves a destination receipt before action so retry cannot retoggle.

Terminal profile commands select only a matching current profile revision for the
existing explicit open button. Knowledge capture opens the existing Inbox dialog;
preview/save and existing drafts remain with Notes. Current-project commands use
the existing Registry review/selection. The receiver now carries the descriptor
revision separately from its navigation-review proof, including Saved Query review.

Product-local connection settings and hosted Launcher use the same shortcut controls.
The shared pure configuration only admits the fixed four command bindings; no Ctrl+C
or arbitrary key/command crosses IPC. Other authenticated products can read/change
only this installation's native Control Center shortcut owner through the common
settings surface. Remembered connection restoration re-establishes explicitly
enabled registration; disconnect/exit releases it, and foreign-owner conflict stays
visible. Legacy standalone shortcut configuration remains its original allowed set.
Notes/Terminal labels identify the suite owner instead of reporting a second local
registration. Modal/IME/editor guards and existing dirty checks remain active.

Syntax and changed product/legacy Launcher type checks passed (15.324 seconds for
wiring, 3.907 seconds for extracted shared controls). Knowledge label/type checks also passed (4.708 seconds). Native shortcut/focus/lifecycle fixtures and the
prepared direct-action/unique-binding cases run at B07 PR completion. No detailed
B07 test/build/Clippy/affected verification has started.

The user's Docker/network incident constraint is active on this checkout too:
local network-changing fixtures and private-copy bypasses are forbidden. B06 owns
the shared instruction and runner-guard commit; these identical safety edits are
kept here immediately and will be reconciled when rebasing onto merged B06.

## Native project provider for Knowledge

Current Project query, Activity projections and native open now refresh Workspace's
registry through the authenticated product pipe. Only Knowledge can request this
path-bearing snapshot; Control Center search receives its existing metadata-only
results. The producer resolves GUID-bound WSL names, re-admits the current root
against Registry object evidence without starting a stopped distro, and marks
noncurrent roots unverified. It never initializes the Runtime scheduler. Snapshot
size/row bounds fail closed instead of widening Current Project to all indexed roots.

Native producer epoch and monotonic content revisions cover restart, selection and
availability changes. Knowledge serializes refresh, revokes old project references
on disconnect/failed refresh, and rejects late replies across invalidation. Native
Workspace selection/registry-change notifications invalidate the receiver; query
and open also refresh independently. There is no idle polling or automatic product
start. Existing Activity rows/aggregation remain intact, with explicit unverified
association labels rather than invented availability.

The shared snapshot structs now have two actual consumers. Rust syntax/format
parsing and the diff/plan review cover this commit; role-isolation and unverified
association regression cases are prepared for B07's single completion verification.
Native changed-context/disconnected-provider fixtures remain pending with the other
B07 acceptance work.

## Confirmed remote recency

Control Center reserves bounded native command receipts before explicit dispatch.
One native observer checks destination status while a request remains pending and
sleeps when no work remains. It never replays OpenCommand. Only an authenticated
Opened receipt for the same operation/product/command writes that opaque command
ID to existing Launcher preferences; missing replies, rejection and expiry do not.
Manual recent-history clearing suppresses pending writes and disconnect revokes
pending observations. Corrupt preference files remain untouched. This also covers
shortcut commands after Launcher closes or Control Center loses focus.

Only Rust syntax/format and diff/plan checks ran for this slice. Bounded-capacity,
retargeted-operation, clear and disconnect cases are prepared for PR completion;
real remote acknowledgement/lost-reply fixtures still belong to B07 acceptance.

## Legacy Launcher preference import

The migration route now previews the native legacy Launcher's bounded JSON
preferences and allowed shortcut settings. Fourteen exact legacy catalog IDs map
to suite commands. Dynamic Workbench/Repository IDs use Workspace's existing native
legacy-reference resolver only when it returns one exact current context; ambiguous,
missing or unavailable mappings remain visible unresolved IDs. Existing suite order
wins, repeat imports deduplicate, and capacity exclusions are visible. Labels and
paths never serve as identity guesses or enter the preference journal.

Explicit apply rechecks the source fingerprint, owner mappings and destination
preimage. A bounded destination journal is written before preferences, followed by
optional disabled shortcut configuration and a committed receipt. Resume recognizes
the exact before/after images and refuses changed destination settings. Source files
are never written. Existing suite shortcuts remain intact; a newly imported binding
stays disabled until the user enables the reviewed installation in common settings.
The old Launcher/Terminal default collision and suggested suite bindings are shown.

Changed Control Center TypeScript passed (2.065 seconds), with Rust syntax/format
and diff review only. Exact-ID/unresolved/repeat and interrupted-write preimage
cases are prepared; native source-preservation/journal-resume and shortcut conflict
acceptance will run with the completed B07 PR, not after this commit.

## Session summary to Daily and Notes preview

Workspace's explicit summary action now requests the authenticated Knowledge Daily
review. Only Workspace can deliver and only Knowledge can read the immutable source
summary; Control Center does not acquire these methods. After destination route
review, Knowledge re-reads the exact source operation/revision and prepares the
existing strict metadata-only Session draft. A separate action opens Notes' existing
claim/preview/cancel/save flow. No existing Daily note is appended or overwritten.

A bounded receiver ledger persists the source operation -> one-time handoff identity
before any offer. Retries reuse it, consumed receipts report saved, and expired or
changed references fail closed. Interrupted pre-ledger publication can leave only an
unoffered expiring envelope. Existing native Notes claim restoration, acknowledgement
and exclusive note creation remain authoritative. Two retained receiver permits
bound the source/IO flow; direct preview invocation must prove the exact reviewed
route, target and descriptor revision. No body, path or command is put in argv.

Workspace/Knowledge TypeScript passed (8.334 seconds); Rust syntax/format and diff
checks passed. Exact reviewed-target regression is prepared. Native cancellation,
duplicate delivery, expired/source-deleted reference and dirty Notes acceptance
remain with the completed B07 PR verification.

## Indexed file to Workspace Editor

Knowledge Search and received federated file results now offer Workspace Editor
using the native source reference. Raw renderer paths are not sent. The existing
source opener rechecks index revision/root/file identity and exports a bounded
native proof only to Workspace; Control Center cannot resolve it. Explicit delivery
can activate the pinned Workspace member, then the existing destination route review
precedes native file approval. Missing installation/connection remains an explicit
unavailable action, with the default-app action separately available.

Workspace retains the confirmed file object and source lease expiry through the
existing Files open request. The file API carries the received reference separately
from the path, so delayed opening cannot fall back to an unrelated root grant.
Windows files use the existing native choice owner. WSL UNC files require the current
registered GUID/context/root, a running distro and exact Linux path mapping; both
sides of the helper read recheck the same Windows file identity. No project selection
or stopped distro start is implicit. Existing editor hydration, dirty buffers,
rename barriers and per-context checks remain in the actual open/adopt path.

Changed Workspace/Knowledge TypeScript passed (9.242 seconds) and Rust syntax/format
parsed. File-object/revision and transport-role regression cases are prepared;
detailed same-name/replaced-file/expired-reference/dirty-editor/WSL fixtures run at
B07 completion. No detailed B07 test/build/Clippy/affected run has started.


## Saved API/Transforms result to Notes

Explicitly saved, masked results now offer the authenticated Knowledge Notes review.
The producer keeps the original draft and export/delete actions; saving alone does
not send it. Knowledge re-reads the exact source-owned artifact revision after native
route review, then reuses Notes' existing claim, preview, cancel and exclusive-save
flow. The shared bounded receipt ledger reuses one handoff per source artifact.
Control Center cannot resolve draft bodies, and legacy external AppLinks cannot
inject the new product-only kind. Requests/Transforms owners are the only admitted
result components; the Protocols route remains non-exportable.

Changed API Studio/Knowledge TypeScript passed (6.107 seconds). Shared DTO ownership,
content revision and native wire role regressions are prepared for PR completion;
Rust syntax/format and diff review are the commit checks. Detailed B07 verification
has not started. No local service or networking fixture was executed.


## Workspace selections to Transforms

Files' context menu now sends one explicit UTF-16 selection after the existing
native editor mirror flush. Native Windows/WSL document identity, disk baseline,
current buffer hash and project context must still match. The bounded source lease
retains only masked selected text and proof metadata, and the receiver rechecks the
source before opening Transforms' existing preview/apply/cancel flow. Closing,
changing or replacing the source rejects a delayed read. No clipboard fallback,
legacy executable or implicit replacement of existing Transforms input is used.

Logs resolves selected source/sequence IDs from bounded native read snapshots.
It re-reads the original source cursors and matches the selected records before
publication and destination resolution; missing/changed source rows fail closed.
This shares the same native transport/review/publication path as Editor selections.
The receiver's bounded receipt journal reuses the one-time publication across
retries/restart and recognizes consumed receipts. Control Center cannot read text.
The existing API selection and HMAC/ephemeral-source restrictions remain owned by
their original producers. A new Code Pad producer is admitted to the existing
masked Toolbox text format; the receiver still requires preview and explicit apply.

Workspace/API TypeScript passed for the Editor slice (11.416 seconds). Logs adapter
completion receives only a changed-type check; UTF-16/masking/native role and stale
source cases are prepared for the complete B07 audit. No B07 detailed audit yet.

Logs adapter changed TypeScript passed (9.668 seconds); syntax/diff checks passed.
Native role, Unicode offset and native buffer hash regressions are prepared,
with the detailed tests still deferred to PR completion.


## Owner Operations and common Review

All four product shells now lazy-load the same Operations surface. Control Center
reads each already-running, approved product independently; other products show
their own rows. Native projections cover Workspace development sessions, legacy
preservation and recent task/service runs; API migration and temporary listener;
Knowledge migration and cached index-worker progress. Source paths, commands,
queries and payloads are absent. Reads do not start a scheduler, indexer or product.
Requests stop when the panel closes or document hides, with at most one refresh
round active; capped phase-change announcements avoid duplicate notifications.

Cancel requested, cancelled, uncancellable commit, failure and unknown state remain
distinct. Completed API migration observations retain a bounded native in-memory
history. Index idle state does not invent success when worker completion evidence
is unavailable. Each row opens an exact-revision review in its owning product;
the common surface has no cancellation or approval authority. Existing owner
screens keep their stricter action reviews. Incoming Review now describes the
owner, exact target, current revision and screen-opening effect together.

Shared shell and four product TypeScript passed (14.544 seconds) after replacing
unsupported Object.hasOwn with the repository-compatible property check. Rust
syntax/format and diff checks are the remaining commit checks. Cancellation-phase
and reviewed-route regressions are prepared for the B07 completion audit.

The B06 retained-artifact observation also applies to Control Center's shortcut
callback: native foreground HWND now supplies wasFocused for the existing modal,
text-editor and IME guards. No additional shortcut or key interception is added.


## Four-product native fixture packet

The hosted suite fixture stages the four already-built executables under one exact
manifest, exercises approved peers, same-name projects, route rejection/acceptance,
replay/stale revision, remembered cold activation, native Editor/Logs selections,
indexed file opening with a dirty editor, saved API results and Session summaries
through real destination previews. It also checks a second installation's shortcut
conflict/provider isolation, native global hotkeys and bounded Operations surfaces.
High contrast/DPI use renderer emulation; composition guards use WebView events and
Unicode input. These do not claim an OS Korean IME or multi-monitor DPI observation.

The fixture is prepared, syntax checked and not executed yet. It uses only a guarded
disposable hosted Windows VM and reuses the completed product build. Existing owned
process-identity cleanup is shared for cold-launched processes. Native cold launch
now removes sender-specific WebView profile/debug environment overrides so another
product cannot inherit the sender's browser store. The Transforms producer label and
stored-draft assertion were aligned with the final behavior before the PR audit.


## B07 verification boundary

B07 has been rebased onto B06 dde28c1 before its first detailed audit, retaining the
native Runtime/PTY/focus repairs and all local network-test guards. The copied safety
changes were stashed, not reapplied over the newer B06 scripts; their policy and
entrypoint guards are present upstream. Cargo's conflict combined Pipes and Windows
UI features. The four-product fixture now runs against the existing Windows product
build in foundation CI. No build is added just for this fixture.

Acceptance mapping for the one PR audit:
- R03/04/11-19/21/24-25: command/provider/native transport, exact peer identities,
  source cancellation/freshness and preference importer unit/authority tests.
- S01-05/S08: existing product native fixtures plus windows-suite-workflows.mjs for
  actual destination reviews, source preservation and four-product delivery.
- Owner Operations/Review: projection and navigation tests plus real product panel
  reads; cancellation intent is never reported as completed cancellation.
- Focus/keyboard/budgets: native registration/input, composition and modal guards,
  route/bundle checks and existing Windows performance measurements. Renderer DPI
  emulation remains distinct from an OS multi-monitor observation.

Run `pnpm verify:affected` once now that the bundle's implementation/importer/fixture
packet is assembled. On failure retain completed evidence and continue only failed,
changed or not-yet-run checks. Windows execution and final-head CI remain separate
required gates. B08/B09 remain unfinished; #541/#542 are not automatically closed.


First detailed audit progress: metadata/resource checks passed; scope regression
expectations required the legacy Launcher's new shared-contract/UI dependency and
the shared remembered-connection source edge. Failed scope tests and remaining
runner tests then passed (3.633 seconds); resolver correctly selected all.
All frontend builds completed. The budget check found Workspace initial JS at
283,907 bytes versus 280,000. RegistryGate is now a lazy feature boundary, preserving
its UI and loading it after the shell. Other products' passed builds/budgets remain
valid; rerun only Workspace build/budget, then continue unrun frontend tests/types
and Rust checks. Full compiler continuation before that failure took 137.752 seconds.
