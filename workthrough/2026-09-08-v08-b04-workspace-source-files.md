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

## Actual verification

Extraction passed 307 shared UI tests, package typecheck and four entry builds;
preserved native suites passed 475 tests and component-only strict Clippy. Restored
standalone entry axe smokes passed. The final model/entry state passed affected all
in 702.074 seconds and [general CI](https://github.com/jihoon22-lee/devbox/actions/runs/34204524413).

The native probe/preview owner passed 24 focused Rust tests, strict Clippy and final
affected all in **624.910 seconds** (8 GiB enforced, cgroup peak 6,445,195,264 bytes).
Its [Windows CI](https://github.com/jihoon22-lee/devbox/actions/runs/34207972813) compiled
successfully but failed one probe test: renaming a repository with held child objects
returned PermissionDenied. The corrected test checks that blocked rename preserves
the lease, then releases it and verifies replacement identity. Portable systems still
exercise replacement while the lease is held. This Windows correction awaits rerun.
The [Windows product fixture](https://github.com/jihoon22-lee/devbox/actions/runs/34207972858)
also retains the old placeholder readiness assertion until host integration is complete.

The current activation/schema/probe changes passed **30 focused Rust tests and strict
Clippy in 14.109 seconds**. Tests cover competing owners, cancellation/unselected
preparation, restart, future/corrupt metadata byte preservation, component type
changes, replaced root and bounded preparation without recovery deletion. Initial
new-store tests caught missing optional files being rejected; absence is now checked
one path component at a time while existing symlink/reparse paths remain rejected.
Final affected all passed in **675.610 seconds** (8 GiB enforced; cgroup peak
6,445,158,400 bytes), including all frontend and Rust gates. Generated notices
changed only the Cargo.lock digest. These Linux checks do not establish actual
Windows product or WSL LSP parity.

The native host now connects an explicit blank start and Registry UI to that
owner. Authentication precedes closed activation/Registry method roles. External
probes and metadata use separate two-worker bounds; timed-out probes keep their
permits until the OS returns. Preview IDs expire, cancel and cannot grant trust.
The UI guards stale load responses, confirms register/rebind and removal, and
submits only reviewed tokens. The current native view is startup/Registry; shared
feature previews remain browser-only until their command/context guards are wired.

The host passed **32 native Rust tests and strict Clippy**. The combined run then
caught a missing direct Tauri frontend dependency and an invalid testing-library
option. After those corrections the Workspace build, **4 UI tests including axe**,
metadata checker and CI scope runner tests passed in **21.489 seconds**. The new
Windows fixture covers actual blank-start/preview/cancel/register UI plus native
replay/role/foreign-installation denial, rename/removal and source-file preservation.
Its source is syntax checked; Windows execution is pending.

The pushed store state [7a856ce Windows CI](https://github.com/jihoon22-lee/devbox/actions/runs/34210889932)
failed before tests because concurrent Tauri build scripts copied the same notice
staging file (Windows sharing violation 32). Windows Rust CI now uses one Cargo
build job; check/Clippy/test scope and test-harness concurrency remain intact.
Linux/local budgets are unchanged. This requires the next Windows CI execution.

The first host affected run failed the catalog's closed authority parser after
684.778 seconds: the new Registry role had been declared in JSON but not admitted
by the Rust catalog. The exact Workspace owner/role pairs are now registered;
foreign-owner, network-authority and unknown-component mutations remain rejected.
Focused catalog tests/strict Clippy/metadata validation passed in **4.387 seconds**.
The corrected final affected all passed in **587.042 seconds** under the shared
8 GiB cap (cgroup peak 6,445,002,752 bytes), including every frontend and Rust gate.
The native Windows registration fixture and serial Cargo CI await the pushed head.

The next change adds native project selection with fresh Registry/root/Git
admission, stale-context rejection and request-deadline/previous-context checks.
Shell metadata refresh keeps the feature subtree mounted. Added regressions cover
root replacement, explicit rebind, stale revision/removal, explicit selection and
preservation of an unsaved UI buffer; the Windows fixture adds actual selection,
stale-header denial and clear. Workspace **33** and shared context **8** Rust
tests, strict Clippy, and **11** shell tests passed in **43.577 seconds**. The
Workspace build and **5** UI tests including axe then passed in **16.765 seconds**.
The previous [native run](https://github.com/jihoon22-lee/devbox/actions/runs/34215893198)
failed because the registration fixture generated an invalid JavaScript request
(a missing object delimiter); module syntax checking did not parse that generated
string. Request generation now serializes the complete payload, and a passing
regression executes the generated expressions with and without selected context,
including quoted Korean paths. Windows acceptance runs it before packaging.
The ongoing affected run was stopped to include this correction. The corrected
final affected all passed in **566.209 seconds** under the shared 8 GiB cap
(cgroup peak 6,445,039,616 bytes). Actual Windows selection execution is pending.

## Remaining acceptance

Shared feature IPC/transport, trust UI, project/source/file context with dirty
transitions, Dependencies route, importer/recovery mappings,
Windows LSP integration and WSL-native transport remain incomplete. Retained hidden
features need active-route keyboard/listener ownership. B03 supplies shared shell
containment on integration. Strict metadata parsing is an activation prerequisite,
not a completed legacy importer. Windows CI remains required for the current
changes; no R/S or completion issue is closed by this draft.
