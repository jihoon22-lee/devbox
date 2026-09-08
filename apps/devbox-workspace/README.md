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

Dependencies now lazily mounts the shared local parser and review UI for the
selected native project, including plain folders without Git. Analysis does not
launch package managers or Git. Remote OSV/deps.dev requests consume a one-time
context/input-bound review, with cancellation and a 30-second request budget.
Summary/cache writes use the private generation; corrupt/future cache files are
preserved, and a missing generation is never recreated by cache publication.

Source now offers separate native Git approval and the shared Git panels. It
pins the launcher/core executable, config/include/hook evidence and native
worktree arguments before execution. Changed evidence requires review. Git and
editor disk operations coordinate while private recovery writes remain usable.
Commit drafts survive permission refreshes and block project switching. Generic
file editing cannot rewrite the products' native Registry/approval stores.

Source changes and diff lines open the retained Files editor. Worktree creation
uses a one-time native target review, then proposes Registry registration and
selection separately. Cancellation begins before queueing/evidence capture, and
local drive aliases are resolved before filesystem IO. Cleanup has a separate native review of explicitly selected registered siblings.
Its preview/apply checks preserve existing safety rules and block folders with
open native documents. Revocation works without contacting a sibling. Legacy
import, complete Windows LSP integration and WSL-native Git/LSP remain incomplete. Current Source,
Dependencies and Files packaged behavior still requires Windows acceptance.

The [B04 workthrough](../../workthrough/2026-09-08-v08-b04-workspace-source-files.md)
records actual extraction/build tests and the remaining Windows/WSL acceptance.
