# Devbox Control Center

v0.8 Control Center under development (B07 command host and B08 installation/recovery). Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-control-center dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-control-center tauri dev` runs the native shell. Browser `?route=products` selects a preview route. The Windows debug executable accepts `--route=products` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.controlcenter` identity and an installation-specific namespace. Manager diagnostics read the original Manager root explicitly; new product writes remain in the new namespace. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof must be recorded in B01 acceptance before claiming Windows parity.

B07 connects the actual Launcher, bounded native search providers, four managed
shortcuts, explicit artifact reviews and shared Operations to the four products.
Package approval pins the installation's manifest and executable identities;
remembered cold activation and native peer checks retain that exact namespace.
Launcher preferences migrate by exact opaque IDs with unresolved entries preserved.
Operations reports owner state and opens owner reviews without acquiring execution
or cancellation authority. The native four-product fixture and one PR audit own
acceptance; implementation commits alone are not acceptance evidence. See the
[B07 workthrough](../../workthrough/2026-09-13-v08-b07-global-commands.md).

B08 embeds Manager Environment, Data Inspector, diagnostics and reviewed package-only
Related Tools. Products/Components distinguish verified package files, absent members,
and unknown runtime or installer state. Legacy ARP observations use the pinned v0.7
**installed-file** reference; public portable executables have different bytes.
Inventory alone does not authorize cleanup, and the old uninstaller is never invoked
against a user's installation to bypass exact ownership checks.

The private `devbox-suite-bootstrap` currently implements these Windows operations:

| Operation | Result |
|---|---|
| `--stage ROOT PAYLOAD` | Verify exact archives and stage a newly owned product directory |
| `--prepare-install ROOT PAYLOAD` | Prepare an isolated generation, retained journal and Import marker |
| `--recover-install ROOT PAYLOAD` | Block and recover an uncommitted first installation while preserving all data and packages |
| `--restart-install ROOT PAYLOAD` | Start a fresh package generation after that recovery; preserve the old journal and files |

`PAYLOAD` is the private `suite-payload.json` from `build-suite-package.py`, with its
four verified ZIPs beside it. The bootstrap verifies its own payload identity first.
Recovery/restart require all product writers to have closed; they do not terminate
processes by name. A committed installation or an update with a previous generation
requires a separate data recovery plan and is rejected by the first-install commands.
These are development operations, not a completed public installer or update flow.

Product migration summaries report setup/review/busy state through authenticated
native owners. They do not authorize activation. Import mode admits only the closed
owner importer/review methods and keeps schedules and ordinary business writes off.
Owner backup/import coordination, health/commit, installed update/downgrade,
shortcuts/ARP/uninstall and reviewed legacy cleanup remain part of B08's unfinished
implementation. See the [B08 workthrough](../../workthrough/2026-09-13-v08-b08-suite-recovery.md)
for actual evidence and remaining acceptance.

Local tests must preserve existing services and host networking. Provisioning and
network-changing Windows/WSL/Docker fixtures run only on disposable hosted VMs under
[verification operations](../../docs/verification.md). Per-commit checks remain
minimal; detailed acceptance runs after the entire PR bundle is implemented.
