# Workspace native WSL helper

Private Linux executable `devbox-workspace-wsl`, packaged with the Windows product.
The library without default features contains only the bounded pipe protocol.
The current executable observes and revalidates root/Git objects using the same
`filesystem::project::ProjectObservation` as the Windows adapter. It executes no
Git, language server or package manager and has no file mutation commands yet.

The inherited stdin/stdout protocol uses versioned, size-limited frames, random
session/request/root tokens, exact monotonic request sequences, deadlines and
expiring root observations. EOF, malformed input and deadline expiry terminate
this metadata-only helper, including a blocked metadata observation. Adding
child processes or writes requires a corresponding retirement/recovery owner.

Linux persistent evidence combines filesystem ID/type, inode and birth time from
retained descriptors. Live revalidation additionally checks device/inode handles,
root ancestry and Git pointer/backlink bytes. Windows binds this evidence to the
registered distro GUID, backing-directory object and WSL2 backing-image object. Missing birth-time evidence
fails closed. Names and paths alone are not persistent object identities.

`cargo test -p workspace-wsl` covers protocol bounds and root observations.
`cargo build --locked --release -p workspace-wsl --target
x86_64-unknown-linux-musl` produces the static artifact; see the
[resource instructions](../src-tauri/resources/wsl/README.md).
The Windows owner selects the native resource directory, verifies the compiled
size/hash, pins it read-only and pins every ancestor against replacement, then launches by distro GUID with structured argv.
Stopped distributions require an explicit start choice. Source-only Rust builds
have no executable helper unless the artifact was staged before compilation.
