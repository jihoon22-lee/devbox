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

## Remaining acceptance

Host IPC/transport and startup controls, registration/trust UI, project/source/file
context with dirty transitions, Dependencies route, importer/recovery mappings,
Windows LSP integration and WSL-native transport remain incomplete. Retained hidden
features need active-route keyboard/listener ownership. B03 supplies shared shell
containment on integration. Strict metadata parsing is an activation prerequisite,
not a completed legacy importer. Windows CI remains required for the current
changes; no R/S or completion issue is closed by this draft.
