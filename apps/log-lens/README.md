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
- Run Manager and WSL Desktop handoffs accept only the bounded `log-source/v1`
  source contract. Run payloads are the existing strict `{kind, sourceId,
  runId, stream}` identity; WSL payloads are an allowlisted `sourceType` plus
  validated distro and `wslPath`/unit. The AppLink argv carries only the
  opaque one-time envelope kind/id. After confirmation, the Run adapter maps
  that identity to the fixed Run Manager app-data root and reads only the
  selected app-owned stdout/stderr rotation segments in logical-offset order.
  A producer path, database row, command, environment value, or raw log is not
  accepted through the handoff boundary.
- The receiver re-checks protocol version, opaque envelope/claim identity,
  timestamps, lease bounds, target, producer, and source-family parity at the
  claim boundary. Native responses are schema-validated again in the frontend
  before a source can be added; a WSL journal with no unit is represented as
  an absent unit (both `undefined` and native `null` are accepted).

Source paths, commands, credentials, and environment values are never placed
in a snapshot or error string. A WSL path is present only in the bounded,
ten-minute one-time pending envelope and the in-memory adapter configuration;
it is not copied to the AppLink argv, clipboard, or a durable source/saved-view
record. A handoff is claimed only for an explicit preview, kept in process
memory during the modal, and acknowledged only after the user adds the source.
Terminal missing/expired/lease-expired claim errors clear stale preview state;
storage or restore failures retain the exact claim when one is held (or the
exact request ID for a claim retry) and expose at most three bounded recovery
attempts. Native errors are reduced to fixed public codes and never
show raw paths, payloads, or storage details. The viewer does not write a
permanent log archive. An explicit Export or Copy action operates on the
currently visible selection only; saved views contain source settings and
filters, never log text.

## Safety and lifecycle

Each line is capped at 16 KiB and each source read at 64 MiB. A 10-second
deadline, bounded process output reader, cancellation token, opaque operation
ID, generation check, and single-flight registry protect WSL/container reads.
The registry remains bounded even when a caller repeats one generation, and
adapter termination falls back to the direct child when process-tree cleanup
helpers fail. On Windows a Job Object with kill-on-close contains descendants;
on Unix the adapter uses a process group and bounded reap. Windows device
namespace paths and adapter-boundary whitespace are rejected before any read.
The handoff modal is also single-flight: while a preview, accept/discard
action, or bounded recovery is active, only the newest opaque request is
queued. Escape/Tab focus handling, opener restoration, unmount guards, and
generation checks prevent a stale native response from mutating the source
UI. The Run reader uses a decimal logical cursor, reports retention rotation
or truncation, and fails closed on linked/reparse paths, malformed or
overlapping segment ranges, and segment/count/output limits.

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

The Log Lens bootstrap is included in the published v0.5.0 assets. The Run
reader is the post-release main-branch completion tracked by #472; it does not
rewrite or replace the immutable v0.5.0 release assets.
