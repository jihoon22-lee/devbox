# WSL Desktop Runtime Snapshot Producer Workthrough

- Date: 2026-08-26
- Issue: #410 `feat(wsl-desktop): runtime snapshot producer`
- Branch: `feat/wsl-desktop/runtime-snapshot-producer`
- Base: `0228a173a8e52688dcecd8748d3eee1662c31a8f` (`main`)
- Target: WSL Desktop 0.4.0 / v0.5.0 P1-05, P1-09 runtime prerequisite
- Status: implementation and PR-wide local verification complete; GitHub CI/Windows W1 pending

## Outcome

WSL Desktop now owns the producer side of `snapshot:wsl-desktop/runtime/v1`. It publishes one
complete `summary.json` under the common integration root so Workbench #281 can later consume
runtime suggestions without reading WSL Desktop state or running WSL/Docker commands itself.
The producer is intentionally native to the app: it uses the installed WSL and Docker CLI only as
the external systems being observed, and does not download, install, start, stop, restart or
configure them.

## Existing boundaries read before implementation

- `CONVENTIONS.md` requires one versioned read-only snapshot per producer, `Envelope::with_views`,
  `write_atomic`, bounded payloads and no raw credentials/environment values.
- `crates/integration` owns `%LOCALAPPDATA%\\devbox\\integration\\<producer>/v<n>/summary.json`,
  envelope/view validation, freshness metadata, path/link safety, 10MiB file bound and atomic
  replacement. The producer does not duplicate those responsibilities.
- WSL Desktop's `SessionState` owns active PTY handles keyed by runtime session ID. The producer
  reads only a bounded count grouped by the validated distro; session ID, pane key, cwd, title,
  command and PTY resources never cross the snapshot boundary.
- Existing `dashboard.rs` remains the interactive Docker detail path (`id/name/image/status/ports`)
  for the WSL panel. The new producer uses a separate minimal format and does not change the
  detail DTO or Docker action behavior.
- `#281` explicitly excludes producer, WSL/Docker commands, Docker mutation, resource startup and
  Workbench changes. This worktree does not modify Workbench.

## Public snapshot contract

Path:

```text
%LOCALAPPDATA%\\devbox\\integration\\wsl-desktop\\v1\\summary.json
```

Envelope:

```json
{
  "schemaVersion": 1,
  "producer": "wsl-desktop",
  "producerVersion": "<Cargo package version>",
  "generatedAt": "<UTC second precision>",
  "data": {
    "views": {
      "runtime": {
        "schemaVersion": 1,
        "freshnessMs": 0,
        "entries": [{
          "id": "Ubuntu",
          "name": "Ubuntu",
          "state": "running",
          "terminalCount": 1,
          "dockerAvailability": "available",
          "containers": [{
            "id": "0123456789abcdef",
            "name": "api",
            "state": "running",
            "portMappings": [{ "published": 8080, "target": 80, "protocol": "tcp" }]
          }]
        }]
      }
    }
  }
}
```

`id` and `name` are the same validated WSL registration name because WSL does not expose a
separate stable numeric distro ID. Only running distros appear. `dockerAvailability` is exactly
`available`, `missing`, `error` or `notQueried`; the normal collector uses `notQueried` only as a
reserved value for an explicitly unqueried fixture, never to hide a failed live query.

## Collection flow

```text
dashboard/terminal event or 60s tick
  -> debounced single producer worker
  -> wsl.exe --list --running --quiet
  -> validate/sort running distro names
  -> capture bounded terminal counts
  -> for each distro, sequentially:
       wsl.exe -d <validated-distro> -- docker ps -a --no-trunc --format <four fields>
       -> parse ID/name/state/ports
  -> normalize complete entry vector
  -> Envelope::with_views(runtime)
  -> integration::write_atomic(summary.json)
```

The exact Docker template is `{{.ID}}\t{{.Names}}\t{{.State}}\t{{.Ports}}`. It does not request
image, raw status, command, labels, mounts, environment or health output. The process runner uses
null stdin, piped bounded stdout/stderr, no shell and a fixed five-second total timeout. stderr is
drained to avoid pipe backpressure, but never decoded, logged, returned through IPC or serialized.
Timeout/output overflow/I/O failure kills and waits for the child and aborts the complete publish.

## Normalization and bounds

| Value | Rule |
|---|---|
| distro | max 64, name max 128 bytes, no control/injection chars, duplicate rows fail closed |
| container | max 256/distro and 512 total, ID 1–64 ASCII hex normalized lowercase, name max 256 ASCII-safe bytes |
| state | only `created`, `dead`, `exited`, `paused`, `removing`, `restarting`, `running`, `unknown` |
| ports | single 1–65535 published/target values and `tcp`/`udp`/`sctp`; max 32/container and 1,024 total |
| terminal | max 256 per distro; only counts are copied from SessionState |
| output | Docker/WSL stdout max 4MiB, stderr max 64KiB, line max 16KiB |
| envelope | common integration writer max 10MiB |

Port tokens are split without retaining the source string. Published host addresses are discarded;
IPv4, compact IPv6 (`:::`) and bracketed IPv6 bindings with the same tuple are deduplicated. Exposed-
only (`80/tcp`), ranges, invalid ports/protocols and malformed individual mapping tokens are omitted.
Malformed tab rows, duplicate IDs, unsafe names or collection-level bound failures abort rather than
being turned into an apparently empty result. Unknown Docker state is represented as `unknown`, not
as raw status text.

## Last-good and concurrency policy

The complete collection is held in memory until every running distro has been processed and all
model validation has passed. The producer calls `write_atomic` only then. A malformed list, timeout,
partial output, unsafe value, final envelope validation error or filesystem failure therefore cannot
replace a previous snapshot with an empty/partial JSON file. The shared writer owns the temp-file
and rename protocol and leaves only `summary.json` after success.

App setup starts one 60-second periodic producer. Dashboard distro/Docker refresh, terminal start,
explicit close and PTY reader cleanup set one coordinator pending bit. A 250ms debounce coalesces
bursts; a single worker owns the collection loop, and an event received during collection schedules
one follow-up complete snapshot. WSL/Docker subprocesses are never launched concurrently by the
producer. A small final synchronous write mutex also protects atomic replacement against a future
manual refresh path.

## Privacy review

The snapshot and fixed public error messages contain no WSL cwd, Windows/Unix path, Docker
volume/socket path, session/pane identity, terminal title/profile command, image, raw status/ports,
command, labels, environment, token, password, Authorization, Cookie or raw stdout/stderr. Distro
and container names are constrained to safe bounded identifiers; suspicious credential/path-like
names fail closed. Existing interactive Docker detail remains runtime-only and is outside this
producer's wire contract.

## Fixtures and verification

Rust fixtures cover:

- exact WSL/Docker argv and rejection of shell/mutation/unsafe distro values;
- UTF-8/CRLF/default-marker running distro parsing and deterministic order;
- empty Docker, available/missing/error distinction, unknown state and malformed rows;
- lowercase bounded hex IDs, safe names, duplicate IDs and privacy/path rejection;
- IPv4/IPv6 dedupe, exposed-only/range/invalid port exclusion and deterministic mappings;
- distro/container/terminal/output bounds and final model revalidation;
- complete envelope shape, canonical deterministic serialization, no raw image/status/address/command
  field, atomic replacement, concurrent complete writes, no temp residue and last-good byte
  preservation after malformed collection;
- shared-writer rejection of a credential-shaped distro identity without creating a snapshot;
- terminal count grouping/privacy and per-distro bound, plus bounded-reader overflow behavior;
- subprocess timeout kill/wait behavior with a real direct-child fixture, so the timeout path cannot
  leave the launched child alive;
- distinct `missing`, `error` and successful `available`-with-empty-containers outcomes.

The producer's pure parser/model tests run without WSL or Docker. Windows W1 remains necessary for
packaged evidence: valid Docker, missing Docker (exit 127), empty output, stopped distro exclusion,
timeout/last-good behavior, actual integration path and inspection that no sensitive/raw fields are
published. `#281` consumer, Workbench files and automatic resource mutation remain separate.

Local verification completed after the PR-wide review in this worktree:

- `cargo test -p wsl-desktop --lib runtime_snapshot --no-fail-fast` — 22 passed;
- `cargo test -p wsl-desktop --lib commands::terminal --no-fail-fast` — 24 passed;
- `cargo test -p wsl-desktop --lib --no-fail-fast` — 69 passed;
- `cargo test -p catalog --test catalog --no-fail-fast` — 11 passed;
- `cargo test --workspace`, `cargo check --workspace -j 4` and
  `cargo clippy --workspace --all-targets -j 4 -- -D warnings` — passed;
- `cargo fmt --all --check` and `git diff --check` — passed;
- all 17 frontend workspace test projects and `pnpm build` — passed in a Linux-native temporary
  mirror to avoid `/mnt/e` dependency-tree overhead;
- dependency policy/regression checks, generated third-party notice consistency, catalog checks,
  `cargo deny check` and `pnpm audit` — passed (`cargo deny` reported only the repository's known
  duplicate-version warnings).

The PR-wide review also removed eight Clippy findings in the new process/fixture code and regenerated
the lockfile checksum line in `THIRD_PARTY_NOTICES.md`. GitHub Actions and packaged Windows W1 remain
the two verification layers not available from this WSL worktree.

## Files changed

- `apps/wsl-desktop/src-tauri/src/core/runtime_snapshot.rs` — pure model, parser, normalization,
  bounds and fixtures.
- `apps/wsl-desktop/src-tauri/src/runtime_snapshot.rs` — fixed process runner, coordinator,
  envelope builder, atomic producer and fixtures.
- `apps/wsl-desktop/src-tauri/src/commands/terminal.rs` — bounded terminal count view and lifecycle
  trigger hooks; no session payload is exported.
- `apps/wsl-desktop/src-tauri/src/commands/dashboard.rs` — successful dashboard refresh trigger;
  existing detail query remains unchanged.
- `apps/wsl-desktop/src-tauri/src/lib.rs` and `core/mod.rs` — module/state/writer registration.
- `apps/wsl-desktop/src-tauri/Cargo.toml`, `Cargo.lock` — existing tokio I/O support and integration
  crate dependency; no new external runtime or sidecar.
- `apps/catalog.json`, `crates/catalog/tests/catalog.rs`,
  `apps/devbox-manager/src-tauri/src/core/catalog.rs` — catalog revision 6 and producer capability.
- `apps/wsl-desktop/README.md`, `docs/roadmap.md`,
  `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` — wire/safety/roadmap contract.

No Workbench source, consumer, ProfileStore, WSL/Docker mutation or Log Lens code is included.
