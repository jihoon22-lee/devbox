# Log Lens

Log Lens 0.1.0 is a bounded, offline log viewer for explicitly selected local
files/directories and fixed WSL or local container adapters. It parses plain
text, JSONL, and logfmt lines, merges them deterministically, and keeps only an
in-memory ring (100,000 lines or 64 MiB).

## Source boundary

- Local file and directory pattern reads are read-only. Directory patterns are
  filename-only globs and are capped at 256 files; omitted matches and
  unreadable members are reported as a truncated/partial source.
- WSL sources use only `wsl.exe -d <distro> -- cat -- <path>` or the fixed
  `journalctl --no-pager --output=short-iso` argv. No shell or arbitrary command
  is accepted.
- Container sources use only `docker logs --timestamps --tail 100000 <id>` or
  the equivalent Podman argv. The adapter does not start, stop, or pull a
  container and does not ingest a network endpoint.
- Run Manager handoff accepts only the opaque `log-source/v1` identity. The
  producer claim/ack integration is intentionally a follow-up PR after this
  receiver bootstrap.

Source paths, commands, credentials, and environment values are never placed
in a snapshot or error string. The viewer does not write a permanent log
archive. An explicit Export or Copy action operates on the currently visible
selection only; saved views contain source settings and filters, never log
text.

## Safety and lifecycle

Each line is capped at 16 KiB and each source read at 64 MiB. A 10-second
deadline, bounded process output reader, cancellation token, opaque operation
ID, generation check, and single-flight registry protect WSL/container reads.
The registry remains bounded even when a caller repeats one generation, and
adapter termination falls back to the direct child when process-tree cleanup
helpers fail. Windows device namespace paths and adapter-boundary whitespace
are rejected before any read.

Parser timestamps accept RFC3339, journal-style numeric offsets, fractional ISO
forms, and timezone-less local forms on a best-effort basis. Browser fixtures
use the same opaque-ID ordering as native merge; potentially catastrophic
JavaScript regex constructs fail closed.
File cursors retain a platform file identity and offset, allowing append while
detecting replacement or truncate. Evicted records and bytes are reported as
backpressure metadata.

## Development

```bash
pnpm --filter log-lens test
pnpm --filter log-lens build
source ~/.cargo/env
CARGO_TARGET_DIR=/tmp/devbox-log-lens-target CARGO_INCREMENTAL=0 cargo test -p log-lens -j2
```

The packaged Windows W3 smoke remains necessary for installed WSL and
Docker/Podman availability, native file identity semantics, and download,
clipboard, focus, and IME behavior.

The catalog entry is enabled for a future catalog-driven v0.5.0 release; this
bootstrap is not part of the already published v0.4.2 stable assets until its
W3 evidence is accepted.
