# Workspace WSL helper resource

`devbox-workspace-wsl` is the first-party static Linux x86-64 helper built from
`apps/devbox-workspace/native`. The private CI artifact includes a bounded
`manifest.json` with source commit, protocol, size and SHA-256. Neither generated
file is committed. Windows packaging must validate both before invoking Tauri;
the build script embeds the verified digest and size for native launch checks.
A source-only Rust check can omit both files; that executable cannot launch WSL.

Build with `cargo build --locked --release -p workspace-wsl --target
x86_64-unknown-linux-musl`, then run `node .github/scripts/workspace-wsl-artifact.mjs
prepare <binary> <source-sha>`. `verify <source-sha>` validates the staged resource.
No Linux package or language server is installed at application runtime.
