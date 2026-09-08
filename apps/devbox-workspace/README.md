# Devbox Workspace

Hidden v0.8 B04 development product. The shared Overview/Source/Files UI and native component adapters are being integrated; product authority, registry, migration and LSP acceptance remain incomplete. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-workspace dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-workspace tauri dev` runs the native shell. Browser `?route=overview` selects a preview route. The Windows debug executable accepts `--route=overview` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.workspace` identity; no legacy data root is opened. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof remain separate from the focused Registry/storage checks.

The existing Workbench, Repo Manager and Code Pad UI/tests live in
`packages/workspace-features` and are consumed by their legacy entry points too.
Workspace lazily loads the three slices and retains visited routes. Other product
routes explicitly remain unavailable. The native engines expose typed component
adapters and opt out of standalone bootstrap/web assets for product consumers.
Native-only initialization selects immutable per-process data/snapshot/handoff/cache
roots; it neither invokes old identifier migration nor starts a language server.
The product host now authenticates explicit startup and Registry commands. Its
native startup screen creates a blank generation only after a user action and
shows schema/owner failures without replacing existing data. Registry UI supports
native folder preview/cancel, explicit registration/rebind, rename and reviewed
metadata removal. Registration grants no execution trust. Blocking probes use a
separate bounded worker pool and cannot consume all metadata workers.

On Windows the current native view is this startup/Registry screen. The shared
Overview/Source/Files remain available as browser previews while domain admission,
context/dirty transitions, importer/recovery and LSP wiring are completed. The
native integration fixture exercises the actual startup/registration UI and
command roles; Windows execution of the latest host must pass before parity is
claimed.

The [B04 workthrough](../../workthrough/2026-09-08-v08-b04-workspace-source-files.md)
records actual extraction/build tests and the remaining Windows/WSL acceptance.
