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

Explicit project selection rechecks native root/Git identity and binds the exact
Registry context to the product session. Stale revisions and replaced roots fail
without changing Registry bytes. Clearing selection grants no execution authority.
The shell refresh preserves the mounted feature subtree.

On Windows, Files now mounts the existing CodeMirror editor after native store
activation. A selected project or native file dialog admits file access; save,
rename and delete require an unchanged native document revision and filesystem
snapshot. Tabs remain mounted across routes, and dirty/busy/recovery states block
project switching. Session and recovery metadata are private to each context.
Recovery uses explicit native preview/apply tokens and preserves cancelled entries.

Project settings now inspect manifest/local-overlay definitions and their native
source snapshots, with explicit one-time approval, cancellation and revocation.
Changes to source files invalidate approval without changing editor context.

Project/local JSON edits and imports show native before/after definitions and
require explicit save. Concurrent changes preserve the original file and editor
draft. Saving revokes execution approval; shared export includes only the manifest.

Source/Dependencies, complete Git/LSP execution trust, legacy import and Windows/WSL LSP remain
incomplete. Basic Files and its new Windows fixture still require native execution
on the current change before being counted as verified product behavior.

The [B04 workthrough](../../workthrough/2026-09-08-v08-b04-workspace-source-files.md)
records actual extraction/build tests and the remaining Windows/WSL acceptance.
