# webhook-lab — Webhook Lab v0.3.0 (로컬 웹훅/콜백 서버)

API Playground가 outbound HTTP 클라이언트라면, Webhook Lab은 **inbound HTTP 요청을 받고 검사·재현**하는 로컬 서버.
산출물: `WebhookLab.exe` (`apps/webhook-lab`).

## 주요 기능

- **서버 시작/정지** — localhost bind 주소·포트 선택 (기본 `127.0.0.1`)
- **request history** — method/path별 headers/query/body/timestamp 기록과 masked replay. renderer에
  노출되는 snapshot을 캡처 시점부터 공용 sanitizer로 처리하므로 Authorization·Cookie·token/
  secret/password/auth 계열 header, query 값, JSON/text body credential과 known token은 history
  목록·마스킹 복사·fixture/replay 입력에 평문으로 남지 않는다. 원본 header vault는 bounded
  process memory에서 사용자가 별도 확인한 일회성 raw copy에만 접근한다.
- **응답 rule** — 고정 status/header/body, delay와 대표 오류 응답(500/404 등). `priority`와
  specificity tie-break로 겹치는 규칙의 선택도 결정적이며 저장 전에 conflict preview를 표시한다.
- **response sequence** — 첫 응답 뒤 bounded 단계별 status/body/delay를 순서대로 반환하고 현재 위치를 초기화
- **rule 설명** — method 대소문자 무시·빈 값 전체 적용, path 정확 일치·후행 `*` wildcard,
  status 응답 코드, delay 밀리초 의미를 편집 중 항상 표시
- **대상별 컨텍스트 메뉴** — history의 마스킹 복사·확인 후 원본 복사·마스킹 헤더
  복사·masked replay·개별 삭제, rule의 편집·복제·PowerShell/POSIX curl 복사·response sequence 초기화·삭제. 우클릭과 `Shift+F10`/Menu 키를 지원하고 닫은 뒤
  원래 행으로 포커스를 돌려보낸다.
- **예시 curl** — 실행 중인 서버의 fresh bind 주소와 rule의 method/path를 반영한 실행 가능한
  요청을 `PowerShell curl.exe` 또는 `POSIX sh curl` 형식으로 복사한다. rule의
  status·응답 headers·응답 body는 `--include`로 실제 응답을 확인할 수 있도록 안전한 주석으로
  함께 표시하며, Authorization·Cookie·API key·token·password 계열 값과 알려진 token 형태는
  `[REDACTED]`로 마스킹한다. wildcard path는 backend trailing-`*` matcher와 일치하는
  concrete sample로 바꾸고, shell별 독립 quoting과 `--globoff`·`--path-as-is`로 command/URL
  확장과 curl의 path dot-segment 정규화를 막는다.
- **OpenAPI rule draft** — 로컬 OpenAPI 3.0/3.1 JSON/YAML에서 method/path와 가장 낮은 2xx
  status만 bounded preview로 읽고, 선택한 항목을 저장되지 않은 rule editor draft로 적용한다.
- **Run Manager service export** — 실행 중인 loopback listener의 현재 backend rule을 앱 전용
  profile로 저장하고, 사용자가 명시적으로 내려받을 수 있는 비활성 service definition을 만든다.
- **Log Lens sanitized capture handoff (#489, W08 PR2)** — history 또는 masked fixture에서
  `webhook-log/v1` one-time preview를 만들어 Log Lens로 명시적으로 보낸다. Webhook Lab은
  header 이름만, redacted origin-form target, timestamp, 최대 4 KiB redacted body preview와
  `redacted`/`truncated` flag만 전달하며 header 값·raw body·filesystem path·command·environment·
  archive는 전달하지 않는다.

### Rule 매칭·응답 의미

- `method`는 대소문자를 구분하지 않는다. 편집기의 method를 비워 저장하면 backend DTO의
  `None`으로 전달되어 모든 method에 매치된다.
- `path`는 요청 URL 문자열 전체가 같은 경우에만 기본적으로 매치된다. rule path의 **마지막
  문자**가 `*`일 때만 `*` 앞부분을 접두사로 사용한다. 따라서 `/events/*`는 `/events/`와
  `/events/123`에 매치하지만 `/eventslater`에는 매치하지 않고, `/events/*/tail`의 중간 `*`는
  wildcard가 아니라 literal 문자다.
- `status`, `headers`, `body`는 요청 조건이 아니라 매치된 요청에 돌려줄 HTTP **응답**이다.
  `delay`는 그 응답을 보내기 전에 기다리는 밀리초이며, 매치가 없으면 `404 Not Found`를
  지연 없이 반환한다.
- native response framing은 `1xx`, `204`, `205`, `304`, `HEAD` 응답에 body와 `Content-Length`를
  쓰지 않으며, `Connection`과 `Content-Length` 등 transport header는 rule이 덮어쓸 수 없다.
- 여러 rule이 동시에 매치되면 하나의 Rust comparator가 다음 순서로 winner를 고른다:
  높은 `priority` → exact path → method 지정 rule → 더 긴 후행-`*` prefix → bytewise 오름차순
  rule ID. live listener, 목록, conflict preview가 모두 같은 comparator를 사용하며 `HashMap`
  순회 순서에는 의존하지 않는다.
- 편집기는 priority를 `-1000~1000`, status를 `100~599` 정수, delay를 `0~60000ms` 정수로
  제한한다. Rust wire type의
  표현 범위보다 좁은 UI 경계로 실수로 비정상 status를 보내거나 서버를 장시간 sleep시키는
  것을 막는다.
- 필드 설명은 값이 채워져 있어도 항상 보이고, label/help/error가 각 입력에 연결된다.
  저장·서버 오류는 backend 원문(로컬 경로·토큰 등)을 화면에 그대로 표시하지 않고 고정된
  안전 메시지로 표시한다.

### Rule 저장 경계와 크기 계약

`set_rule` IPC와 Rust `core/rules.rs::upsert`가 최종 권위다. 프론트 검증을 우회한 호출도
아래 경계를 통과해야 하며, 실패한 add/edit는 map을 변경하지 않는다. 검증 오류는 입력값,
경로, header 값, secret, parser/OS 오류를 포함하지 않는 고정 메시지
`규칙 입력이 유효하지 않습니다`만 반환한다.

- rule은 최대 `200`개다. 기존 `id`는 최대 128자/128 UTF-8 바이트이며 제어 문자를 허용하지
  않는다. 새 rule의 빈 id는 저장 직전에 UUID를 받으며 collection 크기 계산에도 UUID의
  36자/36바이트 footprint를 예약한다.
- v0.5.x rule JSON처럼 `priority`가 없는 입력은 `0`으로 읽는다. 저장 preview는 신규 rule의
  실제 UUID를 먼저 배정한 projected collection으로 겹침과 winner/loser를 계산한다. 겹침이
  있으면 사용자의 명시적 확인 뒤에도 backend가 같은 rules lock 안에서 다시 계산하며, 취소나
  확인 없는 저장은 map과 sequence cursor를 변경하지 않는다.
- method는 `null`(전체 method) 또는 ASCII HTTP token이며 최대 16자/16바이트다. 편집기의
  빈 값은 `null`로 변환하고, `Some("")`이나 공백/제어 문자는 저장하지 않는다.
- path는 ASCII 기준 최대 4,096자/16,384 바이트이며 `/`로 시작하고 모든 Unicode control
  문자를 포함할 수 없다. native parser/replay matcher가 ASCII request-target만 지원하므로
  non-ASCII path는 저장하지 않는다. 문자열을 decode, normalize, query 제거하지 않는다.
  매칭은 저장된 path와 요청 URL의 전체 문자열 exact 비교이거나 **마지막** `*` 하나에 대한
  prefix 비교이며, 중간 `*`는 literal로 남는다.
- response headers는 최대 100개다. 각 이름은 HTTP token, 최대 256자/256바이트, 각 값은
  ASCII 기준 최대 16,384자/65,536바이트이고 control 문자를 허용하지 않는다. native
  HTTP/1.x transport의 wire header가 ASCII 전용인 점과 충돌하지 않도록 `Host`, `Content-Length`, `Connection`,
  `Transfer-Encoding` 등 transport framing header는 rule에서 예약되어 거부된다. 이름과 값을
  합한 rule별 전체는 64,000자/256,000바이트 이하여야 한다.
- response body는 최대 256,000자/1,024,000 UTF-8 바이트다. body는 매칭 조건이 아니라
  반환 payload이므로 별도 텍스트 변환 없이 저장한다. status는 100~599 정수, delay는
  0~60,000ms 정수다.
- collection의 모든 rule에 포함된 id/method/path/header 이름·값/body 문자열의 합은
  최대 2,000,000자/8,000,000바이트다. 프론트는 `Array.from(value).length`와 UTF-8
  `TextEncoder`로, Rust는 Unicode scalar count와 `str::len()`으로 같은 char/byte 단위를
  검사하며, Rust UTF-8로 표현할 수 없는 unpaired JavaScript surrogate도 프론트에서 거부한다.

편집기와 복제 동작은 동일한 validator와 collection projection을 사용한다. invalid raw draft는
입력창에 그대로 남고 IPC를 호출하지 않으며, 편집 중 대상 id가 refresh에서 사라진 stale rule도
고정 메시지로 저장을 중단한다. 작업 중에는 `aria-busy`와 disabled 상태로 double action을 막고,
각 method/path/status/delay/body 설명·오류는 `aria-describedby`/`aria-invalid`로 연결한다.
현재 headers 편집 UI는 별도 기능이지만, 로드·복제된 response headers도 같은 프론트 경계를
검사한다. trailing-star matcher는 그대로 유지하고, 선택 순서만 위의 공개 comparator로
결정한다.

### OpenAPI draft와 Run definition export 계약

Webhook Lab의 OpenAPI 가져오기는 로컬 파일을 renderer에서만 읽는다. 공용 `@devbox/openapi`
parser는 입력 4 MiB, depth 40, node 50,000, string 16,384자, alias 50개를 넘거나 duplicate/
dangerous key, cycle, unsafe integer, custom graph를 포함한 문서를 거부한다. Webhook adapter는
OpenAPI 3.0/3.1의 최대 250 paths·1,000 operations를 정렬해 preview하고 method/path와 가장
낮은 명시적 2xx status(없으면 200)만 rule draft로 만든다. server URL, auth/security,
request body, example, credential은 읽어 rule에 넣지 않는다. `{parameter}` path, `$ref`,
unsafe path와 잘못된 operation은 이유와 함께 개별 비활성화하며 `*`로 자동 확대하지 않는다.
preview나 draft 적용은 기존 rule을 저장·전송하지 않으며 최종 저장은 평소 rule 검증과 conflict
확인을 다시 거친다.

`Run Manager definition 내보내기`는 현재 서버가 `127.0.0.1` 또는 `::1`에서 실제 실행 중일
때만 backend가 허용한다. renderer는 bind, port, rule, executable path나 command를 IPC로
보내지 않는다. backend가 현재 상태를 다시 읽어 priority 순으로 정렬한 rule을
`app_local_data_dir()/service-profiles/<uuid>.json`에 보관하고, export JSON에는 opaque profile
ID를 인자로 받는 고정 명령과 loopback health check만 넣는다. 파일은 schema v1, 최대 64개,
파일당 8 MiB이며 app-owned absolute path, no-link read/revalidation, atomic write, strict unknown
field 거부를 적용한다. credential 형태의 id/method/path/header/body/sequence가 있는 rule은 전체
export를 거부한다. listener lifecycle lock은 상태 확인부터 profile count 검사·write까지 export를
직렬화한다.

내려받은 Run Manager schema v1 문서는 job 없이 service 하나만 포함하며 `enabled=false`,
`autoStart=false`, `restartPolicy=never`, environment/cwd 없음으로 시작한다. response body와 rule,
runtime PID/log path는 export JSON에 포함되지 않는다. 사용자가 Run Manager에서 별도로 import하고
활성화해야 하며, `--service-profile <uuid>`는 정확한 두 인자 startup mode에서만 app-local profile을
읽어 숨긴 listener process를 시작한다. 실제 Windows packaged startup/stop 동작은 v0.6.0 통합
acceptance에서 검증한다.

### Captured fixture 저장 계약 (#314)

history에서 **masked fixture 저장**을 선택하면 backend가 opaque history ID로 현재
마스킹된 snapshot을 읽어 앱 전용 JSON 파일에 저장한다. 사용자가 경로나 body를 IPC로
제공할 수 없고, 원본 header vault는 fixture 입력에 도달하지 않는다.

- 저장 위치는 Tauri `app_local_data_dir()/fixtures.json` 하나다
  (`%LOCALAPPDATA%\com.devbox.webhooklab\fixtures.json`). 파일명과 부모 디렉터리는
  앱이 소유하며 fixture가 임의 경로를 읽거나 쓰지 않는다.
- schema v1의 fixture ID는 `fixture-<number>`로만 발급되고, 최대 200개·파일 8 MiB,
  method 16자, origin-form target 4,096자/16 KiB, header 100개·이름 256자·값
  16,384자·총 64,000자/256 KiB, body 256,000자/1 MiB 경계를 적용한다.
- `Authorization`·`Cookie`·token/secret/password/auth 계열 header와 JSON/text의 같은
  credential 표시는 `[REDACTED]`가 된다. JSON string 안의 bounded embedded JSON도 같은
  depth/node/size 경계로 재귀 sanitization하며, escaped sensitive key와 malformed
  JSON-looking string은 각각 decode·redaction 또는 fixed marker로 fail-closed 처리한다.
  절대 URL·`..`/`.`·역슬래시·잘못된 percent
  encoding·token-shaped path는 고정 `/[REDACTED_PATH]`로 바꾸고, 안전한 query만
  보존한다. 입력을 넘으면 부분 fixture를 만들지 않는다.
- 파일은 atomic replace와 raw-byte compare-and-swap으로 저장한다. read/revision/compare/write와
  fixture add/edit/delete/clear mutation은 `.fixtures.json.lock` persistent sidecar에 대한
  OS exclusive advisory lock으로 cross-process 직렬화하며, lock 획득은 500ms bounded retry와
  fixed error를 사용한다. sidecar는 stale lock-file 삭제를 하지 않고 계속 유지한다. corrupt·
  oversized·symlink/non-file store 또는 lock sidecar는 고정 오류로 fail-closed하고 원본 파일을
  자동 복구·덮어쓰지 않는다. 읽기 시 최종 파일을 no-follow open으로 다시 확인해 metadata 검사와
  실제 read 사이의 symlink/reparse TOCTOU도 거부하고, mutation 전후에는 모든 부모 link 검사와
  immediate parent filesystem identity를 다시 확인한다. 이 identity check는 path-based
  재검증이며 handle-relative ancestor 보장은 아니므로, 확인 사이의 ancestor
  symlink/junction/reparse 교체 race는 남는다. 목록은 capture timestamp 내림차순, 동일 timestamp에서는
  ID 순으로 정렬한다.
- fixture의 `응답 rule 초안`은 method/path만 편집기에 채우며 status 200·빈 response
  headers/body·delay 0으로 시작한다. rule 저장은 별도 사용자 동작이다.

### API Playground handoff (#315)

history 또는 저장된 masked fixture의 `API Playground로 변환`은 backend가 보유한 opaque
history/fixture ID만 IPC로 전달한다. Webhook Lab은 이미 masked 상태인 snapshot만 읽어
`api-request/v1` payload를 만들고, 원본 header vault·raw body·clipboard를 이 경로에서
읽거나 쓰지 않는다. origin-form URL은 host를 임의로 만들지 않고 API Playground preview에서
확인한 뒤 적용 후 request editor에서 입력·수정할 수 있게 보존한다.

catalog에서 `api-playground` 설치와 `handoff:api-request/v1` capability를 확인한 뒤 공용
`crates/applink` store에 10분 TTL·10 MiB bounded envelope를 만들며, envelope에는
`producer=webhook-lab`과 `consumer=api-playground`가 고정된다. 실행 인자에는 payload가
아닌 kind와 128-bit opaque handoff ID만 들어간다. 수신 앱이 없거나 실행에 실패하면 fixed
error만 표시하고 clipboard·임시 파일 fallback은 사용하지 않는다.

API Playground는 cold/hot AppLink 모두에서 handoff를 claim한 뒤 적용 전 preview를 표시한다.
`적용`은 claim을 ack/delete하고 요청 편집기에 반영하며, `취소`는 restore한다. 만료·손상·중복
claim·lease 오류는 fixed error로 표시하고 자동 재전달이나 clipboard fallback을 하지 않는다.
preview가 열린 동안 30초마다 claim lease를 갱신하되 원 envelope TTL은 연장하지 않는다.
credential marker는 `${WEBHOOK_SECRET}` 같은 이름 참조로만 남으며 secret 원문은 handoff에
포함되지 않는다.

### Webhook Lab → Log Lens sanitized capture handoff (#489, W08 PR2)

Webhook Lab 0.3.0은 history 또는 이미 저장된 masked fixture에서 현재 capture의 표시용
projection만 만들어 `webhook-log/v1` one-time handoff로 Log Lens 0.2.0에 보낸다. catalog
revision 17에서 `webhook-lab` producer와 `log-lens` consumer/action이 선언되어 있어야 하며,
대상 앱이 설치되지 않았으면 handoff를 만들지 않는다.

payload schema v1은 다음 필드만 갖는다.

```json
{
  "schemaVersion": 1,
  "method": "POST",
  "target": "/hooks?[REDACTED]",
  "receivedAtMs": 0,
  "headerNames": ["Authorization", "Content-Type"],
  "bodyPreview": "[REDACTED]",
  "redacted": true,
  "truncated": false
}
```

`target`는 filesystem path가 아닌 안전한 origin-form request target이며 위험한 path/query와
credential은 redacted marker로 바꾼다. Header value는 어떤 경우에도 전달하지 않고,
request body 전체를 bounded sanitizer로 검사한 뒤 최대 4 KiB의 redacted preview만 만든다.
Payload와 AppLink argv에는 raw body, header value, filesystem path, command, environment,
credential, raw log, archive가 들어갈 자리가 없다. Log Lens의 preview modal도 body preview를
표시하지 않는다.

발행은 공용 `crates/applink` one-time store의 10분 TTL envelope을 사용하고, 실행 인자에는
`webhook-log/v1`와 128-bit opaque handoff ID만 넣는다. Log Lens는 cold/hot request 모두 claim
후 source summary를 먼저 보여 주며, 사용자가 명시적으로 읽기 전용 source 추가를 누른 뒤에만
ack한다. 취소·검증 실패는 restore하고, launch가 실패하면 producer가 방금 만든 immutable
descriptor와 envelope을 다시 대조해 정확히 그 pending entry만 제거한다. Clipboard·임시
파일·raw archive fallback은 없다. 이 capture source는 ephemeral이므로 Log Lens saved view에
저장할 수 없다.

Log Lens의 canonical wire `displayName`은 `Webhook capture`로 유지한다. UI의 handoff,
reconnect, saved-view 안내는 한국어로 표시하지만 wire 이름을 번역하지 않는다.

### Captured request replay 계약 (#362)

history 또는 저장된 masked fixture의 opaque ID로 현재 실행 중인 Webhook Lab listener에 한
건의 요청을 다시 보낸다. frontend는 body·header·path를 IPC 인자로 보내지 않으며 backend가
memory history 또는 앱 전용 fixture store에서 masked snapshot을 읽고 replay 직전에 같은
fixture sanitizer와 validator를 다시 적용한다.

- destination은 현재 serverStatus의 listener 주소에서만 유도한다. 127.0.0.1·localhost·::1과
  wildcard bind(0.0.0.0·[::])의 loopback destination만 허용하고 DNS·외부 IPv4/IPv6·사용자
  지정 URL은 사용하지 않는다.
- Authorization·Cookie·API key·token/secret/password/auth 계열 header와 알려진 token,
  unsafe path/query/body credential은 [REDACTED] 또는 고정 path marker로 남는다. Host,
  Content-Length, transfer-encoding 같은 transport header는 무시하고 client가 안전한 값을
  다시 만든다.
- 한 action은 한 request만 만들고 process-local 1초 창에서 최대 20건으로 제한한다. body는
  기존 history/fixture의 256,000자·1,024,000바이트 상한을 따르며 connect/read timeout과
  response header 상한을 적용한다. connect 2초, write idle 2초, response read idle 2초,
  write+response 전체 5초의 별도 deadline을 둔다. `Host`·`Content-Length`·`Connection`을 native client가
  고정 생성하므로 입력 header는 97개까지 예약하고, 생성된 전체 wire header가 listener의
  100개·64,000자·256,000바이트 admission을 넘으면 replay를 시작하지 않는다. 동시에 들어온 native replay는 process-local mutex로
  직렬화해 response sequence의 순서를 보존한다. replay 전송은 ASCII HTTP wire 경계를 다시
  확인하고, transport header는 입력에서 제거한다.
- 결과에는 source opaque label과 HTTP status만 포함하고 response body·raw request·network
  오류 원문은 renderer나 log에 반환하지 않는다. 서버가 중지되었거나 stale/corrupt
  snapshot이면 fixed error로 중단하며 clipboard/file/external request fallback은 없다.

listener도 history에 보관하기 전에 요청을 admission한다. native bounded HTTP/1.x transport는
connection마다 HTTP/1.0/1.1 고정 Content-Length 한 건만 읽고, chunked/Expect/알 수 없는
transfer encoding은 추측하지 않고 거부한다. request line/header/body 전체에 5초 wall-clock
deadline과 5초 socket idle timeout을 적용하며 동시에 최대 64개 connection만 worker로
허용한다. method/request-target이 초과하면
`414`, header 개수·문자·바이트가 초과하면 `431`, 선언된/실제 body가 1,024,000바이트를
초과하면 `413`, body read timeout/오류는 `408`, 고정 window 초과는 `429`로 응답하며 어떠한
부분 body도 history에 저장하지 않는다. 응답 rule의 delay는 최대 60초지만 stop 시 50ms
단위로 중단되어 lifecycle join을 붙잡지 않는다. stop은 active socket을 shutdown하고,
replay 중인 bounded I/O도 cancellation flag로 중단한다. native transport는 application
admission 전에 request line/header/body를 직접 bounded parser로 읽으므로 parser 내부의
무제한 allocation·declared length drain·socket slowloris를 외부 HTTP parser에 위임하지 않는다.

### Response sequence 계약 (#363)

각 response rule은 기존 status/headers/body/delay 응답을 첫 단계로 사용하고, 선택적으로
최대 16개의 data-only response step을 순서대로 소비한다. 마지막 step에 도달하면 해당
응답을 유지하며 자동으로 처음으로 돌아가지 않는다. 현재 위치는 process memory의
ephemeral cursor일 뿐 fixture·설정 파일·handoff에 저장하지 않는다.

- 각 step은 기존 response status 100~599, delay 0~60,000ms, headers/body의 동일한 개수·
  문자·UTF-8 byte 상한을 적용한다. arbitrary scripting, expression, distributed state는
  지원하지 않는다.
- 요청이 rule에 매치될 때만 cursor가 한 칸 전진한다. 규칙을 편집하거나 삭제하면 해당
  cursor를 버리고, rule row의 sequence 초기화 또는 context-menu의 response sequence 초기화
  action은 첫 응답부터 다시 시작한다.
- sequence reset은 rule 정의·fixture·history를 변경하지 않는 bounded local action이며,
  없는 rule과 concurrent stale 대상은 고정 오류로 중단한다.

### Example curl 계약

- context menu에는 **PowerShell curl.exe 복사**와 **POSIX sh curl 복사**를 별도 항목으로
  표시한다. PowerShell은 single quote 안의 `'`를 `''`로, POSIX sh는 `'`를 닫고 `\'`를
  이어 붙이는 방식으로 처리한다. `cmd.exe` 형식은 이번 범위에 포함하지 않는다.
- 서버가 실행 중이고 메뉴를 연 뒤 다시 읽은 `serverStatus`에 유효한 address가 있을 때만
  복사를 허용한다. `127.0.0.1`·`localhost`는 `127.0.0.1`로, `[::1]`은 `[::1]`로
  canonicalize한다. wildcard bind `0.0.0.0`·`[::]`는 각각 loopback destination으로
  바꾸며, 외부 IPv4·IPv6와 bracket 없는 IPv6는 fail-closed한다.
- rule path가 마지막 문자 `*`이면 `*` 앞부분에 `example`을 붙인 concrete path를 요청
  URL에 사용한다(`/events/*` → `/events/example`). URL glob 확장은 `--globoff`로 끄고,
  absolute URL·`//` host escape·fragment·원본 또는 percent-decoded 공백/control 문자·잘못된
  percent encoding·path token/placeholder는 거부한다. 원본 path를 trim/decode/re-encode하지
  않으며 `--path-as-is`로 curl의 dot-segment 정규화도 막아 backend exact route semantics가
  바뀌지 않는다.
- 민감 query 값을 `[REDACTED]`로 바꾸면 exact route가 달라지므로, 민감 query가 포함된
  rule은 masking 대신 전체 builder를 중단한다. query의 known token이나 normalization이
  필요한 값도 같은 이유로 중단한다.
- header/body의 placeholder는 값 전체가 `${NAME}` 또는 `{{NAME}}`인 경우에만 보존한다.
  `Bearer ${TOKEN}`, `prefix ${TOKEN}`처럼 raw text와 섞인 값은 전체 `[REDACTED]`로
  대체하고, JSON object key와 path에서는 placeholder를 허용하지 않는다. response metadata는 요청
  `--header`/`--data`로 복사하지 않으며, `--include`가 실제 response headers/body를
  출력한다. example curl 복사 action 자체는 request replay를 실행하지 않으며 raw secret reveal도 제공하지 않는다.
- builder bounds는 path 4,096자, headers 100개/이름 256자/값 16,384자/합계 64,000자,
  body 256,000자, JSON depth 32·node 10,000개·string 64,000자, 최종 출력 512,000자다.
  status는 100~599, delay는 0~60,000ms 정수만 허용한다. parsing·URI·clipboard 예외는
  화면에 원문을 반향하지 않고 고정된 안전 오류로 처리한다.
- stale rule/address 재검증, copy busy lock, menu keyboard(`Shift+F10`/Menu key)와 Escape
  focus restore를 유지한다. 서버 중지·규칙 삭제·clipboard 실패는 복사 없이 고정 alert로
  알린다.

## 안전 경계

- 기본 bind `127.0.0.1`, LAN 공개(`0.0.0.0`)는 명시적 경고 + 별도 설정
- `Authorization`·`Cookie`·API key·token/secret/password/auth 계열 header와 body/query
  credential은 일반 history DTO와 기본 복사에서 capture-time masking
- example curl과 masked replay 모두 같은 민감정보 경계를 따른다. example curl은 실행하지 않고,
  replay는 현재 localhost listener로만 한 건씩 전송하며 raw secret reveal은 제공하지 않는다.
- 원본 헤더는 persistence·log·snapshot·일반 DTO에 넣지 않고 현재 프로세스의 bounded history
  entry에만 보관한다. 사용자가 원본 복사 경고를 확인한 뒤 정확한 history ID로 요청한 한 번의
  clipboard write에만 사용한다.
- body 크기 상한(256K자)·history 개수 상한(200건)·요청당 보관 헤더 상한(100개/총 64K자)
- server lifecycle은 start/stop transition lock과 accept-thread join으로 직렬화하며, accept/
  handler 오류는 stale running 상태를 남기지 않는다. response sequence cursor는 rule
  mutation/reset과 같은 lock 순서로 원자화하고 concurrent replay는 직렬화한다.
- history를 비운 뒤에도 프로세스 안에서 ID를 재사용하지 않아 열린 메뉴가 새 요청을 가리키지 않는다.

## 기술

- Rust 표준 `TcpListener` 기반 bounded HTTP/1.x transport (`src-tauri/src/core/http.rs`)

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-webhook-lab-design.md`

W08 PR2 (#489)의 Webhook Lab 0.3.0 → v0.6.0 Log Lens 0.2.0 sanitized handoff와 catalog
revision 17은 현재 구현/문서 계약이다. cold/hot launch, capability discovery, exact pending
cleanup, saved-view exclusion은 v0.6.0 #493 hosted Windows package gate를 통과했다. 이 결과가
임의 사용자 PC의 모든 installed webhook/source 조합을 관찰했다는 뜻은 아니다.
