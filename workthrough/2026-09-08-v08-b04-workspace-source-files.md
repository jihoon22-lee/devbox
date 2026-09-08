# B04 Workspace Source and Files

WP #546 / R02–06, R08, R13, R16–21, R24–25 and S01. This bundle owns
Project/Worktree Registry, manifest/local trust, Overview/Source/Files/Dependencies,
legacy import and WSL-native LSP. Runtime and Terminal/session owners remain B05/B06.

## Current change

Workbench, Repo Manager and Code Pad UI/test trees are shared through
`packages/workspace-features`. Legacy app entries use those same components;
Workspace loads Overview, Source and Files lazily and retains visited routes.
Feature CSS is scoped to its container. No external dependency was introduced.
The old static catalog import was adjusted to its unchanged repository source.

The native engines expose 111 typed adapters (Workbench 32, Repo Manager 31,
Code Pad 48). Their original argument DTOs and implementation remain in place;
the product host must perform caller/session/owner admission before dispatch.
Native-only initialization fixes one immutable generation and private common
snapshot/handoff/cache root per process. Standalone defaults retain their existing
paths. Workbench does not auto-absorb legacy profiles in component mode, and
Code Pad does not call its legacy identifier migration or start a language server.
Its existing manager/installer/watcher and main-window diagnostic/status events
are reused; product shutdown must await confirmed owned LSP tree termination.

Product-only cross-app/session calls currently report unavailable until B05/B06/B07
supply the corresponding owners. Native typed open delivery remains a hint to the
receiver's existing pending-open and dirty-document checks. Optional `standalone`
features keep the old executable bootstraps/web assets out of component-only builds.

## Actual verification

The shared package typecheck and 34 test files / 307 tests passed. The three
legacy entry builds and new Workspace build passed in the same focused run
(44.751 seconds, 8 GiB enforced, cgroup peak 2,588,184,576 bytes). These checks
cover extraction only; they do not establish native product parity.

The preserved native suites passed 475 tests. Typed-adapter strict Clippy passed
with default features in 16.050 seconds after unit-return/item-placement fixes.
Scoped initialization then passed default Clippy in 13.012 seconds, and the final
component-only (`--no-default-features --all-targets`) Clippy passed in 12.966
seconds. The generated notice changed only its pnpm-lock digest (1.949 seconds).
The final extraction/root/feature-gating state passed affected all in 735.109
seconds (8 GiB enforced; cgroup peak 6,444,986,368 bytes), including all frontend
build/test/typecheck and Rust check/Clippy/test/fmt gates. Native product startup
is still unimplemented and is not represented as passing.

## Remaining work

Native host admission/transport/storage activation, registry and trust, project/source/file
context and dirty transitions, Dependencies routing, importer/recovery, Windows
LSP preservation and WSL-native transport remain incomplete. Retained hidden
features still need keyboard/listener lifetime integration. B03 owns the shared
shell CSS containment change, which this branch will inherit on main integration.
Code Pad's legacy `apply_recovery` writes a renderer-provided path directly.
Component mode now requires a native recovery review instead of forwarding that
write; B04 must supply the identity-bound preview/apply workflow before claiming
recovery parity. The importer must also validate recovery/session/config schemas
strictly rather than using legacy empty-on-corruption convenience readers.
CI, actual Windows/WSL and packaged product acceptance remain unperformed.

The final shared transport/readiness-independent state passed the same 307 UI
tests, package typecheck, Workspace build and component-only strict Clippy in
50.540 seconds. Catalog virtual build edges now point to the shared package;
editor/catalog regression expectations include all reverse consumers. The scope
regression passed. The all-scope affected run covered this verifier adjustment
and the extraction/component changes together; Windows product startup remains
unimplemented and is not represented as passing.
