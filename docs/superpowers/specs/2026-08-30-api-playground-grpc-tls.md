# API Playground gRPC Lab — TLS/mTLS design

**Date:** 2026-08-30

**Status:** v0.6.0 implementation contract

**Tracking:** #485

## 1. Goal and review boundary

API Playground의 기존 Protocol Lab 안에 gRPC client를 추가한다. 사용자는 native `.proto`
source 또는 server reflection으로 service descriptor를 가져오고, unary/server-streaming/
client-streaming/bidirectional-streaming RPC를 canonical ProtoJSON으로 호출한다.

이 PR은 MCP transport와 독립적인 세 번째 rollback 경계다. 다음을 한 묶음으로 구현한다.

- runtime local `.proto` import와 gRPC reflection v1/v1alpha
- descriptor-backed dynamic ProtoJSON/protobuf codec
- 네 가지 RPC kind, deadline, explicit cancellation, bounded result
- native roots/custom CA/server-name/TLS와 DPAPI-sealed mTLS identity reference
- response body를 자동 저장하지 않는 versioned history/export

MCP connection/OAuth/stdio state, REST History/Collection, arbitrary gRPC metadata, server reflection
enablement, descriptor editing/code generation은 변경하지 않는다.

## 2. Authoritative baseline and dependency decision

Protocol behavior is based on the current gRPC reflection v1 service, gRPC deadline/cancellation
guidance, and the canonical ProtoJSON mapping.

Primary references:

- https://grpc.io/docs/guides/reflection/
- https://github.com/grpc/grpc-proto/blob/master/grpc/reflection/v1/reflection.proto
- https://grpc.io/docs/guides/deadlines/
- https://grpc.io/docs/guides/cancellation/
- https://grpc.io/docs/what-is-grpc/core-concepts/
- https://protobuf.dev/programming-guides/json/

The repository and CI do not provide `protoc`. Runtime user-selected sources also cannot be compiled
by a Cargo build script. The implementation therefore uses one aligned dependency family:

- `tonic 0.14` for HTTP/2 gRPC channel, streaming, status, and rustls TLS
- `prost/prost-types 0.14` and `prost-reflect` for dynamic messages and descriptors
- pure-Rust `protox` at runtime for local source compilation
- `tonic-reflection` generated v1/v1alpha client messages, without enabling its server feature in
  production

No downloaded executable, vendored `protoc`, shell invocation, or generated user code is introduced.
The selected tonic TLS provider is the existing workspace's aws-lc/rustls family so a second crypto
provider is not added only for gRPC.

## 3. User flow

### 3.1 Schema source

The gRPC panel has an explicit source choice.

1. **Local proto**
   - choose one root `.proto` with the native picker;
   - its parent is the default import root;
   - optionally choose a different native import-root directory;
   - the renderer receives only opaque selection IDs, safe basenames, expiry, and kind;
   - source compilation occurs only when `Connect` is pressed.
2. **Server reflection**
   - connect to the configured endpoint/TLS profile;
   - try `grpc.reflection.v1.ServerReflection` first;
   - fall back to v1alpha only when v1 explicitly returns `UNIMPLEMENTED` at the reflection boundary;
   - network, TLS, timeout, malformed descriptor, permission, and other status failures never cause
     a protocol downgrade.

The connected view displays `local-proto`, `reflection-v1`, or `reflection-v1alpha`, the safe root
basename for a local source, descriptor file count, and service/method projections. It never displays
native paths, source text, comments, custom options, or raw descriptor bytes.

### 3.2 TLS and mTLS profile

The connection form accepts:

- absolute `http://` or `https://` authority endpoint;
- connect timeout and per-RPC deadline;
- TLS root mode: `native`, `custom`, or `native+custom`;
- optional server-name override;
- optional opaque TLS credential ID.

`https` always verifies the server certificate. There is no insecure verifier, trust-all checkbox,
hostname bypass, key log, or silent plaintext fallback. A custom CA and client identity are allowed
only with `https`. `http` is shown as plaintext and remains useful for deliberate local/intranet test
servers, but it cannot carry a stored TLS credential.

The credential manager imports bounded PEM files through native pickers:

- optional CA bundle;
- client certificate chain and exactly one unencrypted private key as a pair.

The backend validates PEM shape before storage. Plaintext private key, certificate, and CA contents
never cross IPC. Each blob is sealed independently with the app's Windows DPAPI sealer and stored in
a versioned atomic app-local document. Renderer projections contain only credential ID, user label,
`hasCustomCa`, `hasClientIdentity`, and creation time. Non-Windows builds fail before reading or
persisting a selected credential and clearly report that packaged Windows storage is required.

### 3.3 Method exploration and invocation

Each projected method shows service, method, input/output type, RPC kind, and a bounded canonical JSON
template. The method path is reconstructed from the backend-owned descriptor; the renderer cannot
submit an arbitrary HTTP/2 path.

- unary and server-streaming accept one ProtoJSON message;
- client-streaming and bidirectional-streaming accept a JSON array of ProtoJSON messages;
- `Invoke` is always explicit;
- one-shot streaming sends the bounded input sequence and collects the bounded response sequence;
- no RPC is executed by source import, reflection, method selection, history load, or reconnect;
- arbitrary request metadata/auth headers are absent in v0.6.0. mTLS is the only credential-bearing
  gRPC request mechanism in this panel.

ProtoJSON parsing rejects duplicate object keys and unknown fields. It supports the canonical mapping,
including lowerCamelCase/proto field names, enum names, base64 bytes, 64-bit integer strings, and
well-known types through `prost-reflect`. Server messages are decoded with the exact connected output
descriptor and serialized back to canonical ProtoJSON.

### 3.4 Timeline, history, and export

The live result contains bounded response message bodies because inspecting a response is the user's
explicit action. Bodies are memory-only and are not copied to REST History/Collection, logs,
integration snapshots, local quality snapshots, or telemetry.

The gRPC timeline and history contain summaries only:

- source kind, service/method, RPC kind;
- request/response message counts;
- start time, elapsed time, safe gRPC status name;
- TLS mode and whether an opaque credential reference was used.

They exclude endpoint query data (queries are invalid), request/response bodies, metadata, descriptor
content, source path, credential ID/label, certificates, keys, and raw errors. Local history uses
`devbox.api-playground.grpc-history/v1`, retains at most 50 entries, and is parsed fail-closed.

`Export summary` sends one validated history summary to the backend. The backend constructs
`devbox.api-playground.grpc-exchange/v1` itself and saves it through a native dialog and atomic write.
There is no raw-message, descriptor, certificate, key, or connection-profile export in this release.

## 4. Architecture

```text
ProtocolLab.tsx
  ├─ McpLab (existing)
  └─ GrpcLab.tsx
       ├─ schema/TLS profile and credential projections
       ├─ bounded ProtoJSON input/result state
       └─ summary-only history
            ↓ typed IPC
grpcApi.ts
            ↓
commands/grpc.rs
  ├─ opaque source-selection and connection registries
  ├─ reflection v1 with explicit v1alpha fallback
  ├─ owned request cancellation/deadline
  └─ tonic dynamic calls
commands/grpc_credentials.rs
  ├─ native bounded PEM selections
  └─ domain-separated DPAPI-sealed atomic store
commands/grpc.rs
  └─ summary-only native export
            ↓
core/grpc.rs
  ├─ endpoint/profile/ProtoJSON validation
  ├─ contained protox resolver and descriptor projection
  └─ prost-reflect dynamic codec
```

## 5. Native selection and local source ownership

There are at most 32 pending gRPC file/directory selections. Each expires after ten minutes and owns:

- opaque random 128-bit ID;
- exact kind (`proto`, `import-root`, `ca`, `client-cert`, `client-key`);
- native path only in backend memory;
- safe basename projection;
- filesystem identity captured from a no-follow handle;
- expiry deadline.

Using a selection requires exact kind/identity, `ensure_no_links` for every ancestor, and a fresh
no-follow handle. Identity is rechecked after reading. A selection is consumed by a successful proto
connection or credential import and is never persisted as a path.

The local resolver accepts normalized relative protobuf import names only. Every imported file must:

- end in `.proto`;
- resolve below the authorized import root after canonical containment checks;
- contain no symbolic-link/reparse component;
- be opened no-follow and read as UTF-8 through that exact handle;
- remain within file/count/total budgets.

The import-root identity is checked before and after compilation. `protox` receives the controlled
resolver, not an unrestricted filesystem include path. Google well-known protobuf sources bundled by
`protox` are the only non-selected source namespace.

## 6. Descriptor and reflection contract

| Boundary | Limit |
|---|---:|
| source/descriptor files | 256 |
| one local proto source | 1 MiB |
| local source total | 8 MiB |
| one reflected descriptor proto | 1 MiB |
| reflected descriptor total | 8 MiB |
| services / methods | 256 / 2,000 |
| message/enum types | 5,000 |
| projected name/type | 1 KiB |
| template JSON | 256 KiB per method |

Reflection uses one bidirectional stream so the server's permitted descriptor de-duplication remains
well-defined. It sends one `list_services` request and then one exact `file_containing_symbol` request
per bounded non-reflection service. Responses must match the original request kind/symbol. Duplicate
file names are accepted only when encoded bytes are identical; conflicting duplicates fail closed.
Every returned `FileDescriptorProto` and the resulting pool must satisfy the table before projection.

`grpc.reflection.*` services are transport infrastructure and are not exposed as callable user methods.
Reflection is optional on servers; an unavailable reflection source is actionable and does not imply
the target gRPC service is unavailable when a local proto is supplied.

## 7. Endpoint and TLS contract

The endpoint is at most 8 KiB and must:

- be absolute `http` or `https` with host and explicit/default valid port;
- have path `/` only;
- contain no userinfo, query, fragment, control character, or credential-shaped component;
- normalize to a stable scheme/authority form before connection ownership is created.

Server-name override is optional, ASCII, bounded to 253 bytes, and contains no URI/path/control syntax.
The rustls/tonic TLS builder performs the final DNS/IP server-name validation. Root modes are exact:

- `native`: Windows/native roots only;
- `custom`: selected stored CA only;
- `native+custom`: both.

Custom CA mode requires a credential with CA material. Client identity is optional and independent of
root mode. The connected channel owns whatever rustls key material is needed for that channel; the
unsealed temporary buffers are zeroized immediately after `ClientTlsConfig` construction. Key material
is never formatted with `Debug` or included in an error.

Connect timeout is 100 ms–30 s. Connect and reflection have a combined 120 s hard ceiling. Network,
TLS, PEM, hostname, HTTP/2, and OS failures map to stable codes without endpoint/certificate/parser text.

## 8. Request ownership, bounds, and cancellation

The backend owns at most eight pending/connected gRPC records. Pending connects reserve capacity before
I/O. A record contains the `Channel`, descriptor pool, exact method map, RPC timeout, and up to four
active request reservations. Normalized authority and safe source/TLS projections are returned once at
connect time but are not persisted in the native record.

Each request has a frontend-generated bounded ID, connection generation, watch cancellation sender,
monotonic deadline, and RAII reservation. Cancel requires the exact connection/request owner. Disconnect
first signals all owned requests and then removes the channel/descriptors. Late completion cannot mutate
a removed or replaced connection.

| Boundary | Limit |
|---|---:|
| active connections / requests per connection | 8 / 4 |
| connect timeout / RPC deadline | 100 ms–30 s / 100 ms–300 s |
| request messages | unary 1; streaming 1–100 |
| one encoded/decoded protobuf message | 1 MiB |
| request/response message total | 4 MiB / 8 MiB |
| response messages | 100 |
| input/output JSON depth / nodes | 64 / 20,000 per message |
| input/output JSON string/key | 256 KiB / 4 KiB |

Every tonic client is configured with the one-message encoding/decoding limit in addition to total
application budgets. Deadline is attached to the gRPC request and also enforced by a monotonic local
timer. Explicit cancellation drops the active tonic future/stream, allowing HTTP/2 cancellation to
propagate. Cancellation or timeout does not retry a possibly side-effecting RPC. No automatic reconnect,
retry, hedging, or request replay is implemented.

## 9. Stable error and status contract

Native errors exposed to the renderer are codes only:

```text
grpc_native_required
grpc_invalid_profile
grpc_source_selection_invalid
grpc_source_invalid
grpc_source_too_large
grpc_descriptor_invalid
grpc_reflection_unavailable
grpc_connection_limit
grpc_connect_timeout
grpc_tls_failed
grpc_credential_storage_unavailable
grpc_credential_storage_failed
grpc_credential_invalid
grpc_connection_stale
grpc_method_unavailable
grpc_request_invalid
grpc_request_too_large
grpc_request_limit
grpc_request_timeout
grpc_request_cancelled
grpc_response_too_large
grpc_protocol_failed
grpc_export_failed
```

Server status codes are projected only as fixed names (`CANCELLED`, `UNKNOWN`, `INVALID_ARGUMENT`,
`DEADLINE_EXCEEDED`, `NOT_FOUND`, `ALREADY_EXISTS`, `PERMISSION_DENIED`, `RESOURCE_EXHAUSTED`,
`FAILED_PRECONDITION`, `ABORTED`, `OUT_OF_RANGE`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE`,
`DATA_LOSS`, `UNAUTHENTICATED`). Server status messages/details/metadata are never returned or saved.

## 10. Credential storage contract

The store is Windows-only, app-local, and bounded:

- schema `devbox.api-playground.grpc-tls-credentials`, version `1`;
- maximum 16 credentials and 4 MiB encoded document;
- unique random credential ID and unique user label;
- independent versioned DPAPI envelopes for CA, client certificate, and private key;
- a gRPC-specific DPAPI optional-entropy domain distinct from request-environment secrets;
- atomic write, no-link parent/file validation, identity recheck, strict duplicate/unknown JSON rejection;
- list returns projections only; delete is explicit and serialized with import/list mutation.

Deleting a credential removes only the encrypted persisted record. An already connected channel may
finish its current lifetime with its in-memory TLS configuration; the UI explains this and new
connections cannot resolve the deleted reference. There is no plaintext migration or fallback store.

## 11. Verification

Automated source tests cover the pure boundaries that do not require a packaged Windows process:

- endpoint/root-mode/server-name validation and stable error mapping;
- strict duplicate-free ProtoJSON, canonical scalar/well-known mappings, unknown field rejection;
- all four RPC-kind descriptor projections, strict ProtoJSON mappings, message/total/count limits, and
  owned cancellation/reservation state;
- local resolver containment, traversal/link rejection, import cycles, and representative byte budgets;
- explicit `UNIMPLEMENTED` fallback classification, reflection service/descriptor validation,
  conflicting duplicates, and malformed/oversized payloads;
- live loopback HTTP/2 reflection v1 discovery, an actual v1 `UNIMPLEMENTED` to v1alpha fallback,
  and descriptor-driven unary/server-streaming/client-streaming/bidirectional-streaming dispatch;
- PEM pairing/shape, store schema/duplicates/bounds, mock sealer round trip, path/link replacement;
- connection/request reservation release on success/error/cancel/timeout/drop;
- summary-only history migration/bounds and export allowlist;
- defensive renderer projection validation and native-required browser preview.

The final PR must pass API Playground Rust tests/check/strict Clippy/fmt, frontend tests/build, dependency
policy/notices, and the repository's Linux and Windows CI. The loopback source fixture proves plaintext
HTTP/2 reflection success/fallback and the four dispatch shapes, but not deadline/cancel propagation,
native picker replacement races, DPAPI restart/delete, or TLS/mTLS handshakes. System/native roots,
custom CA, packaged summary save, and privacy inspection remain explicit Windows release gates; source
tests do not claim them.
