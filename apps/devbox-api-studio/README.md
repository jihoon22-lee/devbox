# Devbox API Studio

v0.8 B01 development shell; domain migration and feature parity remain owned by the subsequent work package. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-api-studio dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-api-studio tauri dev` runs the native shell. Browser `?route=requests` selects a preview route. The Windows debug executable accepts `--route=requests` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.apistudio` identity; no legacy data root is opened. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof must be recorded in B01 acceptance before claiming Windows parity.
