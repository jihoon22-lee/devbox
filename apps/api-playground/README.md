# api-playground — API Playground v0.4.0

로컬 REST/WebSocket API 테스트 앱. 데스크톱 실행에서는 Rust backend가 HTTP와 WebSocket 클라이언트를
담당해 **CORS 제약 없이** 요청한다.
산출물: `ApiPlayground.exe` (`apps/api-playground`).

## 주요 기능

- **요청 작성** — Method, URL, Params/Header/Cookies/Body(JSON·form·multipart·raw). Header table은 같은 이름의
  행과 입력 순서를 유지하고 행별 enabled, 복제·삭제, 현재 환경 secret reference 삽입을 지원한다.
  Cookies tab은 domain/path/만료일을 관리하는 cookie jar가 아니라 현재 요청의 `Cookie` header를
  name/value 행으로 편집한다. Multipart body는 text/file part, part별 Content-Type, enabled,
  복제·삭제와 데스크톱 file picker를 지원한다.
- **OpenAPI 3.x 가져오기** — 로컬 `.json`/`.yaml`/`.yml` 파일 또는 HTTP(S) URL을 bounded parse한 뒤
  server, path/method, path/query/header/cookie parameter, request body example과 지원되는
  basic/bearer/api-key 인증의 **빈 draft metadata**를 operation별로 미리 본다. 체크한 한 operation만
  현재 draft에 명시적으로 적용하거나, 여러 operation을 기존 항목을 덮어쓰지 않는 새 `OpenAPI`
  Collection 항목으로 추가할 수 있다. 로컬 파일 선택·parse는 완전 오프라인이며 URL 입력을 선택한
  경우에만 native fetch를 수행한다. Swagger UI bundle, code generation, 자동 request 전송과 secret
  값 주입은 제공하지 않는다.
- **응답 보기** — 상태코드·시간·크기와 Body/Headers/Cookies 전용 탭. JSON pretty와 본문 복사,
  표 형태의 마스킹 header, 값이 가려진 `Set-Cookie` 이름·안전 attribute를 제공한다. 데스크톱에서는
  별도 경고 확인 뒤 현재 응답의 원문 headers 또는 `Set-Cookie`만 일회성으로 복사할 수 있다.
- **Auth 프리셋** — Basic / Bearer / API Key
- **History / Collection** — 최근 요청과 저장 요청을 v2 형식으로 보존·재호출. 항목 우클릭 또는
  `Shift+F10`/Menu 키로 복제·이름 변경·확인 후 삭제·마스킹 cURL 복사를 실행한다.
- **History 검색·필터** — History의 표시 이름·method·안전하게 정화한 URL·상태 코드만 대상으로
  최대 128자 검색어와 method/성공·실패 필터를 적용한다. 표시 label도 bounded 상태로 유지하며,
  header, Cookie, auth, body와 환경 secret은 검색 색인이나 검색 결과 문자열에 포함하지 않는다.
- **Collection / Environment JSON transfer** — Collection과 Environment 사이드바에서 각각
  versioned JSON 문서를 내보내거나 가져온다. 문서 schema는
  `devbox.api-playground.collection-export` 또는 `devbox.api-playground.environment-export`,
  `schema_version: 1`이며 입력·출력 전체는 1 MiB, Collection 256건, Environment 64건,
  Environment 변수 256건의 bounded 계약을 따른다. 가져오기는 기존 ID를 덮어쓰지 않고 새
  항목으로 추가하며, 데스크톱에서는 native file picker와 atomic write를 사용하고 브라우저에서는
  명시적인 JSON download/file selection만 사용한다.
- **Secret transfer policy** — Environment export는 secret 변수의 DPAPI blob이나 평문 값을
  포함하지 않고 `${NAME}` reference와 `secret: true` metadata만 남긴다. 민감한 이름 또는
  token-shaped 값을 `secret: false`로 위조한 문서는 거부하며, 가져온 secret reference는
  `미설정` 상태로 저장되어 사용자가 새 값을 다시 입력해야 한다. Collection request도
  기존 persistence sanitizer/read-back을 다시 통과하고, multipart runtime path와 generated
  body를 저장하지 않는다.
- **cURL 변환** — 기본 masking cURL 복사, 확인 후 원문 cURL 1회 복사
- **환경(environment)·비밀(secret)** — URL·params·headers·cookies·body·auth에서 `${NAME}`과
  `{{NAME}}` 참조를 지원하고, DPAPI로 보호된 secret은 backend가 요청 직전에 메모리에서만
  해제한다 (`crates/secrets`). Header table의 picker에는 현재 환경의 봉인된 secret 이름만
  표시하고 `${NAME}`을 삽입하며 frontend로 DPAPI secret을 unseal하지 않는다.
- **Protocol Lab · MCP** — 데스크톱에서 MCP Streamable HTTP와 native stdio를 검사한다. HTTP는
  modern `2026-07-28` `server/discover`와 legacy `2025-11-25` initialize/session 흐름을
  선택하거나 안전한 auto fallback으로 협상하며, stdio는 검토한 executable을 shell 없이 실행한다.
  capability가 확인된 tool/resource/prompt만 한 page씩 명시적으로 조회·호출하고, 지원하는 JSON
  Schema 부분집합만 form으로 실행한다. HTTP OAuth 2.1은 system browser·PKCE로 authorize하며
  Windows DPAPI로 봉인한 token만 저장한다. Protocol profile/result/cursor/timeline은 저장하지
  않으며 OAuth grant metadata와 token 저장은 별도 native 보안 경계다. 브라우저 preview는 MCP,
  stdio, OAuth network 요청을 보내지 않는다.

## Protocol Lab · MCP (`#485`)

Protocol Lab은 기존 Environment header reference를 재사용하되, 해제된 secret과 legacy session
ID는 Rust process 밖으로 보내지 않는다. endpoint는 HTTP(S) absolute URL만 허용하고 userinfo,
fragment, credential-shaped query를 거부한다. redirect는 따르지 않으며 custom header는 100행·
128 KiB, request/response는 1 MiB/4 MiB, timeline은 1,000건·4 MiB로 제한한다. connection과
active request reservation은 RAII guard로 성공·실패·취소·timeout·drop 경로에서 정리한다.

목록은 사용자 동작마다 한 page만 가져오고 종류별 100 page·10,000 item·retained 16 MiB에서
중단한다. 직전 응답의 exact cursor만 허용하고 native connection state에서 atomically revalidate해
concurrent pagination race를 막으며, 중복 identity·재사용·cycle cursor는 고정 오류다.
`$ref`, composition, conditional 또는 알 수 없는 JSON Schema keyword는 read-only JSON으로만
보여 주며 호출을 비활성화한다. root `$schema` metadata는 지원하지만 nested `$schema`는
view-only로 남긴다. known-secret redaction은 callable tool schema의 legitimate `password`/`token`
property name을 보존하고 reflected credential string/key는 거부한다. Tool arguments와 prompt
arguments는 timeline에서 전체 masking하고 pagination cursor는 값 대신 `[PRESENT]`만 표시한다.
modern cancel은 owned response stream을 중단하며 legacy는 같은 request ID의 cancelled notification을
최대 2초 동안 best-effort로 전송한다. modern cache metadata와 call/read/get 필수 content shape도
native IPC 전에 검증한다.

Modern JSON-RPC protocol error는 official schema에 따라 id가 없을 수 있지만, recognized error code만
idless로 허용하며 UI에는 stable error code로 매핑한다. modern response의 `MCP-Session-Id`는
거부하고, legacy initialize 이후에는 처음 할당된 session이 바뀌거나 새로 나타나는 response를
거부한다. legacy handshake가 실패하면 할당된 session을 best-effort `DELETE`하고, session-bound
404는 local connection을 무효화하며 side-effecting request를 자동 replay하지 않는다. backward-
compatible legacy SSE resumption/GET listener는 PR1에 포함하지 않는다.

PR1의 source 검증은 focused MCP Rust tests 33 passed, API Playground Rust tests 133 passed,
frontend 33 files/231 tests passed, `cargo check`, strict Clippy와 production build passed이다.
이는 app/source evidence이며 full workspace CI나 Windows packaged acceptance 결과를 주장하지
않는다.

## MCP stdio + HTTP OAuth (`#485`, PR2)

Protocol Lab의 PR2는 MCP Streamable HTTP에 OAuth 2.1을 붙이고, HTTP와 분리된 native stdio
transport를 추가한다. OAuth는 HTTP profile에만 적용되며 stdio에는 적용하지 않는다. stdio의
credential은 사용자가 기존 Environment에서 명시적으로 연결한 값만 spawn 경계에서 해석한다.
브라우저 preview는 두 transport 모두 `native_required`이며 native network/process 동작을
시작하지 않는다.

### Native MCP stdio

- **선택·profile** — executable과 선택적 cwd는 native picker로만 고르고, renderer에는 128-bit
  lowercase hex selection ID와 control-free basename label(최대 256 bytes), 만료 시각만 보낸다.
  selection은 최대 32개를 10분 동안만 보관하며, executable/directory 종류를 혼용하지 않는다.
  spawn 직전에 regular file/directory, canonical path와 filesystem identity를 재검증하고
  symlink/reparse alias나 만료·변경된 selection은 `mcp_stdio_selection_invalid`로 거부한다.
- **실행 경계** — shell이나 command string을 사용하지 않고 executable과 argv를 native process
  argument로 전달한다. child environment는 `env_clear()` 후 Windows의 `PATH`, `PATHEXT`,
  `SYSTEMROOT`, `WINDIR`, `COMSPEC`, `TEMP`, `TMP` 또는 Unix의 `PATH`, `HOME`, `TMPDIR`,
  `LANG`, `LC_ALL`, `LC_CTYPE`만 복원한다. WSL stdio는 지원하지 않는다. argv는 64개·값당
  8 KiB·전체 64 KiB, environment binding은 64개·이름당 256 bytes·resolved 전체 256 KiB,
  timeout은 100 ms–120 s로 제한한다. reserved runtime 이름, 중복 child/source 이름, 누락된
  source, control/NUL과 argv 내 secret 사용은 `mcp_stdio_profile_invalid` 또는
  `mcp_stdio_environment_invalid`로 fail-closed한다.
- **소유권·framing** — Windows에서는 suspended/no-window child를 kill-on-close Job Object에
  넣고, Unix test/runtime 경계에서는 private process group을 사용한다. connection 하나가
  process tree 하나를 소유하며 disconnect, cancel, timeout, EOF, framing/protocol 오류에서
  stdin을 닫고 tree를 terminate/reap한다. stdout은 LF/CRLF 한 줄당 UTF-8 JSON-RPC 하나만
  허용하고 embedded newline, 빈 줄, 비 UTF-8/비 JSON, 중복·예상 밖 response ID를 거부한다.
  outbound request는 shared 1 MiB bound를 따르고, line/parsed JSON은 4 MiB, 한 exchange는
  4 MiB·1,000 messages로 제한한다. stderr는 protocol로
  해석하지 않고 control 제거·known secret redaction 후 64 KiB·256-line zeroizing ring에만
  drain하며 raw text는 IPC로 보내지 않는다.
- **협상·운영** — `modern`은 `server/discover`, `legacy`는 `initialize`와
  `notifications/initialized`, `auto`는 호환성에 해당하는 unrecognized-method/modern discovery
  timeout에서만 process를 종료·reap한 뒤 새 legacy process로 fallback한다. spawn/network/
  framing/malformed/credential 오류는 fallback하지 않는다. 최대 8개 connection, connection당
  active request 하나를 허용하며 기존 explorer의 page·identity·cursor·schema 검증과 bounded
  timeline을 재사용한다. cancel은 legacy `notifications/cancelled`를 best-effort로 보낸 뒤
  전체 connection을 무효화하므로 늦은 응답을 다음 request에 재사용하지 않는다.

### HTTP OAuth 2.1

- **Discovery와 binding** — endpoint는 exact resource로 사용하며 HTTPS만 허용한다(loopback
  fixture는 HTTP 허용). initial resource request, bounded `WWW-Authenticate` challenge와 RFC
  9728 protected-resource metadata를 path-specific well-known → origin-root 순서로 확인한 뒤,
  RFC 8414/OIDC authorization-server metadata를 조회한다. metadata의 resource와 issuer는
  exact normalized match여야 하고, 여러 issuer가 있으면 사용자가 고른 값만 허용한다. 모든
  OAuth 요청은 redirect를 따르지 않으며 userinfo, fragment, control, credential-shaped query와
  cross-origin substitution을 거부한다.
- **Client와 callback** — public client ID(최대 8 KiB)만 받으며 client secret, DCR/CIMD와 device/
  client-credentials/password grant는 제공하지 않는다. authorization server는 code flow,
  public-client `none`, PKCE `S256`을 광고해야 한다. system browser를 열기 전에 ephemeral
  `127.0.0.1` listener를 bind하고 random state/verifier와 S256 challenge를 만든다. callback은
  loopback peer의 단일 HTTP/1.1 GET `/oauth/callback`만 받고, exact state·advertised `iss`·중복
  parameter를 검증하며 authorization code는 token 교환 직후 zeroize한다. 성공·실패 페이지는
  고정 문구만 반환한다.
- **Token lifecycle** — token exchange는 authorization code, redirect URI, client ID, PKCE
  verifier와 같은 `resource`를 exact form POST로 보내고 Bearer token type만 수락한다. token은
  query에 넣지 않으며 access/refresh token은 backend `Zeroizing` memory에만 평문으로 존재한다.
  connect 또는 request 경계에서만 exact issuer/resource/client binding을 확인해 Bearer header를
  주입하고, 만료 시 한 번만 refresh한다. refresh rotation은 old token을 버리기 전에 atomic
  store update를 완료하며 background refresh나 다른 grant fallback은 없다. profile에 OAuth
  grant가 있으면 enabled custom `Authorization` header와 함께 사용할 수 없다.
- **Windows-only persistence** — 최대 32 grant, versioned JSON file(최대 1 MiB)을 app-local data
  directory의 `oauth/mcp-grants.json`에 보관한다. access/refresh token은 각각 기존
  `crates/secrets`의 versioned DPAPI envelope로 봉인하고, file/parent의 symlink/reparse
  redirection을 재검증한 뒤 `atomic_write`한다. renderer projection에는 grant ID, issuer,
  resource, public client ID, scopes, expiry와 `active`/`expired` status만 있고 token, callback
  code, state, verifier, DPAPI blob, discovery body와 storage path는 없다. non-Windows/WSL은
  stable `mcp_oauth_storage_failed`를 반환하며 pure logic test만 수행한다.
- **Revoke와 UI** — discovered revocation endpoint가 있으면 선택된 token을 POST하고, remote
  success와 local removal을 구분한다. remote failure 시에는 사용자가 명시적으로 local removal을
  선택한 경우에만 local grant를 지운다. UI는 HTTP에서만 Authorize/Refresh grants/Revoke를
  제공하고 stdio에서는 OAuth control을 비활성화한다. authorize flow는 한 번에 하나이며 최대
  5분, metadata/token network operation은 15초 bounded timeout이다.

### Bounds and stable errors

OAuth 입력은 endpoint/issuer/client ID 8 KiB, scope 32개·scope당 256 bytes로 제한하고,
metadata/token response는 각각 128 KiB, callback request는 16 KiB로 bounded parse한다. OAuth
grant storage는 32건·1 MiB이며, token expiry는 monotonic live margin과 persistence용 wall-clock
timestamp를 분리한다. stdio와 OAuth에서 raw OS/network/process/server error text, URL/path,
header, token, authorization code와 callback parameters는 IPC에 반향하지 않고 아래 stable
codes만 renderer에 전달한다.

```text
mcp_stdio_selection_invalid       mcp_stdio_profile_invalid
mcp_stdio_environment_invalid     mcp_stdio_spawn_failed
mcp_stdio_transport_failed        mcp_stdio_protocol_invalid
mcp_stdio_message_too_large       mcp_stdio_request_timeout
mcp_stdio_request_cancelled       mcp_stdio_connection_stale
mcp_stdio_cleanup_failed          mcp_stdio_connection_limit
mcp_stdio_request_limit

mcp_oauth_required                mcp_oauth_request_invalid
mcp_oauth_discovery_failed        mcp_oauth_resource_mismatch
mcp_oauth_issuer_mismatch         mcp_oauth_pkce_required
mcp_oauth_client_unsupported      mcp_oauth_callback_failed
mcp_oauth_token_failed            mcp_oauth_storage_failed
mcp_oauth_reauthorization_required mcp_oauth_cancelled
mcp_oauth_revoke_failed
```

### Validation boundary and acceptance status

The native command layer enforces profile, transport, storage, and process bounds, while `mcpApi.ts`
revalidates opaque IDs, projections, timeline ordering, stable error codes, and returned bounds. The
shared explorer keeps result/timeline state in process memory;
only the Windows DPAPI-backed OAuth grant file persists across restarts. This worktree's source
evidence is **160 Rust tests passed**, strict Clippy with warnings denied passed, and **244 frontend
tests across 33 files passed**; the API Playground production frontend build also passed.

WSL cannot exercise Windows DPAPI sealing, native picker/process-tree Job Object behavior, system
browser callback flow, or a packaged `.exe`. Therefore packaged Windows acceptance is still pending:
no Windows stdio fixture, OAuth browser/discovery/restart/revoke run, child-process cleanup result,
or MCP Inspector comparison is claimed here. Those checks must run on Windows before this feature
is considered packaged-acceptance complete.

## Protocol Lab · gRPC (`#485`, v0.6.0)

gRPC는 Protocol Lab 안의 native 전용 패널이다. 브라우저 미리보기는 `grpc_native_required`를
표시하며 파일 선택·reflection·TLS·RPC 네트워크를 시작하지 않는다. 연결 전에 사용자가 schema
source와 TLS profile을 확인하고, 연결 후 descriptor가 제공한 method만 명시적으로 호출한다.

### Schema source와 method flow

- **Local proto** — native picker로 root `.proto`와 선택적인 import-root를 고른다. backend가
  `protox`로 프로세스 안에서 컴파일하며 `protoc`, shell, 다운로드 compiler, 생성된 사용자 코드는
  사용하지 않는다. import는 선택한 root 아래의 UTF-8 `.proto`만 허용하고 symlink/reparse,
  traversal, root 밖 파일은 거부한다.
- **Server reflection** — endpoint에 연결한 뒤 gRPC reflection v1을 먼저 시도한다. v1 reflection
  경계에서 명시적으로 `UNIMPLEMENTED`를 받은 경우에만 v1alpha로 한 번 전환하며, TLS·권한·timeout·
  malformed descriptor·일반 network 오류는 downgrade나 retry를 일으키지 않는다.
- **Method explorer** — descriptor에서 service/method/input/output type과 RPC kind를 투영한다.
  unary, server-streaming, client-streaming, bidirectional-streaming 네 종류를 지원하며, client- /
  bidirectional-streaming 입력은 ProtoJSON message array로 받는다. method path는 backend descriptor에서
  재구성되고 source import/reflection, method 선택, reconnect는 RPC를 자동 실행하지 않는다.

ProtoJSON은 canonical mapping을 사용하고 duplicate object key와 unknown field를 거부한다.
현 result 화면의 response body는 사용자가 명시적으로 Invoke한 동안에만 bounded memory에 보관되며,
서버 status는 고정된 gRPC status name으로만 표시된다. server status message/details/metadata와
raw transport error는 renderer로 전달하지 않는다.

### TLS / mTLS와 native credential

endpoint는 path가 `/`인 absolute `http://` 또는 `https://` authority여야 하며 userinfo, query,
fragment, credential-shaped component는 허용하지 않는다. HTTPS는 인증서 검증을 항상 수행하고
`native`, `custom`, `native+custom` root mode와 선택적인 server-name override를 제공한다.
trust-all verifier, hostname bypass, key log, silent plaintext fallback은 없다. 의도적인 local/intranet
test를 위해 HTTP는 허용하지만 TLS credential은 HTTP 요청에 연결하지 않는다.

CA bundle 또는 client certificate/private-key pair는 native picker로만 고른다. private key는
암호화되지 않은 PEM 하나만 허용하며, PEM 원문·native path는 IPC나 UI에 보내지 않는다. credential은
Windows 패키지에서만 일반 request environment secret과 분리된 DPAPI entropy domain으로 독립
봉인해 app-local versioned store에 atomic write하고, renderer에는
opaque credential ID와 label, CA/client-identity 존재 여부, 생성 시각만 투영한다. WSL/non-Windows는
이 저장 경계를 수행하지 않는다. gRPC v0.6.0에는 arbitrary metadata/header 또는 별도 bearer auth가
없으며 mTLS가 유일한 credential-bearing request mechanism이다.

### Bounds, ownership, and persistence

| Boundary | Limit |
|---|---:|
| Local source / reflected descriptor files | 256 files, 1 MiB/file, 8 MiB total |
| Projected services / methods / types | 256 / 2,000 / 5,000 |
| Method template | 256 KiB |
| Connections / active requests per connection | 8 / 4 |
| Connect timeout / combined connect+reflection ceiling | 100 ms–30 s / 120 s |
| RPC deadline | 100 ms–300 s |
| One encoded/decoded message | 1 MiB |
| Request / response message total | 4 MiB / 8 MiB |
| Stream input / output messages | 1–100 / up to 100 |
| Local summary history / stored credentials | 50 entries / 16 credentials |

Each request has an opaque request ID, exact connection generation, deadline, and cancellation owner.
Cancel, timeout, disconnect, connection replacement, and drop release the corresponding reservation;
there is no automatic reconnect, retry, replay, or hedging. TLS credential storage is bounded to a 4 MiB
encoded document. History and export use summary-only data: source kind, service/method, RPC kind,
message counts, time, elapsed time, fixed status name, TLS mode, and a boolean credential-used flag.
They exclude endpoint, request/response body, metadata, descriptor bytes, source path, credential ID/label,
certificates, keys, and raw errors. `Export summary` creates a versioned document through the backend's
native save dialog and atomic write; it never exports the live message bodies or connection profile.

Stable native errors are code-only (`grpc_invalid_profile`, `grpc_source_selection_invalid`,
`grpc_reflection_unavailable`, `grpc_tls_failed`, `grpc_credential_invalid`,
`grpc_request_timeout`, `grpc_request_cancelled`, `grpc_response_too_large`, and related `grpc_*`
codes). Packaged Windows acceptance is still required for DPAPI persistence/restart/delete, native and
custom roots, mTLS, native pickers, reflection, all four RPC kinds, timeout/cancel cleanup, and summary
export.

### Source verification

The latest source evidence includes **189 API Playground Rust tests**, **29 focused gRPC Rust tests**,
and **36 frontend files / 264 tests** with `--maxWorkers=2`; the focused gRPC frontend suite adds
**3 files / 20 tests**. A local tonic integration fixture covers reflection v1, explicit v1
`UNIMPLEMENTED` → v1alpha fallback, and unary, server-streaming, client-streaming, and
bidirectional-streaming RPCs. `cargo check`, strict Clippy, scoped TypeScript checking, production
`pnpm build`, dependency
check/build-manifest/catalog, `pnpm audit`, and `cargo deny` passed; cargo-deny emitted only existing
duplicate/yanked warnings, and its advisory/license/source gates passed under the existing time-bounded
policy. Windows DPAPI,
TLS/mTLS, native picker, and packaged acceptance are still pending and are not represented by these
source checks.

## Binary response preview (`#348`)

응답 `Content-Type`과 strict UTF-8/제어문자 판별을 조합해 binary 응답을 별도 projection으로
표시한다. 일반 HTTP 응답은 최대 16 MiB, GraphQL 응답은 최대 4 MiB까지만 bounded stream으로
읽으며, 화면에는 media type·원래 크기와 최대 4 KiB hex/UTF-8 preview만 보낸다. invalid UTF-8은
억지로 text로 변환하지 않고 binary로 분류한다. preview와 response metadata는 기존 request
secret/token redaction을 거치며 raw bytes는 History·Collection·localStorage·log·Tauri event
DTO에 들어가지 않는다.

데스크톱에서만 현재 response ID에 묶인 process-memory bounded buffer를 명시적으로 native save
dialog에서 선택한 위치에 atomic write할 수 있다. 새 요청이 시작되거나 response ID가 stale하면
이전 buffer를 즉시 폐기하고 저장을 거부한다. 브라우저 preview에서는 save를 비활성화하고
bounded preview만 제공한다. binary save는 자동 다운로드·실행·clipboard fallback을 만들지 않으며,
취소·경로·파일 오류는 원문 path나 backend 오류를 반향하지 않는 고정 오류로 표시한다.

## Webhook Lab / Developer Toolbox handoff (`api-request/v1`, #315, #343)

Webhook Lab의 masked history/fixture 또는 Developer Toolbox의 명시적인 현재 output에서
`API Playground로 변환`/`API Playground로 보내기`를 선택하면 공용 AppLink protocol v2가
전달한 opaque handoff ID를 claim한다. payload는 argv에 들어가지 않고 공용 handoff store에
10분 동안만 보관되며, preview에는 `producer`, `consumer`, handoff ID, 만료 시각과 요청
method/URL/header/body가 표시된다. Toolbox output은 `POST /`와 `text/plain` body draft로
도착하고, origin-form URL은 이 단계에서 임의의 host로 채우지 않으므로 사용자가 preview에서
확인한 뒤 적용하고 request editor에서 host를 입력·수정해야 한다. Body가 JSON처럼 보여도
`text/plain` 계약을 유지해 자동으로 JSON 요청으로 바꾸지 않는다.

preview는 적용 전까지 request editor·History·Collection·response를 변경하지 않는다. `적용`은
backend claim을 ack/delete한 뒤 요청 editor에 넣고, `취소`는 claim을 restore한다. 만료·손상·
중복 소비·lease/storage 오류는 원문 경로·payload를 반향하지 않는 fixed error로 처리하며
clipboard·임시 파일로 자동 전환하지 않는다. Webhook credential은 `${WEBHOOK_SECRET}` 같은
환경 변수 참조만 전달되고 원문 secret은 producer, handoff store, preview DTO에 들어가지 않는다.
열린 preview는 30초마다 60초 claim lease를 갱신하지만 envelope의 10분 TTL은 늘리지 않는다.
renderer가 사라지거나 claim 응답이 늦게 도착하면 native restore를 시도하며, 수신자는
`webhook-lab` 또는 `developer-toolbox` producer와 `api-playground` target이 정확히 일치하는
envelope만 허용한다. 적용은 request editor를 갱신할 뿐 HTTP request를 자동으로 보내지 않는다.

## Response selection → Developer Toolbox (`toolbox-text/v1`)

현재 렌더링된 response body에서 사용자가 명시적으로 선택한 text만 bounded deterministic
`toolbox-text/v1` one-time masked handoff로 Developer Toolbox에 보낼 수 있다. 선택은 현재
response에 묶여야 하며 response가 바뀌거나 selection이 stale이면 고정 오류로 거부한다. 전체
response로 암묵적으로 확장하거나 clipboard fallback으로 전환하지 않으며, Developer Toolbox에서
명시적으로 preview한 뒤 apply할 때만 소비된다.

## GraphQL 요청 (P2-05, #294)

Body 종류에서 GraphQL을 선택하면 REST 본문과 분리된 `query`, `variables` JSON object,
`operationName` 편집기가 표시된다. 기존 URL params·headers·auth·environment를 그대로
재사용하며, 전송할 때만 environment reference를 해석한다. method는 GraphQL-over-HTTP의
GET/POST만 허용한다.

### Wire contract

- POST는 `Content-Type: application/json`과 함께 다음 canonical JSON object를 전송한다.
  `operationName`(입력 시), `query`, `variables` 순서와 nested object key를 deterministic하게
  직렬화한다. 사용자가 입력한 `Content-Type`, `Content-Length`, transfer 관련 파생 header는
  무시하고 transport가 결정한다. 그 밖의 enabled header와 auth는 기존 request 경계를
  따른다.
- GET은 endpoint query와 params를 URL encoder로 보존한 뒤 `query`, compact JSON
  `variables`, 입력된 `operationName`을 query parameter로 추가한다. URL은 8 KiB를 넘을 수
  없고 credential-shaped query key(`token`, `authorization`, `cookie`, `api-key`, `password`,
  `private-key`, `username` 등)는 값이 비어 있어도 fail-closed로 거부한다. endpoint는
  `http`/`https`와 host를 요구하며 userinfo, fragment, control 문자를 허용하지 않는다.
- 여러 operation은 명시적인 operationName이 필요하고, 이름은 GraphQL name 문법과 128
  UTF-8 bytes를 따른다. query는 최대 512 KiB·100,000 token·100 operations, variables는
  최대 512 KiB의 JSON object·depth 32·10,000 nodes·key/value string 64 KiB다. 생성된
  POST body는 2 MiB, request header는 100행·합계 128 KiB를 넘을 수 없다.
- response body는 native에서 최대 4 MiB까지 bounded stream으로 읽는다. `data`는 depth 64,
  10,000 nodes, key/value string 64 KiB, `errors`는 100개·message 4 KiB·path 20개와 path
  item 128 bytes로 제한한다. 상한을 넘거나 JSON envelope가 손상되면 raw parser/OS 오류
  대신 고정 상태(`not_json`, `invalid`, `oversized`)만 반환한다.

### Response, persistence, and cancellation

- Body tab의 GraphQL summary는 HTTP status(예: HTTP 400)와 GraphQL envelope error(예:
  HTTP 200 + `errors`)를 별도로 보여 주며, bounded `data`와 `errors[].message/path/location`을
  함께 표시한다. `extensions`와 알 수 없는 error field는 projection에서 버린다. 원문
  response body는 기존 masked Body tab에도 남긴다.
- History/Collection에는 GraphQL fields만 저장하고 생성된 POST body나 GET URL을 저장하지
  않는다. query string literal은 기본적으로 `[REDACTED]` 처리하며 exact whole-value
  `${NAME}`/`{{NAME}}` reference만 다시 해석할 수 있도록 보존한다. variables는 JSON
  형태를 유지하되 credential-shaped key/value와 알려진 token을 masking하고, credential
  key에서는 exact whole-value `${NAME}`/`{{NAME}}` reference만 보존한다. 알 수 없는
  GraphQL field는 제거한다. backend sanitizer가 저장 직전에 같은 shape와 redaction을
  재검증하며 request editor 자체는 사용자가 저장/전송하기 전까지 memory-only다.
- response data/error/body/final URL/redirect metadata에는 request auth, cookie, sensitive
  header/variable 및 credential-shaped GraphQL argument가 반향되지 않는다. raw header와
  원문 cURL은 기존처럼 별도 확인 뒤 일회성으로만 제공되며 GraphQL 기본 cURL은 masked
  fields를 사용한다. GraphQL subscription, persisted query, introspection/schema explorer,
  code generation은 이 기능에 포함하지 않는다.
- Send 중에는 버튼이 Cancel로 바뀐다. native cancellation은 bounded caller request ID와
  process-local monotonic token을 함께 사용해 늦은 이전 Cancel IPC를 정확한 요청에만
  적용하고, 새 요청으로 이전 요청을 supersede한다. HTTP connect/header 대기와 bounded
  response body read도 즉시 취소한다. browser preview는 AbortController를 사용한다.
  sequence/mounted guard가 stale response와 unmount 후 state 변경을 버리며, 별도
  sidecar/외부 process는 만들지 않는다. timeout 범위는 100 ms~120 s로 고정한다.
- Tauri 밖 browser preview도 같은 query/variables/body/response projection과 request bounds를
  사용하지만 CORS와 브라우저 header 제약을 받는다. DPAPI secret 요청은 전송하지 않으며,
  browser GraphQL redirect는 auth 재전달을 막기 위해 manual mode로 멈춘다. packaged native
  loopback test가 실제 HTTP acceptance의 기준이다.

## SSE streaming

SSE는 기존 요청 편집기 아래에서 `Start SSE`/`Stop SSE`로 실행한다. GET/POST, 기존
environment reference·Basic/Bearer/API-key auth·header·Cookie·JSON/form/raw/multipart text body를
재사용하며, method·URL·params·header·Cookie·auth·body를 native 경계에서 다시 검증한다. 브라우저
미리보기는 secret environment를 포함한 요청을 거부하고 CORS와 브라우저의 forbidden `Cookie`
header 제약을 따른다. browser fetch는 redirect를 차단하며, file multipart와 part별 Content-Type은
데스크톱 전용이다.

- **Parser contract** — native와 browser가 같은 incremental parser 계약을 사용한다. UTF-8 chunk
  경계를 보존하고 BOM, CR/LF/CRLF, comment, `event`, multiline `data`, `id`, empty `id`와
  decimal `retry`를 처리한다. EOF의 마지막 unterminated line을 flush하며 malformed UTF-8,
  NUL id/event, malformed/oversized retry와 line은 replacement 없이 고정 오류로 종료한다.
- **Bounds** — decoded stream은 20 MiB, retained history는 10,000 event 또는 20 MiB, line/field는
  64 KiB, event name/id는 각각 256 bytes, event data는 1 MiB, retry는 0–60,000 ms다. request는
  URL 8 KiB, headers/params/Cookies/environment 각 100행, header·Cookie·parameter field 64 KiB,
  environment key 128 bytes, body 4 MiB,
  multipart 50 part와 기존 file/text bounds를 사용한다. UI에는 최근 1,000개만 렌더링하고 오래된
  항목의 누적 제거 수를 표시한다.
- **Reconnect and lifecycle** — reconnect는 기본 off이며 사용자가 켠 경우에도 최대 5회,
  server `retry`는 250 ms–60 s로 clamp한다. `Last-Event-ID`를 보내지 않고, redirect는 native에서
  최대 10회만 따라가며 cross-origin 이동에서는 sensitive header/auth/body를 제거하고 credential이
  있는 목적지는 차단한다. connect/idle/total timeout은 각각 100–30,000 ms, 100–300,000 ms,
  1,000–3,600,000 ms 범위다. pause는 rendering만 멈추고 bounded buffer는 계속 유지하며 Stop,
  unmount, 새 generation은 network task와 늦은 event를 취소/폐기한다.
- **Privacy and output** — native stream은 opaque session ID와 masked `event/data/id/retry` DTO만
  Tauri event로 보낸다. URL, local path, raw chunk, request credential, redirect location,
  network/parser stderr는 UI·log·history·telemetry에 반영하지 않는다. event는 자동 저장·export하지
  않고 process memory에만 둔다. `Copy masked events`를 명시적으로 눌렀을 때에만 현재 표시 범위를
  masked SSE text로 clipboard에 한 번 기록한다.
- **Native/browser parity** — pure Rust `src-tauri/src/core/sse.rs`와 browser
  `src/lib/sse.ts` fixture가 chunk split, BOM, lone CR/CRLF/LF, multiline, empty id, retry,
  invalid UTF-8, bounds와 eviction을 고정한다. native command fixture는 loopback HTTP chunked response의
  media type·Accept·Last-Event-ID 차단과 split UTF-8 metadata도 확인한다. native command는 기존 request resolver/redactor와 동일한 secret
  boundary를 재사용하고, browser path는 동일 parser·redaction·generation contract를 따르되
  CORS/forbidden-header 차이를 화면에 고지한다. 새 parser/transport third-party dependency는
  추가하지 않았고, command의 direct `tokio::time` edge는 이미 workspace lock에 있는 Tokio를
  timeout/sleep에만 사용한다.
- **Preflight and cleanup** — GET은 일반 body뿐 아니라 enabled multipart content도 native/browser
  시작 전에 거부한다. browser file part와 text part별 Content-Type도 background fetch 전에
  데스크톱 전용 고정 오류로 거부한다. terminal `closed`/`error`와 component unmount는
  idempotent stop을 통해 native event listener와 browser AbortController를 정리한다.

Windows packaged W2에서는 loopback GET/POST stream, chunk boundary와 reconnect/cancel, native/browser
  fixed error, redaction, bounded eviction, Stop/unmount, keyboard/IME/focus와 offline/no-persistence
  evidence를 확인한다. 외부 SSE service, WebSocket, unbounded/background auto reconnect,
  arbitrary `Last-Event-ID` replay, raw event export는 이 기능에 포함하지 않는다.
## WebSocket 요청 (P2-07, #296)

WebSocket panel은 현재 request의 URL·params·headers·cookies·auth·environment를 재사용한다.
`Connect` 후에만 `Send`, `Ping`, `Close`를 실행할 수 있고 `Disconnect`는 정상 close(1000)로
종료한다. endpoint는 `ws://` 또는 `wss://`만 허용하며 URL userinfo, fragment, credential-shaped
query key와 제어 문자는 fail-closed로 거부한다.

### Wire contract

- 데스크톱 native transport는 `tokio-tungstenite`와 rustls native root로 TLS를 검증한다. 사용자가
  입력한 `Host`, `Connection`, `Upgrade`, `Sec-WebSocket-*`, `Content-Length`,
  `Transfer-Encoding` 같은 transport 파생 header는 무시하고 나머지 enabled header와 Basic /
  Bearer / API Key auth, request Cookie를 handshake에 전달한다. certificate verification을
  끄는 옵션은 없다.
- Text와 binary frame은 UTF-8 text 또는 hex 입력으로 보낼 수 있다. native에서는 ping/pong을
  명시적으로 보내며 peer ping에는 pong으로 응답하고, close code/reason과 연결 상태
  (`connecting`, `open`, `closing`, `closed`, `error`)를 별도 event로 표시한다. Socket.IO,
  STOMP, GraphQL subscription은 지원하지 않는다.
- 각 message는 최대 4 MiB이고 ping/pong payload는 RFC 6455 control-frame 상한 125 bytes,
  close reason은 UTF-8 123 bytes다. 화면은 최대 10,000개 또는 20 MiB까지 오래된 message부터
  제거하며 제거 수를 표시한다. binary는 bounded hex/UTF-8 preview만 렌더링하고 실행하지
  않으며, `Save binary`를 명시적으로 눌렀을 때만 native file picker로 현재 memory payload를
  선택한 파일에 원자적으로 저장한다. 종료된 session의 bounded binary는 다음 Connect 또는 앱
  종료 전까지 저장할 수 있지만 listener와 network task는 terminal state에서 즉시 정리한다.

### Security and browser preview

- backend는 resolved header/auth/environment와 raw binary를 webview event에 보내지 않는다.
  text, close reason, binary hex/UTF-8 preview와 error는 기존 redactor 및 fixed message를
  통과하며 session ID는 opaque numeric ID다. raw binary는 process memory의 bounded buffer에만
  남고 History·Collection·localStorage·로그에 저장하지 않는다. 저장 경로는 webview가 정하지
  않고 native dialog의 사용자 선택만 허용한다.
- Tauri 밖에서는 표준 browser WebSocket preview를 제공하지만 custom header/auth/cookie와
  direct ping/pong은 browser API 제약 때문에 차단한다. secret environment가 있는 요청은
  native에서만 실행할 수 있다. browser와 native 모두 같은 endpoint, payload, close, preview,
  retention bounds와 request timeout을 적용한다. browser socket이 timeout까지 `CONNECTING`이면
  고정 오류만 표시하고 socket을 닫는다.

## 보안·저장 경계

- **History migration** — v1 `apip-history`는 평문 포함 여부를 증명할 수 없어 UI·검색·재전송에서
  즉시 격리하고 raw 원문을 보존하지 않는다. 안전한 History v2를 기록·read-back한 뒤에만 v1
  key와 migration marker를 삭제·기록한다. v2 read-back·raw 삭제 확인·marker 기록 중 하나라도
  실패하면 marker를 남기지 않고 다음 실행에서 fail-closed로 재시도한다.
- **Collection migration** — v1 `apip-collections`는 민감한 auth/header literal을 환경 변수
  reference 또는 `[REDACTED]`로 안전 변환해 v2에 보존한다. 변환된 항목에는
  `requiresSecretReview`를 표시하며, 변환 실패 시 v1 원문은 UI에 노출하지 않고 다음 실행에서
  다시 시도한다. 이 boolean 스키마 메타데이터는 backend sanitizer를 통과해 boolean 타입을
  보존하며, 같은 이름의 비boolean 값은 민감값으로 마스킹한다. raw 원문 backup은 만들지 않는다.
- **Header persistence** — History·Collection v2의 기존 header에 `enabled`가 없으면 true로
  정규화한다. 새 저장본은 중복 행의 순서와 enabled boolean을 명시적으로 보존한다. disabled
  민감 literal도 저장 전 masking하며, disabled secret reference는 저장하되 해제하지 않는다.
- **Header send boundary** — 요청 header는 최대 100행이다. packaged app의 Rust backend는 활성
  중복 행을 순서대로 append하고 disabled 행을 secret 해제·redaction seed·전송·masked/원문
  cURL에서 제외한다. 브라우저 preview도 `Headers.append`를 사용하지만 Fetch 구현이 같은 이름의
  값을 결합할 수 있으므로 exact duplicate wire 검증은 packaged native 경로를 기준으로 한다.
- **Request Cookie boundary** — 구조화 Cookie는 최대 100행이며 name/value 순서와 enabled를
  History·Collection에 보존한다. 값 입력은 기본 password 표시이고, 직접 입력한 활성·disabled 값과
  값 일부에 reference가 섞인 문자열은 저장·기본 cURL에서 `[REDACTED]`로 바꾼다. 값 전체가 단일
  `${NAME}`/`{{NAME}}` reference인 경우에만 참조를 보존하고 backend가 활성 행만 요청 직전에
  해제한다. 활성 행은 `name=value; name=value` 순서의 `Cookie` header 하나로 전송하며, 활성 raw
  `Cookie` header가 Headers tab에도 있으면 모호한 병합 대신 전송·cURL을 fail-closed로 막는다.
  Cookie name/value 문자와 행 수는 frontend와 backend에서 검증한다.
- **Multipart boundary** — multipart는 최대 50개 part, 활성 text 전체 UTF-8 1,000,000바이트,
  파일당 25 MiB와 파일 전체 50 MiB로 제한한다. file picker가 선택한 경로는 현재 실행의
  frontend→Rust 명령에만 존재하고 History·Collection·기본 cURL에는 저장하거나 표시하지 않는다.
  저장에는 안전한 basename만 남겨 다음 전송 전에 파일 재선택을 요구한다. Rust backend가 전송
  직전에 경로를 canonicalize하고 regular file·크기를 검사한 뒤 `reqwest::multipart`로 stream한다.
  multipart의 Content-Type·boundary·Content-Length는 backend가 만들며 사용자가 입력한 파생
  header는 무시한다. text part의 environment reference는 활성 행만 backend에서 해제하고 민감한
  part 이름의 직접값은 저장·기본 cURL에서 마스킹한다.
- **요청·응답 redaction** — response headers/body, final URL, redirect 위치와 오류는 secret,
  Authorization, Cookie 및 민감한 token 패턴을 redaction한다. 모든 cross-origin redirect에서는
  Authorization/Cookie/API-key 헤더와 auth를 다음 요청에 전달하지 않고 요청 body도 억제한다.
  메서드를 보존하는 307/308 redirect에도 동일하게 적용하고, 목적지 URL 자체에 민감정보가
  포함된 cross-origin redirect는 follow 전에 차단해 fail-closed로 처리한다.
- **Response header/Cookie boundary** — 일반 응답 DTO에는 마스킹된 header와 Cookie 이름,
  `[REDACTED]` 값, 제한된 안전 attribute만 포함한다. 원문 header는 Serialize/Debug를 구현하지 않은
  backend 보관소에 가장 최근 요청 1건만 두며 새 요청 시작 즉시 이전 값을 폐기한다. 동시 요청의
  오래된 opaque response ID는 원문을 되살릴 수 없다. header는 최대 100개·원문 합계 64 KiB로
  제한하며 상한 초과나 비텍스트 값이 있으면 원문 전체 복사를 비활성화한다. 원문은 확인 뒤
  clipboard write에만 사용하고 localStorage, History, Collection, 로그에 기록하지 않는다.
- **cURL** — 화면과 기본 복사는 masking된 결과만 사용한다. 확인 대화상자 뒤의 원문 복사는
  데스크톱 backend가 일회성으로 생성하며 저장하지 않는다. Multipart 기본 cURL은 파일 경로 대신
  basename 기반 재선택 placeholder를 사용하고, 확인한 원문 cURL만 현재 runtime 경로를 포함한다.
- **항목 메뉴** — History·Collection 메뉴는 v2에 저장된 마스킹 request만 사용한다. 복제와
  이름 변경도 backend sanitizer 및 read-back 검증을 다시 통과하며, 삭제는 확인 전 저장소를
  변경하지 않는다. History의 선택적 표시 이름은 기존 v2 항목과 하위 호환된다.
- **브라우저 preview** — Tauri 밖에서는 `fetch` 미리보기만 제공하므로 CORS 제한이 있다.
  DPAPI secret이 포함된 요청과 secret 해제·원문 cURL은 차단하며, 응답·URL도 미리보기 경계에서
  redaction한다. 브라우저가 `Cookie`를 forbidden request header로 제한할 수 있으므로 Cookie의
  실제 wire 동작과 cross-origin 제거 계약은 packaged native 경로를 기준으로 한다. text-only
  multipart는 `FormData`로 미리 볼 수 있지만 file part와 part별 Content-Type은 데스크톱 전용이다.
  Fetch는 `Set-Cookie` response header를 노출하지 않으므로 browser preview의 Cookies tab과 원문
  복사는 사용할 수 없고, 이 기능의 acceptance는 packaged native 경로를 기준으로 한다.

### OpenAPI import 안전 경계

- 입력은 UTF-8 기준 4 MiB, 구조 깊이 40, 노드 50,000, 문자열 16,384자, path 250개,
  operation 1,000개, server 20개, parameter 선언 2,000개, security scheme 100개, operation별
  media type 50개로 제한한다. 생성되는 request parameter/header/cookie 행은 각각 100개,
  JSON body와 multipart/form 데이터는 UTF-8 512 KiB로 제한하며 Collection 표시 이름은 120자로 자른다. JSON은
  comments/trailing comma/unsafe number/duplicate key를
  허용하지 않고, YAML은 YAML 1.2 core·unique key·merge 비활성·alias 50개 상한으로 읽는다.
- parser 오류·순환 alias·`__proto__`/`prototype`/`constructor` key·제어문자·비 HTTP(S) server와
  userinfo·민감 query를 고정 메시지로 fail-closed한다. 로컬 파일 이름은 basename만 120자까지
  표시하고 parser 오류나 URL 원문을 화면·로그·preview에 반향하지 않는다.
- URL import는 native `reqwest` 경계에서 URL 2,048자, connect 5초/전체 15초, redirect 3회,
  decoded response 4 MiB를 강제한다. userinfo/credential-shaped query/fragment와 HTTP(S) 외 scheme을 거부하고,
  redirect는 같은 host의 동일 scheme 또는 HTTP→HTTPS 승격만 허용한다. status/network/UTF-8 오류는
  원문 URL이 없는 고정 메시지로 반환한다. URL import가 실패해도 로컬 import에는 네트워크 의존성이 없다.
- `$ref`는 자동 fetch/해석하지 않는다. operation·path item·parameter·request body·security
  scheme에 `$ref`가 있으면 해당 operation만 적용 불가로 표시하고 나머지 operation preview는
  계속한다. 지원하지 않는 method/auth도 같은 operation 단위 오류 경계를 사용한다.
- 기존 request template에는 문서 전체 server 선택 슬롯만 있으므로 path item/operation-level
  `servers` override는 우선순위를 추측하지 않고 해당 operation을 적용 불가로 표시한다.
- 예제는 deterministic 우선순위(example → named examples 정렬 → schema example/default/enum)로
  선택한다. `authorization`, `cookie`, `token`, `secret`, `password`, `credential` 등 민감한
  이름의 값과 `${ENV}`/`{{ENV}}` 형태의 environment reference·Bearer/Basic literal은 항상 빈 문자열로 만들고,
  basic/bearer/api-key에도 값은 절대 주입하지 않는다.
  비민감 path example만 URI component로 URL placeholder에 넣고, secret path는 placeholder를
  유지한다.
  구조화 JSON body의 민감 property는 빈 값으로 redaction하고, opaque raw body example은
  안전상 생략한다. body는 UTF-8 512 KiB 이내만 draft에 넣는다.
- server를 선택해도 import는 URL을 조립할 뿐 요청하지 않는다. 유효한 HTTP(S) server가 없으면
  preview는 보여 주되 apply를 막고, server 선택 변경은 기존 operation 순서·선택 상태를
  보존한다. 현재 draft 적용은 한 항목, Collection 추가는 명시적으로 체크한 항목만 수행하며
  기존 Collection을 overwrite하지 않는다.

## 기술

- Rust(`reqwest` multipart stream, `tokio-tungstenite`)와 Tauri dialog plugin이 직접 요청·파일
  선택 → 브라우저 CORS 없음. WebSocket raw frame은 backend bounded memory buffer에만 둔다.
- 공용 package `@devbox/openapi`는 bounded JSON/YAML graph parse와 null-prototype normalization만
  담당한다(4 MiB/depth 40/nodes 50,000/string 16,384자/alias 50, duplicate·dangerous key·custom
  graph·unsafe number 거부). API Playground의 OpenAPI 3.0/3.1 semantic validation과
  operation-to-request transformation은 `src/lib/openapi.ts`가 소유하며, Webhook Lab의 rule
  projection은 Webhook Lab에 남는다. URL source는 API Playground가 native `reqwest`로만 bounded
  fetch하고, gzip/deflate/brotli/zstd 응답도 해제 후 4 MiB에서 자른다. 이 parser 추출 자체는
  integration/applink 계약을 사용하지 않는다.
- 공용 패키지 `packages/tokens`, `packages/context-menu` 사용

## 개발

- 순수 로직: `src-tauri/src/core/graphql.rs`·`src-tauri/src/core/sse.rs`·
  `src-tauri/src/core/websocket.rs`·`src-tauri/src/commands/request.rs` → `cargo test`
- OpenAPI URL 경계: `src-tauri/src/commands/openapi.rs` → `cargo test -p api-playground`
- OpenAPI 순수 로직: `src/lib/openapi.test.ts` → `pnpm --filter api-playground test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
