# Workspace native WSL helper

Private Linux executable `devbox-workspace-wsl`, packaged with the Windows product.
The library without default features contains only the bounded pipe protocol.
The `files` feature contains the file grants, conflict checks, definition schema
and snapshots, Windows path admission and protected-storage policy shared by the Windows host and Linux
helper. Tauri storage discovery stays in the Windows host.
The executable observes/revalidates root/Git objects and opens files only after
attaching one native project context to an observed root. Reviewed Source operations
can execute Git through the native process owner described below. Language servers
and package managers are not exposed by this pipe. Explicit saves require the current native
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
expiring preview observations. Attached file/definition roots remain owned until release
or EOF; Windows preview expiry releases its owner. Every operation still
revalidates the native objects. EOF, malformed input and deadline expiry cancel active
work. Saves check cancellation and native authority before staging and immediately
before replacement. Temporary files retain their parent/file identities; cleanup
removes only the still-owned staging file. A blocked syscall has a five-second exit
deadline: disk contents remain either the original or the complete replacement,
but a hard exit may leave staging data. The Windows owner drains abandoned replies
while retiring the helper and never replays a save. Sessions prepared for child
execution retain their owner until native process retirement is acknowledged.

The private `--supervise -- <absolute program> <argv...>` entry point now provides
that owner for Git execution and the upcoming LSP transport. It starts before helper threads, sets
Linux subreaper and parent-death notification, and inherits the caller's explicit
cwd/environment/stdio. Each job has its own owner. After root exit or cancellation,
it discovers candidates through `/proc`, proves direct-child ownership with
`waitid(WNOWAIT)`, signals only still-unreaped children and collects adopted detached
descendants. It exits only after `ECHILD` confirms retirement; failures retain the
owner rather than killing unrelated processes. It needs neither pidfd nor WSL2's
children file. This CLI is not exposed through the pipe protocol. Native Git's
opt-in execution policy can reexecute the mapped `/proc/self/exe` image and request
SIGTERM retirement. Its ordinary Linux process-group path also keeps the root PID
unreaped until the final group signal. Windows Job ownership is unchanged.

Actual owned WSL1 and WSL2 fixtures verified normal exit, double-fork/setsid cleanup,
cancellation without touching another job, parent death and mapped-image reexecution.
The Rust integration tests cover these behaviors locally and in the helper CI job.
Git pipe admission and cancellation-aware shutdown now use that owner. LSP transport
remains incomplete; the file-only session watchdog is unchanged.

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

Project definition reads and trust review now retain native Linux bytes and objects
through `definitions_attach/read/validate`. They share the Windows manifest schema,
130-file/1,024-object bounds, 2-MiB source limit and 8-MiB total snapshot limit.
Each ancestor/content read checks native filesystem admission and cancellation;
validation checks exact bytes and the first absent component of optional files.
Windows keeps local overlays in its private generation and compares both owners
before committing one-use Registry trust. Reading/approving does not execute sources.

`definitions_write` consumes the reviewed manifest owner before IO. The candidate
must pass the shared 256-KiB strict schema. Existing manifests use Code Pad's
bounded encoding/CRLF conflict checks with a source/authority precommit guard.
First creation uses retained directory descriptors, exclusive parent creation and
non-overwriting complete-file publication. Only a newly created parent's retained
identity can replace the reviewed absence. Concurrent creators and links fail;
cancellation removes only unchanged owned staging files. Failure can leave an
empty .devbox directory or a complete, unacknowledged manifest; it never retries.
The Windows host revokes execution trust before writing and coordinates definition
IO with Git/editor filesystem permits. New trust still requires a fresh review.

The `git` feature now shares the bounded Git configuration/include/hook/executable
snapshot traversal between the Windows host and native helper. Windows keeps its
captured native environment resolver; Linux uses a frozen, case-sensitive
snapshot, skips relative/linked/foreign PATH candidates and reads an executable
without probing it. HOME and optional global/XDG configuration are native paths.
Absent explicit overrides, system configuration is pinned through GIT_CONFIG_SYSTEM
to /etc/gitconfig for /usr, or the selected local prefix's etc/gitconfig. Transient
launcher cwd/depth/interop variables are omitted; their values never enter approval
records. The approved command environment otherwise remains frozen.

Private `source_capture` and `source_validate` retain at most four 48-MiB Git
snapshots per pipe, with the existing object/include/hook limits. File, definition
and Git owners must share the same context; reattaching cannot erase changed
sources. Windows stores only the combined native Git/definition approval digest
and display projection, then revalidates both owners before approval. Changed
hooks/configs/tools require fresh review. No Git command runs from these methods.
Source execution uses the child-aware session retirement/cancel bridge below.
The hosted Windows fixture provisions Git only in its owned disposable distro and
checks review/approval/revoke/reconnect evidence with synthetic repositories.


`execution_prepare` first confirms that this session will acknowledge retirement,
even if a subsequent command frame is only partially received. `source_execute`
admits the closed set of basic Repo Manager Source operations against its retained
Git review and current native root. Before each actual Git command, a unique framed
admission ticket asks Windows to revalidate the private approval, Registry context,
project definitions and original request deadline. A wrong, stale or repeated ticket
closes the session. Linux checks its own evidence again after the reply. Unix file
mode is part of review evidence, so making an unchanged hook executable revokes trust.

The helper reuses Repo Manager with default features disabled and Tokio's existing
runtime; the static Linux dependency graph contains no Tauri/GTK/WebKit runtime.
Git executes with the reviewed environment and per-command subreaper owner. EOF,
timeout and explicit cancellation signal the active operation but retain every
blocking worker and owned descendant. The response waits for all command owners;
the final retirement frame is emitted only after future admission is closed.
Windows retains its context/filesystem permits and partial-frame decoder while
waiting for that proof and natural launcher exit. It cannot treat killing wsl.exe
as proof of Linux child retirement. An unconfirmed retirement keeps the owner held.

Real subprocess regressions cover selected stage/commit, denied approval, forged
tickets, partial command EOF and cancellation with a detached hook descendant.
The Windows fixture passed at `3011ef9` in 11.07 seconds, covering the full Source
host boundary, approval revocation and retained filesystem permits. Its general
and product CI runs both completed successfully.


`source_worktree_preview` retains the existing native parent and missing target
without creating either a directory or Git command. The shared Windows/Linux
worktree target owner checks the selected filesystem, Git-metadata exclusions,
parent identity and target absence. The Windows preview retains that exact helper
connection and issues its own one-use token. Creation consumes both tokens, checks
the original approval and destination before and after each command admission,
then reuses Repo Manager's reviewed worktree creation. Failed or cancelled creation
is never automatically retried or cleaned. Registration is a separate explicit
proposal with the same WSL distro; it does not automatically start a stopped distro.

Actual Linux tests cover creation, preserved unrelated files, token replay and a
destination created while approval is pending. The expanded Windows fixture also
registers the linked worktree and stages/commits in its own context; compilation
passed but that new actual Windows result remains pending.

WSL cleanup now uses the existing Repo Manager preview and non-force removal.
Windows admits at most eight explicitly reviewed, registered siblings of the same
project/repository/distro, with a combined 64 MiB Git-evidence budget. Only this
stored approval supplies the native member list; renderer paths cannot extend it.
Linux independently captures those roots, compares common-repository object and
Git environment, and binds each command to its retained root evidence. Unknown
Git-returned worktree paths fail before filesystem IO. Windows rechecks scope
revocation on every command and member definitions/open editor documents before
sibling admission. An open sibling document stops the scope operation with the
existing close-tabs message. Native retirement still owns the entire request.

Actual Source-pipe tests cover approved cleanup, unapproved siblings, foreign
repository/context, duplicate descriptors, changed evidence and late denied approval.
The Windows fixture adds real Files open/close and cleanup-scope revocation; its
actual hosted execution remains pending.

`dependency_inventory` requires the attached project context and uses Repo Manager's
bounded offline parser. Native filesystem admission precedes directory enumeration,
metadata and file reads; blocked subtrees yield a bounded partial report. Collection
has its own budget of at most ten seconds within the overall pipe deadline, leaving
room to serialize and return a truncated result. No Git, package manager, script,
network call or Linux cache publication occurs. Windows validates report paths,
coordinates, graph/count bounds and revision before publishing its private summary.
The same native reader revalidates lock revisions for existing remote-review tokens.

The Windows connection's project-method gate is now shared with actual pipe tests.
This fixes `fd48dd3`'s actual Windows rejection of `source_worktree_preview`, which
Linux-only dispatch tests had missed. Source/Dependency pipe tests and MSVC compile
checks passed after the correction; new actual Windows results remain pending.
