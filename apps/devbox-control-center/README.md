# Devbox Control Center

v0.8 B01 development shell; domain migration and feature parity remain owned by the subsequent work package. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-control-center dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-control-center tauri dev` runs the native shell. Browser `?route=products` selects a preview route. The Windows debug executable accepts `--route=products` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.controlcenter` identity; no legacy data root is opened. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof must be recorded in B01 acceptance before claiming Windows parity.

B07 connects the actual Launcher, bounded native search providers, four managed
shortcuts, explicit artifact reviews and shared Operations to the four products.
Package approval pins the installation's manifest and executable identities;
remembered cold activation and native peer checks retain that exact namespace.
Launcher preferences migrate by exact opaque IDs with unresolved entries preserved.
Operations reports owner state and opens owner reviews without acquiring execution
or cancellation authority. The native four-product fixture and one PR audit own
acceptance; implementation commits alone are not acceptance evidence. See the
[B07 workthrough](../../workthrough/2026-09-13-v08-b07-global-commands.md).
