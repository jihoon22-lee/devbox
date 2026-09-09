# Workspace native WSL helper

Private Linux executable `devbox-workspace-wsl`, packaged with the Windows product.
The library without default features contains only the bounded pipe protocol.
The `files` feature contains the file grants, conflict checks, Windows path
admission and protected-storage policy shared by the Windows host and Linux
helper. Tauri storage discovery stays in the Windows host.
The executable observes/revalidates root/Git objects and opens files only after
attaching one native project context to an observed root. It executes no Git,
language server or package manager and exposes no file mutation commands yet.

The inherited stdin/stdout protocol uses versioned, size-limited frames, random
session/request/root tokens, exact monotonic request sequences, deadlines and
expiring root observations. EOF, malformed input and deadline expiry terminate
this read-only helper, including a blocked observation. Adding
child processes or writes requires a corresponding retirement/recovery owner.

Linux persistent evidence combines filesystem ID/type, inode and birth time from
retained descriptors. Live revalidation additionally checks device/inode handles,
root ancestry and Git pointer/backlink bytes. Windows binds this evidence to the
registered distro GUID, backing-directory object and WSL2 backing-image object. Missing birth-time evidence
fails closed. Windows retains the registration key and compares a bounded digest
of all its values; LastWriteTime only checks read consistency. Identical value
rewrites do not replace identity, but changed policy values or a deleted key do. Names and paths alone are not persistent object identities.

File admission checks retained metadata, this distro's root-filesystem device/ID
and the effective native mount type. drvfs/9p, other mounted filesystems, links,
devices and another project context cannot become file access. Temporary filesystems
outside the distro root filesystem are currently unsupported. File reads use the
same Windows/Code Pad encoding, size and native revision contract. Buffer sync is
metadata only; dropping the connection cannot write an unsaved buffer to disk.
These helper methods are not yet connected to the product's WSL Files route.

`cargo test -p workspace-wsl` covers existing file-owner regressions, native mount
admission and actual subprocess framing/context/replay/EOF behavior.
`musl-tools` is needed for native C dependencies.
`cargo build --locked --release -p workspace-wsl --target
x86_64-unknown-linux-musl` produces the static artifact; see the
[resource instructions](../src-tauri/resources/wsl/README.md).
The Windows owner selects the native resource directory, verifies the compiled
size/hash, pins it read-only and pins every ancestor against replacement, then launches by distro GUID with structured argv.
Stopped distributions require an explicit start choice. Source-only Rust builds
have no executable helper unless the artifact was staged before compilation.
