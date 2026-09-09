# Workspace native WSL helper

Private Linux executable `devbox-workspace-wsl`, packaged with the Windows product.
The library without default features contains only the bounded pipe protocol.
The `files` feature contains the file grants, conflict checks, Windows path
admission and protected-storage policy shared by the Windows host and Linux
helper. Tauri storage discovery stays in the Windows host.
The executable observes/revalidates root/Git objects and opens files only after
attaching one native project context to an observed root. It executes no Git,
language server or package manager. Explicit saves require the current native
revision and disk snapshot and reuse Code Pad's encoding/CRLF atomic replacement.
Rename/delete require the same native revision and precommit authority/cancellation
checks. Linux rename uses `renameat2(RENAME_NOREPLACE)` where available. WSL1's
native wslfs returns ENOSYS; only that filesystem uses non-overwriting hard-link
publication followed by guarded removal of the original name, relative to the
retained parent descriptor. It checks authority, bytes and both identities again
between steps. Cancellation or failure preserves both names and reports that
reconciliation is required; hard process termination can likewise leave two names
for the same file. This compatibility path is not an atomic namespace rename and
never automatically retries or deletes the destination during rollback. Other
unsupported Linux filesystems still fail before publication.

The inherited stdin/stdout protocol uses versioned, size-limited frames, random
session/request/root tokens, exact monotonic request sequences, deadlines and
expiring preview observations. Attached editor roots remain owned until release
or EOF so idle time cannot discard open document revisions. Every operation still
revalidates the native objects. EOF, malformed input and deadline expiry cancel active
work. Saves check cancellation and native authority before staging and immediately
before replacement. Temporary files retain their parent/file identities; cleanup
removes only the still-owned staging file. A blocked syscall has a five-second exit
deadline: disk contents remain either the original or the complete replacement,
but a hard exit may leave staging data. The Windows owner drains abandoned replies
while retiring the helper and never replays a save. Adding child processes requires
a corresponding process-retirement owner.

Linux persistent evidence combines filesystem ID/type, inode and birth time from
retained descriptors. Live revalidation additionally checks device/inode handles,
root ancestry and Git pointer/backlink bytes. Windows binds this evidence to the
registered distro GUID, backing-directory object and WSL2 backing-image object.
WSL1 wslfs (`0x53464846`) has no statx. Only ENOSYS on that filesystem admits its
native NTFS file ID, including the nonzero sequence in the high 16 bits, instead
of birth time. Other missing/invalid birth-time evidence fails closed. An owned
Windows/WSL1 probe verified that this inode equals the backing file's Windows ID,
survives a distro restart and differs after replacement. Native WSL1 also lacks
`/proc/self/fdinfo`; exact device/fsid/root checks and unambiguous mountinfo remain
required when descriptor mount IDs are absent. See [NTFS references](https://learn.microsoft.com/en-us/windows/win32/devnotes/mft-segment-reference)
and [wslfs identification](https://www.gnu.org/software/coreutils/filesystems.html).
Windows retains the registration key and compares a bounded digest
of all its values; LastWriteTime only checks read consistency. Identical value
rewrites do not replace identity, but changed policy values or a deleted key do. Names and paths alone are not persistent object identities.

File admission checks retained metadata, this distro's root-filesystem device/ID
and the effective native mount type. drvfs/9p, other mounted filesystems, links,
devices and another project context cannot become file access. Temporary filesystems
outside the distro root filesystem are currently unsupported. File reads use the
same Windows/Code Pad encoding, size and native revision contract. Buffer sync is
metadata only. Explicit saves rotate the native revision; old revisions cannot
repeat the mutation. Dropping a connection during a save may leave its complete
replacement committed without acknowledgement, so the caller must reopen/reconcile
before another save. Dropping a connection never saves buffered text on its own.
The product WSL Files owner now delegates these methods and retains only acknowledged
revision/path metadata in Windows. Its sessions and recovery buffers remain in the
private Windows store; native Windows chooser files retain their own grants.

Quick Open and Markdown/Mermaid preview reuse the Code Pad core without Tauri.
The guarded listing is also used by Windows Files: it admits each directory before
reading children, retains directory identity, checks cancellation and applies
50,000-file/400,000-entry/128-depth/8-MiB output bounds. Denied paths mark the result
incomplete. Preview admits each local image before reading and after completion;
cancelled previews return no result. Existing sanitization, image limits and
remote-image rendering behavior remain in the shared Markdown renderer.

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

`files_poll` accepts only opened paths and uses fresh leaf evidence under the original
parent/root authority. It reports metadata without replacing the editor's file grant
or acknowledging dirty text. `files_recover` consumes the currently reviewed native
revision and uses the same guarded encoding/CRLF save. The Windows owner forwards
the remaining user deadline; metadata-only cleanup never creates a helper. Explicit
shutdown confirms launcher exit and can be retried; application exit retains any
owner whose retirement could not be confirmed.

Registry `Version` is the distribution filesystem format, not WSL1 versus WSL2.
Mode and VHD admission use `Flags & 0x8` (`LXSS_DISTRO_FLAGS_VM_MODE`), matching
[Microsoft's enumeration implementation](https://github.com/microsoft/WSL/blob/03f6b0e5dd8bdbcb90406813699f616534a25eb3/src/windows/service/exe/LxssUserSession.cpp#L1049).
A modern WSL1 registration has filesystem Version 2 without a VHD. WSL2 still
requires the registered/default VHD filename and its retained native identity.
