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
- **cURL 변환** — 기본 masking cURL 복사, 확인 후 원문 cURL 1회 복사
- **환경(environment)·비밀(secret)** — URL·params·headers·cookies·body·auth에서 `${NAME}`과
  `{{NAME}}` 참조를 지원하고, DPAPI로 보호된 secret은 backend가 요청 직전에 메모리에서만
  해제한다 (`crates/secrets`). Header table의 picker에는 현재 환경의 봉인된 secret 이름만
  표시하고 `${NAME}`을 삽입하며 frontend로 DPAPI secret을 unseal하지 않는다.

## Webhook Lab handoff (`api-request/v1`, #315)

Webhook Lab의 masked history 또는 fixture에서 `API Playground로 변환`을 선택하면 공용
AppLink protocol v2가 전달한 opaque handoff ID를 claim한다. payload는 argv에 들어가지 않고
공용 handoff store에 10분 동안만 보관되며, preview에는 `producer`, `consumer`, handoff ID,
만료 시각과 요청 method/URL/header/body가 표시된다. origin-form URL은 이 단계에서 임의의
host로 채우지 않으므로 사용자가 preview에서 확인한 뒤 적용하고 request editor에서 host를
입력·수정해야 한다.

preview는 적용 전까지 request editor·History·Collection·response를 변경하지 않는다. `적용`은
backend claim을 ack/delete한 뒤 요청 editor에 넣고, `취소`는 claim을 restore한다. 만료·손상·
중복 소비·lease/storage 오류는 원문 경로·payload를 반향하지 않는 fixed error로 처리하며
clipboard·임시 파일로 자동 전환하지 않는다. Webhook credential은 `${WEBHOOK_SECRET}` 같은
환경 변수 참조만 전달되고 원문 secret은 producer, handoff store, preview DTO에 들어가지 않는다.

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
- OpenAPI parser는 `yaml@2.9.0` 및 `jsonc-parser@3.3.1`을 사용하며 앱 내부 순수 변환 계층에
  격리한다. URL source만 기존 native `reqwest`로 bounded fetch하며, gzip/deflate/brotli/zstd 응답도
  해제 후 4 MiB에서 자른다. 공용 integration/applink 변경은 없다.
- 공용 패키지 `packages/tokens`, `packages/context-menu` 사용

## 개발

- 순수 로직: `src-tauri/src/core/graphql.rs`·`src-tauri/src/core/sse.rs`·
  `src-tauri/src/core/websocket.rs`·`src-tauri/src/commands/request.rs` → `cargo test`
- OpenAPI URL 경계: `src-tauri/src/commands/openapi.rs` → `cargo test -p api-playground`
- OpenAPI 순수 로직: `src/lib/openapi.test.ts` → `pnpm --filter api-playground test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
