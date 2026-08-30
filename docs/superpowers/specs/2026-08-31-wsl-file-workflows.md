# WSL file workflows and initial bundle contract

Date: 2026-08-31  
Issue: #490  
Targets: Everything+, Code Pad, Knowledge Base, `crates/wsl`, frontend CI

## Goal

Windows Tauri apps must distinguish a WSL-native path from an ordinary UNC
share, preserve Linux path case, and expose the actual watcher/LSP capability
instead of turning an unsupported subsystem into a generic file error. A WSL
distribution going offline or a bounded scan becoming incomplete must preserve
the last known-good derived index.

This work does not introduce a shell protocol, a long-running process inside a
distro, or a new cross-app payload. File bytes continue to cross only the local
Windows/WSL filesystem provider. No network fetch is needed for scan, edit,
preview, watcher fallback, or bundle verification.

## Shared WSL path identity

The shared parser accepts the user-facing `\\wsl$\<distro>\...` and
`\\wsl.localhost\<distro>\...` aliases plus the
`\\?\UNC\wsl.localhost\...` spelling returned by Windows canonicalization.
It validates the distribution name and Linux components without starting WSL.

- transport alias and distribution name: ASCII case-insensitive;
- Linux path tail: case-sensitive and Unicode-preserving;
- spaces and Korean characters: retained as ordinary path data;
- empty tail, traversal, control characters, unsafe distro name, and oversize
  values: rejected;
- WSL containment: component boundary only, never a lower-cased string prefix.

Only this identity/containment rule is shared. The three applications retain
their separate watcher lifetimes and mutation models.

## Everything+ root lifecycle

### Registration and initial convergence

An existing canonical WSL UNC directory is a supported search root. A new root
is scanned immediately; every persisted root also requests one reconciliation
on application restart so changes made while Everything+ was closed converge.
Native roots keep recursive `notify`; WSL roots use bounded metadata polling.

One root scan keeps at most 250,000 regular-file metadata records and commits
250 records per SQLite transaction. Before clearing any derived row, the scan
must prove all of the following:

1. the root is still an ordinary directory with no link/reparse component;
2. traversal did not hit the file-count bound;
3. traversal did not encounter an unreadable directory entry or metadata row;
4. the registered root and content policy still match the scan snapshot.

If proof is incomplete, scanned additions/updates may be applied, but absence
is not treated as deletion and old rows are not cleared. The root reports one
of `root_unavailable`, `root_scan_limit`, or `root_scan_incomplete`; raw paths
and OS errors are not echoed.

### Watcher and polling bounds

- callback channel: 1,024 messages;
- paths accepted from one native event: 256;
- pending debounced paths: 4,096;
- ready batch: 512;
- path size: 32 KiB;
- WSL metadata poll: every 5 seconds;
- one polling snapshot: the same 250,000-file root bound.

A callback error, channel overflow, path/debounce overflow, or polling diff
overflow records only the owning root in a bounded set and requests one full
reconciliation. An event received during a full scan is coalesced to a queued
root restart rather than rearmed in a busy loop. An incomplete polling snapshot
can identify additions/updates but never deletions.

The UI exposes `sourceKind` (`native`/`wsl`), `watchMode`
(`native`/`polling`/`unavailable`), pending count, last sync time, and a stable
error code. WSL polling is presented as an intentional capability, not as
native realtime watching.

## Code Pad capability boundary

Code Pad continues to edit and atomically save files exposed through WSL UNC.
Open-document external-change detection uses bounded polling for WSL parents
and native notify elsewhere. Workspace/file containment uses the shared WSL
case rule.

Language servers launched by the Windows host are not claimed to support a WSL
workspace. Code Pad reports the workspace capability explicitly:

| Capability | Native root | WSL root |
|---|---|---|
| file read/edit/atomic save | supported | supported through local UNC provider |
| external-change watcher | native | bounded polling |
| host LSP | supported when configured | unsupported, explicit reason |

The LSP manager also rejects a direct/manual WSL start with the same stable
reason so bypassing the frontend cannot mislabel a process-start failure as a
file read failure. Running an LSP inside WSL is a separate future protocol and
is not silently emulated with a shell command.

## Knowledge Base WSL vault

An existing validated WSL UNC directory may be used as the vault. Normal editor
writes use a uniquely named sibling temporary file, flush/sync it, and replace
the target atomically. WSL vaults use bounded polling; native vaults keep
recursive notify.

Polling/reconciliation applies Markdown additions and updates. Deletions are
removed from SQLite only after a complete authoritative scan. On overflow,
unreadable subtrees, root replacement, or an offline distro, prior rows remain
and the watcher reports a stable status. A successful change emits one
`docs-changed` event and republishes the privacy-safe activity snapshot.

## Mermaid and bundle budget

Code Pad and Knowledge Base must not load Mermaid in the initial editor chunk.
The first preview containing a Mermaid block dynamically imports and initializes
the renderer in strict security mode. Concurrent previews share one import and
initialization promise; existing last-good-SVG and syntax-error behavior stays
unchanged.

CI builds the affected frontend before reading its generated `index.html`,
resolves only initial module scripts, and compares their raw and gzip sizes to
checked-in per-app budgets. Lazy chunks are reported but do not count as initial
entry bytes. Missing output, path escape, duplicate entry, and budget increase
fail closed.

## Automated acceptance

- WSL aliases and extended canonical UNC converge to one identity.
- `DevBox` and `devbox` below the same distro remain different roots/files.
- spaces and Korean characters round-trip without quoting or shell execution.
- missing/incomplete/over-limit scans do not clear last known-good rows.
- watcher callbacks and poll diffs stay within all declared bounds.
- restart requests one reconciliation and overflow coalesces by root.
- Code Pad displays edit/watcher/LSP capabilities independently.
- Knowledge saves use atomic sibling replacement and incomplete scans retain
  deletion candidates.
- browser tests run offline and Mermaid is absent from initial entry chunks.
- Windows compile CI covers platform code; packaged Windows+WSL runtime evidence
  remains an explicit #493 acceptance gate.
