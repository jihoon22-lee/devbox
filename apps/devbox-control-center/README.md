# Devbox Control Center

v0.8 B01 development shell; domain migration and feature parity remain owned by the subsequent work package. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-control-center dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-control-center tauri dev` runs the native shell. Browser `?route=products` selects a preview route. The Windows debug executable accepts `--route=products` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.controlcenter` identity; no legacy data root is opened. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof must be recorded in B01 acceptance before claiming Windows parity.

B07 currently connects reviewed package identities, authenticated native peer
transport and per-product command metadata. Received route commands require the
destination's review and a separate UI acknowledgement. The shared legacy Launcher
is hosted as a lazy modal with product-native favorites and an explicitly enabled
shortcut owner; opening it retains the current product route. Domain/entity search,
Terminal/Capture bindings, cold launch, cross-product artifacts and preference
migration remain in the same unfinished B07 bundle. Detailed verification is
scheduled after that complete implementation; see the
[B07 workthrough](../../workthrough/2026-09-13-v08-b07-global-commands.md).
