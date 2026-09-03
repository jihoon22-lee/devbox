# Log Lens

Log Lens 0.2.0 is a bounded, offline log viewer for explicitly selected local
files/directories and fixed WSL, local-container, Run Manager, or one-time
Webhook Lab capture adapters. It parses plain text, JSONL, and logfmt lines,
merges them deterministically, and keeps only an in-memory ring (100,000 lines
or 64 MiB). The W08 PR2 surface also provides strict app-local saved views and
Korean controls without turning source data into a log archive.

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
- When Port Manager exposes a Run correlation with `logs_available`, its
  stdout/stderr Log Lens actions still use the same identity-only Run handoff.
  Port Manager's native action re-collects the listener and re-reads the
  current producer view before publishing it; Log Lens receives no listener
  path, command, environment, or log bytes and resolves the app-owned source
  from `{kind, sourceId, runId, stream}` only.
- Webhook Lab's `webhook-log/v1` handoff is a separate, one-time sanitized
  capture source. Its bounded payload contains only the HTTP method, a safe
  origin-form target, capture timestamp, header names (never values), and a
  redacted body preview up to 4 KiB with `redacted`/`truncated` flags. It has no
  filesystem path, command, environment, raw body, header value, or archive
  field. The source is read-only and ephemeral; it cannot be written to a saved
  view. The preview dialog never renders the body preview.
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
filters, never log text. The canonical wire `displayName` remains English
(`Run Manager handoff`, `Webhook capture`, and the other source names), even
though the W08 UI, reconnect notices, and saved-view controls are Korean.

- Saved views are stored only in the Log Lens app-local
  `saved-views.json`. Schema v1 contains `schemaVersion`, a monotonic
  `revision`, and at most 20 unique `{ name, sources, filter }` entries. The
  native boundary applies revision compare-and-swap, validates the complete
  document, writes atomically, and rejects links/reparse points. A corrupt,
  oversized, unknown-field, or unsafe store is preserved and reported with a
  fixed error; it is never replaced automatically. WSL file descriptors and
  ephemeral Webhook captures are not persistable sources.
- Loading a saved view changes only source configuration and filter state. It
  clears the in-memory records/cursors/bookmarks, marks the source as
  disconnected, and requires the user to press `source 재연결` before any read
  or follow refresh. Reconnect starts a fresh bounded read; there is no silent
  source access while a view is being loaded.
- `Send selected logs to Developer Toolbox` operates only on records explicitly
  selected in the current source generation. It builds a bounded deterministic
  export and publishes a one-time masked `toolbox-text/v1` handoff; stale
  selection or generation is rejected. It never expands to all visible records
  and never uses a clipboard fallback. Developer Toolbox must preview and apply
  the handoff explicitly.

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
UI. The AppLink wakeup listener is established before the cold one-shot slot
is pulled; a disposed registration neither consumes that slot nor leaves a
late listener active. The Run reader uses a decimal logical cursor, reports
retention rotation or truncation, and fails closed on linked/reparse paths,
malformed or overlapping segment ranges, and segment/count/output limits.

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

The Port Manager correlation and owner/Log Lens handoff path still has pending
packaged-Windows real acceptance; local tests and builds do not imply that
installed cross-app validation is complete.

W08 PR2 (#489) adds the `webhook-log/v1` Webhook Lab source, strict saved-view
persistence, disconnected saved-view loading, and Korean UI. The catalog
capability is revision 17 and the app target is Log Lens 0.2.0. Windows
packaged acceptance for the saved-view/reconnect and Webhook Lab→Log Lens
paths is still pending; this document does not claim a release or installed
acceptance.

The Log Lens bootstrap is included in the published v0.5.0 assets. The Run
reader was completed by #472/#473 after the v0.5.0 tag and is included in the
v0.5.1 stable release; it does not rewrite or replace the historical v0.5.0
assets. GitHub Release metadata is authoritative for the exact v0.5.1 tag,
workflow result, and asset digests.
