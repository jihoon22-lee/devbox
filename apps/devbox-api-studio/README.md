# Devbox API Studio

v0.8 B02 API Studio implementation in progress; persistent migration and integrated handoff acceptance remain open. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-api-studio dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-api-studio tauri dev` runs the native shell. Browser `?route=requests` selects a preview route. The Windows debug executable accepts `--route=requests` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.apistudio` identity; no legacy data root is opened. Navigation retains mounted route drafts in memory, with bounded history. B01 Windows installation identity evidence is recorded in the foundation workthrough. B02 native feature/runtime parity requires its own Windows acceptance.

Requests/Protocols, Webhooks and Transforms consume the shared frontend feature package and native adapters. See the [B02 workthrough](../../workthrough/2026-09-07-v08-b02-api-studio.md) for actual checks and remaining work.

Native dependencies disable the legacy crates' default `standalone` feature.
API Studio therefore embeds only its own frontend bundle. The three legacy apps
still build and run with their default features. Native OAuth uses the initialized
system-browser plugin through its validated API component; the renderer receives
no additional opener capability.
