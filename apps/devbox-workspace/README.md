# Devbox Workspace

v0.8 B01 development shell; domain migration and feature parity remain owned by the subsequent work package. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-workspace dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-workspace tauri dev` runs the native shell. `?route=overview` selects a route for development. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.workspace` identity; no legacy data root is opened. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof must be recorded in B01 acceptance before claiming Windows parity.
