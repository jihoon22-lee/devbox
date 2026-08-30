# API Playground Protocol Lab — dynamic gRPC with TLS/mTLS

## Overview

This work adds a native gRPC client panel inside API Playground's existing Protocol
Lab. A developer can inspect a service from a local `.proto` source or server
reflection, review the descriptor-backed methods, and explicitly invoke unary or
streaming RPCs with canonical ProtoJSON. The same panel provides verified TLS roots
and an optional Windows DPAPI-backed mTLS credential.

The renderer receives bounded projections and stable error codes only. Native paths,
source text, descriptor bytes, plaintext certificate/key material, raw server errors,
request metadata, and connection profiles do not cross the IPC boundary or enter
summary persistence. Sensitive credential material remains only as encrypted
envelopes alongside bounded non-secret metadata in the native DPAPI store. Response
bodies are available only in the live result view; summary
history and export do not contain message bodies. This source work does not claim
packaged Windows acceptance.

## Context and scope

The implementation follows [issue #485](https://github.com/jihoon22-lee/devbox/issues/485)
and the [gRPC TLS design](../docs/superpowers/specs/2026-08-30-api-playground-grpc-tls.md)
contract.
It is a separate Protocol Lab transport and credential boundary from MCP HTTP,
MCP stdio/OAuth, and REST persistence.

The user flow is:

1. Choose **Local proto** and select a root `.proto` plus an optional import root,
   or choose **Server reflection**.
2. Enter the absolute HTTP(S) authority, timeout values, and TLS root mode. For
   HTTPS, optionally select a stored TLS credential and server-name override.
3. Press **Connect gRPC**. Local sources compile only at this point; reflection
   requests v1 first and tries v1alpha only after an explicit v1 `UNIMPLEMENTED`.
4. Select a descriptor-backed method, review its input template, and press **Invoke**.
   Streaming inputs use a bounded JSON array; no import, reflection, reconnect, or
   method selection automatically performs an RPC.
5. Inspect the bounded live response or export a summary after an explicit user
   action.

The release intentionally excludes generated client code, arbitrary gRPC metadata,
server reflection enablement, schema editing, automatic retry/reconnect, bearer
authentication, and a trust-all TLS mode.

## Implementation

### Schema compilation and reflection

- `core/grpc.rs` validates the endpoint, root mode, server name, descriptor pool,
  RPC kind, method path, ProtoJSON input, response projection, and fixed gRPC status
  names.
- A controlled `protox` resolver accepts only normalized relative `.proto` names
  below the authorized import root. Every file is opened as UTF-8 through a fresh
  no-link identity check; traversal, symlink/reparse components, replacement, import
  cycles, and source/count/byte budget violations fail closed.
- The runtime dynamic codec uses `prost`/`prost-types` and `prost-reflect`. It
  supports canonical ProtoJSON mappings, including proto field names, enum names,
  bytes, 64-bit integer strings, and well-known types. Duplicate JSON object keys
  and unknown fields are rejected.
- The reflection command owns one bidirectional reflection stream. It validates
  request/response kind and symbol, de-duplicates descriptor files only when their
  encoded bytes are identical, strips source-code info before projection, and never
  exposes raw descriptors. Reflection v1alpha is a compatibility fallback only at
  the defined initial `UNIMPLEMENTED` boundary.

### Dynamic RPC transport

- `commands/grpc.rs` owns the channel, descriptor pool, exact method map, connection
  identity, and request reservations; the renderer separately owns its UI generation.
  Method paths come from the connected descriptor rather than renderer input.
- Unary, server-streaming, client-streaming, and bidirectional-streaming calls use
  the same dynamic tonic codec. Requests and responses have both per-message and
  aggregate budgets; all calls attach a gRPC deadline and a local monotonic timer.
- Cancellation drops the active call and signals the owned request. Disconnect and
  stale-generation handling invalidate the channel before late completion can mutate
  renderer-visible state. No request is retried or replayed because a call may have
  side effects.
- The backend maps network, TLS, HTTP/2, protocol, timeout, cancellation, and source
  failures to stable `grpc_*` codes. Server status messages, details, metadata, and
  raw OS/error text are not returned.

### TLS/mTLS credential boundary

- HTTPS always verifies certificates. The supported root modes are native roots,
  selected custom CA, or native plus selected custom CA. There is no insecure
  verifier, hostname bypass, key log, or silent downgrade to plaintext.
- Native pickers select a bounded CA bundle or a client certificate plus exactly one
  unencrypted private key. PEM shape and pair requirements are checked before any
  persistence. HTTP profiles cannot use a stored TLS credential.
- `commands/grpc_credentials.rs` stores each CA/certificate/key independently in a
  versioned, bounded, atomic Windows DPAPI document. Parent and file no-link/identity
  checks prevent reparse redirection. A distinct DPAPI entropy domain prevents sealed
  TLS blobs from being replayed as ordinary request-environment secrets. The renderer
  sees only an opaque ID, safe label, material-presence flags, and creation time.
  DPAPI output buffers and invalid UTF-8 plaintext copies are explicitly zeroized
  before release; successfully decoded material remains in `Zeroizing` storage.
  Non-Windows/WSL returns the stable
  storage-unavailable boundary without reading or persisting selected credential
  material.

## Security, privacy, and resource limits

The native and renderer layers both validate their respective projections. The
important limits are:

| Boundary | Limit |
|---|---:|
| Local/reflected descriptor files | 256 files; 1 MiB/file; 8 MiB total |
| Projected services/methods/types | 256 / 2,000 / 5,000 |
| Method template | 256 KiB |
| Connections/active requests per connection | 8 / 4 |
| Connect timeout/RPC deadline | 100 ms–30 s / 100 ms–300 s |
| Combined connect + reflection ceiling | 120 s |
| Encoded/decoded message | 1 MiB each |
| Request/response message totals | 4 MiB / 8 MiB |
| Stream input/output messages | 1–100 / up to 100 |
| Local summary history/credential records | 50 / 16 |

Local file and credential selections are expiring opaque IDs. Native paths remain
backend-only, are revalidated before use, and are consumed after successful use.
The renderer never chooses an arbitrary HTTP/2 method path or sends arbitrary
metadata. Response message bodies are bounded process-memory state for the current
explicit invocation. The summary history schema
`devbox.api-playground.grpc-history/v1` and native export
`devbox.api-playground.grpc-exchange/v1` retain only source/method/RPC summaries,
counts, timing, fixed status names, TLS mode, and a boolean credential-used flag.

They intentionally omit endpoint/query, request/response bodies, metadata,
descriptor contents, source paths, credential IDs/labels, certificates, private
keys, and raw error text. History has a bounded local-storage read-back contract;
export uses a native save dialog and atomic write. Browser preview remains native
only and cannot initiate network, file-picker, TLS, or RPC work.

## Dependency and advisory decision

The dynamic client uses one aligned dependency family: tonic 0.14.6, prost and
prost-types 0.14.4 as resolved, prost-reflect 0.16.5, protox 0.9.1, tonic-reflection
0.14.6, and tokio-stream 0.1.19 as resolved. No `protoc`, downloaded executable,
generated user code, or new frontend package is introduced.

During dependency review, a direct `rustls-pemfile` candidate was removed after
`cargo deny` reported **RUSTSEC-2025-0134**. No advisory exception was added. PEM
reading now uses the already locked `rustls-pki-types` 1.15.1 API. The dependency
decision, source/license/checksum record, package-size checkpoint, and generated
notices are tracked in [`docs/dependency-policy.md`](../docs/dependency-policy.md)
and [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md).

## Changes made

- Added the dynamic gRPC core and native command modules under
  `apps/api-playground/src-tauri/src/{core,commands}`.
- Added typed renderer IPC wrappers, bounded summary storage helpers, and the
  `GrpcLab` panel; `ProtocolLab` exposes gRPC alongside existing MCP tabs.
- Added focused Rust and frontend fixtures for projection validation, strict
  ProtoJSON, source containment, reflection fallback, limits, reservations,
  credential shapes/storage, and summary-only behavior.
- Updated the API Playground README with the user flow, native/TLS/mTLS boundary,
  privacy contract, and limits.

## Verification evidence

The latest source evidence recorded for this worktree is:

```text
cargo test -p api-playground -j2
189 passed; 0 failed

cargo test -p api-playground grpc -j2
29 passed; 0 failed

pnpm --filter api-playground test -- --maxWorkers=2
36 files; 264 tests passed

Focused gRPC frontend suite (--maxWorkers=2)
3 files; 20 tests passed

cargo check -p api-playground
passed

cargo clippy -p api-playground --all-targets -- -D warnings
passed

cargo check --workspace -j2
passed

cargo clippy --workspace --all-targets -j2 -- -D warnings
passed

cargo test --workspace -j2
passed (all workspace app, shared crate, and doc tests)

cargo fmt --all -- --check
passed

pnpm --filter api-playground build
passed (TypeScript check and production Vite build)

bash .github/scripts/run-frontend-scope.sh typecheck apps api-playground
passed

dependency check, build-manifest, catalog, and pnpm audit
passed

cargo deny --locked check
passed; existing duplicate/yanked warnings only; advisory/license/source gates passed under the
existing time-bounded policy
```

The local tonic integration fixture verifies reflection v1 success, explicit v1
`UNIMPLEMENTED` fallback to v1alpha, and unary, server-streaming, client-streaming,
and bidirectional-streaming RPC dispatch. These are source/integration checks, not
packaged Windows acceptance.

## Remaining Windows acceptance

Packaged Windows testing remains a release gate. It must exercise native proto/import
pickers, Windows DPAPI credential create/list/delete/restart, native/custom/combined
roots, server-name verification, a local TLS/mTLS fixture, reflection v1 and explicit
v1alpha fallback, all four RPC kinds, ProtoJSON rejection, message/stream limits,
deadline/cancel/disconnect cleanup, stale connection behavior, and summary export.
The acceptance run must confirm that no path, certificate, key, body, metadata, or
raw error text enters renderer state, history, export, logs, or the packaged bundle.
