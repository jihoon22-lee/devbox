# Repo Manager / Workbench WSL-native Git contract

Date: 2026-08-30  
Issue: [#482](https://github.com/jihoon22-lee/devbox/issues/482)

## Scope

This slice completes the remaining confirmed WSL Git defects in Repo Manager
and Workbench. It does not declare every native filesystem consumer to support
arbitrary POSIX input. Everything+, Knowledge Base, Code Pad, Run Manager, and
Log Lens retain their documented input/runtime boundaries and receive physical
Windows acceptance in the v0.6.0 release issue.

## Target and path contract

- `GitTarget::Native` retains a reviewed Windows drive or ordinary UNC cwd.
- `GitTarget::Wsl` owns a validated distro and absolute, case-preserving POSIX
  cwd. It invokes fixed `wsl.exe -d <distro> -- /usr/bin/timeout ... git -C
  <cwd>` argv through the existing bounded process-tree runner.
- `\\wsl$`, `//wsl$`, and `\\wsl.localhost` are transport aliases. The distro
  is compared case-insensitively, while every Linux path component keeps its
  original case.
- Relative Git metadata such as `.git/MERGE_HEAD` resolves inside the selected
  execution namespace. Absolute WSL worktree output converts to the same
  distro's host UNC; `/mnt/<drive>` converts to a drive path.
- A host path passed back to WSL Git must be either a drive path or a WSL UNC
  for the same distro. Ordinary UNC, cross-distro UNC, traversal, control
  characters, and Windows-unrepresentable Linux names fail closed.

Windows `std::fs::canonicalize` returns extended paths. Repo Manager rejects
extended/device syntax at the inbound IPC boundary, then permits only the two
trusted shapes that canonicalization itself can produce: `\\?\\C:\\...` and
`\\?\\UNC\\...`. Volume GUID and device namespaces remain rejected. This
normalization occurs before selecting a Git target and before returning a
canonical worktree path to the UI.

## Repo Manager behavior

Every repository Git surface uses the target-aware bounded runner: status,
safety markers, history/detail/diff, stage/unstage/commit, fetch/pull/push,
repository identity, cleanup preview/revalidation/apply, and worktree
list/create/remove. Git-emitted common-directory, marker, and worktree paths are
converted to host paths before filesystem identity, canonicalization, marker
checks, or UI serialization. Worktree create/remove converts reviewed host
targets back into the selected Git namespace only at the final argv boundary.

## Workbench behavior

The public multi-project Git status command derives a target per project path.
Life Log WSL UNC entries populate both the host path and a structured
`WslProfile`. Project health derives an explicit distro-scoped target even when
the stored Git root uses a drive spelling.

Before a WSL profile's Git status is read, Workbench runs only the bounded
`wsl.exe -l -v` observation. Missing, stopped, or unavailable distros produce a
stable health item and skip Git, preventing a read-only health refresh from
starting a distro. Cancellation and timeout continue to terminate and reap the
owned native process tree.

## Physical Windows acceptance

Test both `\\wsl$` and `\\wsl.localhost` aliases with spaces, Hangul, and
case-distinct Linux directories. In Repo Manager exercise status, history,
diff, selected stage/unstage/commit, fetch, fast-forward pull, upstream push,
linked worktree list/create/remove, cleanup preview, cancellation, and timeout.
Verify host UI paths never expose `/home/...` or `\\?\\...` spellings.

In Workbench import the Life Log snapshot, confirm distro/POSIX profile fields,
and check project health in Running, Stopped, Missing, and unavailable WSL
states. Stopped and missing cases must not start a distro or spawn distro Git.
After cancellation/timeout, confirm no owned `git`, hook/helper, `wsl.exe`, or
`timeout` process remains.
