# Webhook Lab 설계 — Local Mock/Webhook Server

- 상태: MVP 구현 및 v0.5.0 stable 반영 완료 — #314 captured fixture 저장·#315 Webhook→API
  handoff·#362 captured replay·#363 response sequence 완료. v0.5.1에서도 이 설계 기록과
  구현을 유지한다. W08 Webhook→Log Lens와 service export를 포함한 v0.6.0 #493 hosted packaged
  gate도 통과했으며, 현재 main은 v0.7.0 release를 준비한다.
- 작성일: 2026-08-14
- 근거: `docs/product-opportunities.md` §15.3, §17.9

## 1. 제품 정의

API Playground가 outbound HTTP client라면 Webhook Lab은 **inbound HTTP 요청을 받고
검사하고 재현하는 로컬 서버**다. 개발 중 외부 서비스(웹훅·콜백)를 로컬에서 받아
검증한다.

## 2. MVP 범위

- localhost bind 주소·포트 선택 (기본 `127.0.0.1`)
- method/path별 request history (headers/query/body/timestamp)
- 고정 status/header/body response rule
- delay와 대표 오류 응답 (500/404 등)
- JSON fixture 저장 (fixture root, 이 앱이 관리)
- captured request를 **API Playground request로 변환** (curl/JSON export)
- Port Manager primitive로 포트 충돌 확인

## 3. 안전 경계

- 기본 bind는 `127.0.0.1`. LAN 공개(`0.0.0.0`)는 명시적 경고 + 별도 설정.
- `Authorization`·`Cookie`·`API key`·token/secret/password/auth 계열 header와
  credential-shaped query/body는 history snapshot 생성 시점부터 masking.
- body 크기·history 개수·request rate 상한.
- fixture root 밖의 파일을 응답하지 못하게 한다 (safe_join, knowledge-base 패턴).
- request를 받아 외부 command를 실행하는 hook은 **MVP 제외**.

## 4. 런타임

- Rust에서 경량 HTTP 서버를 띄운다. 현재 사용 중인 `reqwest`(client)와 별개로
  서버 역할이 필요하다.
- 서버 구현은 Rust 표준 `TcpListener` 기반 `core/http.rs` bounded HTTP/1.x transport다.
  HTTP/1.0/1.1의 한 connection 한 request와 고정 `Content-Length`만 지원하고,
  chunked/Expect/알 수 없는 transfer encoding은 추측하지 않는다. 앱 admission 이전에
  request line/header/body의 총 deadline·socket timeout·line/header/body/connection 상한을
  적용해 parser 내부 allocation과 slowloris가 lifecycle을 붙잡지 않게 한다.

## 5. 아키텍처

```
apps/webhook-lab/
├─ src-tauri/src/
│  ├─ core/
│  │  ├─ server.rs      # 경량 HTTP 서버 (요청 수신 → history 기록 → rule 응답)
│  │  ├─ rules.rs       # response rule 파싱·매칭 (순수)
│  │  ├─ replay.rs      # masked loopback replay client (bounded, pure builder + native send)
│  │  ├─ history.rs     # 수신 요청 기록 (순수, 상한 적용)
│  │  └─ masking.rs     # 민감 헤더 마스킹 (순수)
│  └─ commands.rs       # start/stop/list_history/rule CRUD
└─ src/
   ├─ App.tsx           # 서버 상태·포트, history 목록, rule 편집
   └─ api.ts
```

## 6. 완료 조건

- 요청을 받아 history에 기록하고 rule에 따라 응답한다.
- 민감 헤더가 history에 마스킹 없이 남지 않는다.
- fixture root 밖 경로를 응답하지 않는다.
- LAN 공개가 명시적 설정으로만 가능하다.

## 7. #282 rule 설명·저장 검증 계약 (2026-08-26)

현재 rule editor 보강은 response rule의 의미를 설명하고 입력 경계를 고정하는 독립 PR이다.
method가 비어 있으면 `None`으로 저장되어 모든 method에 매치하고, 값이 있으면 대소문자를
무시한 HTTP token 비교를 한다. path는 ASCII request-target만 지원하며 저장 문자열과 요청
URL의 전체 exact match가 기본이다. rule path의 마지막 `*`일 때만 앞부분 prefix match를
한다. 중간 `*`는 literal이고 query를 별도로 제거하거나 path를 normalize하지 않는다.
`status`·`headers`·`body`는 요청 조건이 아닌 반환 response metadata이며 `delay`는 응답 직전
대기 시간이다.

`set_rule` IPC와 `core/rules.rs::upsert`가 frontend보다 우선하는 storage 경계다. rule 최대
200개, id 128자/128 UTF-8 바이트, method 16자/16바이트, path 4,096자/16,384바이트,
response headers 100개(이름 256자/256바이트, 값 16,384자/65,536바이트, 합계 64,000자/
256,000바이트), body 256,000자/1,024,000바이트, collection 문자열 합계 2,000,000자/
8,000,000바이트, status 100~599, delay 0~60,000ms를 적용한다. path/header value의
control 문자는 거부하고 method/header name은 ASCII token이어야 한다. path도 parser/replay
matcher와 동일하게 ASCII만 허용한다. char는 Unicode scalar count, byte는 UTF-8 byte count이며
JS `Array.from`/`TextEncoder`와 Rust 구현이 같은 단위를 사용한다. 빈 신규 id는 UUID가 되기 전
36자 footprint를 예약한다.

검증 실패는 raw input·secret·경로를 포함하지 않는 `규칙 입력이 유효하지 않습니다`로만
응답하고 map을 변경하지 않는다. editor는 같은 validator로 add/edit/duplicate를 검사하며
invalid raw draft를 유지하고 stale id, double action, 접근성 오류를 처리한다. #282 당시에는
기존 HashMap 순회 semantics와 priority를 범위에 넣지 않았고, v0.6.0의 아래 §12가 그 선택
계약을 명시적으로 대체한다.
예시 curl은 별도 완료 범위이고, captured fixture 저장은 아래 #314 계약으로 구현했다. captured
request replay/sequence는 아래 #362/#363 계약으로 구현했다. API Playground handoff는 아래 #315
계약으로 구현하며, #282 완료 상태는 rule 설명·검증까지로 기록한다.

## 8. #314 captured fixture 저장 계약 (2026-08-27)

history의 opaque ID로 backend가 읽은 masked snapshot만 앱 전용
`app_local_data_dir()/fixtures.json`에 저장한다. fixture ID는 `fixture-<number>`로 발급하고
schema v1, 최대 200개·8 MiB 파일, method/target/header/body/timestamp의 bounded validator를
적용한다. Authorization·Cookie·token·secret·password·auth 계열 값과 known credential marker는
`[REDACTED]`, 절대/unsafe path는 `/[REDACTED_PATH]`로 바꾼다. JSON string 안의 bounded
embedded JSON도 같은 depth/node/size 경계로 재귀 sanitization하며 escaped sensitive key를
decode하고 malformed JSON-looking string은 fixed marker로 fail-closed 처리한다. frontend는
path·body를 저장 명령에 전달하지 않는다.

corrupt·oversized·symlink/non-file 저장소는 고정 오류로 fail-closed하며 기존 bytes를 자동
복구하지 않는다. app-owned path 검사, 최종 store component의 no-follow read, atomic replace,
raw-byte CAS와 process-local writer lock에 더해 persistent `.fixtures.json.lock` sidecar의
OS exclusive advisory lock을 사용한다. lock은 read/revision/compare/write와 add/edit/delete/
clear mutation을 cross-process로 직렬화하고 500ms bounded acquisition 뒤 fixed lock error를
반환한다. sidecar는 stale lock-file 삭제 없이 유지한다. mutation 전후에는 모든 부모 link 검사와
immediate parent filesystem identity를 재검증한다. 이는 path-based 보강이며 handle-relative
ancestor 보장은 아니므로 확인 사이 ancestor symlink/junction/reparse 교체 race는 남는다.
목록은 timestamp 내림차순·ID tie-break로 결정한다. fixture에서 response-rule 초안을 만들
때는 method/path만 local editor에 채우며 response metadata는 빈 값으로 둔다. API handoff와
replay/sequence는 각각 아래의 별도 계약으로 관리한다.

## 9. #315 Webhook Lab → API Playground handoff 계약 (2026-08-27)

history 또는 저장된 fixture를 `API Playground로 변환`할 때 frontend는 history/fixture의
opaque ID만 backend에 전달한다. producer는 backend-owned masked snapshot을 다시 검증해
`api-request/v1` payload를 만들고 catalog에서 `api-playground`의
`handoff:api-request/v1` capability와 설치 상태를 확인한다. URL은 origin-form을 그대로
전달하며 Webhook Lab이 host나 secret을 추측하지 않는다.

payload는 공용 `crates/applink` handoff store의 10분·10 MiB bounded envelope에 기록한다.
envelope는 `producer=webhook-lab`, `consumer=api-playground`를 가지며 process argv에는
kind와 128-bit lowercase opaque ID만 포함한다. 민감 header/query/body marker는
`${WEBHOOK_SECRET}` 같은 environment reference로만 보존하고 raw credential, 파일 경로,
clipboard, 임시 export를 사용하지 않는다.

API Playground는 cold start와 second-instance callback 모두에서 pending ID를 claim한 뒤
producer/consumer/handoff ID, expiry, 요청 method/URL/header/body를 적용 전 preview로
보여준다. preview가 request editor·History·Collection·response를 바꾸지 않으며, 사용자가
`적용`할 때만 claim을 ack/delete하고 editor에 넣는다. `취소`는 restore한다. 미설치·실행
실패·만료·손상·wrong target/kind·중복 claim·lease/storage 오류는 원문 경로와 payload를
반향하지 않는 fixed error이며 clipboard fallback이 없다.

## 10. #362 captured request replay 계약

history 또는 저장된 masked fixture의 opaque ID만 IPC로 받아 현재 실행 중인 Webhook Lab
listener에 한 건의 HTTP 요청을 전송한다. backend는 history snapshot 또는 fixture store에서
masked 값을 읽고 replay 직전에 fixture sanitizer/validator를 다시 통과시킨다. destination은
현재 server status에서 유도하며 loopback과 wildcard bind의 loopback mapping만 허용하고 DNS,
외부 주소, frontend URL은 사용하지 않는다.

replay는 body 256,000자/1,024,000바이트, history와 같은 header 경계, ASCII HTTP wire
경계, 2초 connect timeout·write/read idle timeout, write+response 전체 5초 deadline,
response header 64 KiB, process-local 1초·20건 rate limit을 적용한다. Host, Content-Length,
transfer-encoding 등 transport header는 입력에서 제거하고
client가 고정된 안전한 값으로 생성한다. 고정 `Host`·`Content-Length`·`Connection` 3개를
위해 입력 header는 97개까지 예약하며, 생성된 전체 wire header가 listener의 100개·64,000자·
256,000바이트 경계를 넘는 fixture는 replay하지 않는다. 동시에 호출된 native replay는 하나의 local lock으로
직렬화해 response sequence cursor 소비 순서를 안정화한다. 결과는 opaque source label과
status만 반환하며 response body, raw request, network/parser detail은 renderer·log로 보내지
않는다. 실패는 fixed error이고 clipboard/file/external request fallback은 없다.

listener admission도 native 경계에서 수행한다. method/target 초과는 414, header count·char·
byte 초과는 431, 선언 또는 실제 body 초과는 413, body read 오류는 408, 고정 request window
초과는 429로 응답하며 부분 요청을 history에 남기지 않는다. 서버 lifecycle은 transition lock과
accept-thread join으로 start/stop을 선형화하고, accept/handler 오류가 나면 running/address를
stale 상태로 유지하지 않는다. response delay도 50ms 단위로 running flag를 확인해 stop 시
join을 오래 붙잡지 않는다. stop은 active connection socket을 shutdown하고 replay cancellation
flag를 먼저 세워 진행 중인 native I/O도 중단한다. lifecycle lock은 replay의 주소 확인부터
bounded connect/write/read까지 유지해 stop/restart와 외부 local process의 port 재사용 사이
TOCTOU를 차단한다.

## 11. #363 response sequence/reset 계약

response rule의 기존 status/headers/body/delay를 첫 단계로 사용하고 최대 16개의 data-only
step을 순서대로 소비한다. 마지막 step은 유지하며 자동 반복하지 않는다. cursor는
process-local memory에만 존재하고 fixture·설정·handoff에 저장하지 않는다. 매치된 요청만
cursor를 전진시키며 rule 수정·삭제와 명시적인 response sequence 초기화 action은 cursor를
첫 단계로 되돌린다. rule mutation/reset과 request handler는 rules→cursor lock 순서를
공유해 stale cursor를 소비하지 않는다. 각 step은 기존 response status/delay/header/body
상한과 control/token 검사를 공유하며, native transport framing을 덮어쓸 수 있는 response
header와 비 ASCII header value는 거부한다. native writer는 `1xx`, `204`, `205`, `304`, `HEAD`
응답에 body와 `Content-Length`를 쓰지 않는다. arbitrary scripting과 distributed state는
범위 밖이다.

## 12. v0.6.0 deterministic rule·OpenAPI·Run service 계약

`ResponseRule.priority`는 `-1000..=1000`, missing/default는 0이다. live matcher, 목록 정렬,
conflict preview는 높은 priority, exact path, method-specific, 긴 trailing-star prefix,
bytewise ascending ID 순서의 단일 Rust comparator를 공유한다. 신규 rule은 preview 전에 UUID를
받고, exact/prefix 및 any/specific method overlap을 모두 분류한다. 저장 command는 rules→cursor
lock을 잡은 상태에서 preview를 다시 계산하고 conflict 확인이 없으면 map/cursor를 변경하지
않는다. 따라서 HashMap insertion order, frontend 정렬, request timing은 winner를 바꾸지 않는다.

공용 `packages/openapi`는 API Playground에서 검증해 온 JSON/YAML parser를 두 번째 consumer와
공유한다. Webhook adapter는 로컬 OpenAPI 3.0/3.1 문서에서 최대 250 paths·1,000 operations의
method/path/lowest-2xx만 preview한다. server/auth/security/request body/example은 projection에
없으며 parameter path와 `$ref`는 비활성 사유를 표시하고 wildcard로 변환하지 않는다. 적용은
빈 headers/body, delay 0, priority 0의 local editor draft일 뿐 저장이나 listener mutation이 아니다.

실행 중인 exact loopback listener만 `export_run_service_definition`을 허용한다. command는
backend-owned current executable과 새 UUID로 고정하며 renderer 입력을 사용하지 않는다. rules는
strict app-local `service-profiles/<uuid>.json`에만 저장되고 credential-shaped rule metadata 또는
response를 포함하면 거부한다. listener lifecycle lock은 상태 확인부터 profile count 검사·write까지
export를 직렬화한다. 다운로드 JSON은 Run Manager schema v1의 disabled/autoStart-false/restart-never service
한 건, loopback health check, opaque profile ID만 포함하며 response/rule/env/cwd/runtime identity를
포함하지 않는다. service process는 정확한 `--service-profile <uuid>` argv에서만 profile을 읽고
숨은 listener를 시작한다. profile은 64개·8 MiB, strict schema, no-link handle read/revalidation과
atomic write 경계를 사용한다. 이 문단 작성 당시 pending이던 Windows packaged 실행·종료는
v0.6.0 #493 hosted release acceptance를 통과했다. 사용자별 service policy 차이는 별도 환경
관찰 범위다.
