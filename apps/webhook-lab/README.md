# webhook-lab — Webhook Lab (로컬 웹훅/콜백 서버)

API Playground가 outbound HTTP 클라이언트라면, Webhook Lab은 **inbound HTTP 요청을 받고 검사·재현**하는 로컬 서버.
산출물: `WebhookLab.exe` (`apps/webhook-lab`).

## 주요 기능

- **서버 시작/정지** — localhost bind 주소·포트 선택 (기본 `127.0.0.1`)
- **request history** — method/path별 headers/query/body/timestamp 기록
- **응답 rule** — 고정 status/header/body, delay와 대표 오류 응답(500/404 등)
- **rule 설명** — method 대소문자 무시·빈 값 전체 적용, path 정확 일치·후행 `*` wildcard,
  status 응답 코드, delay 밀리초 의미를 편집 중 항상 표시
- **대상별 컨텍스트 메뉴** — history의 마스킹 복사·확인 후 원본 복사·마스킹 헤더
  복사·개별 삭제, rule의 편집·복제·PowerShell/POSIX curl 복사·삭제. 우클릭과 `Shift+F10`/Menu 키를 지원하고 닫은 뒤
  원래 행으로 포커스를 돌려보낸다.
- **예시 curl** — 실행 중인 서버의 fresh bind 주소와 rule의 method/path를 반영한 실행 가능한
  요청을 `PowerShell curl.exe` 또는 `POSIX sh curl` 형식으로 복사한다. rule의
  status·응답 headers·응답 body는 `--include`로 실제 응답을 확인할 수 있도록 안전한 주석으로
  함께 표시하며, Authorization·Cookie·API key·token·password 계열 값과 알려진 token 형태는
  `[REDACTED]`로 마스킹한다. wildcard path는 backend trailing-`*` matcher와 일치하는
  concrete sample로 바꾸고, shell별 독립 quoting과 `--globoff`·`--path-as-is`로 command/URL
  확장과 curl의 path dot-segment 정규화를 막는다.

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
- 여러 rule이 동시에 매치되는 경우 `HashMap` 순회 순서는 우선순위나 결정성 계약이 아니다.
  겹치는 rule 중 어느 것이 선택되는지에 의존하지 말고, method/path 조합이 겹치지 않게
  작성한다.
- 편집기는 status를 `100~599` 정수, delay를 `0~60000ms` 정수로 제한한다. Rust wire type의
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
- method는 `null`(전체 method) 또는 ASCII HTTP token이며 최대 16자/16바이트다. 편집기의
  빈 값은 `null`로 변환하고, `Some("")`이나 공백/제어 문자는 저장하지 않는다.
- path는 최대 4,096자/16,384 UTF-8 바이트이며 `/`로 시작하고 모든 Unicode control 문자를
  포함할 수 없다. 문자열을 decode, normalize, query 제거하지 않는다. 매칭은 저장된 path와
  요청 URL의 전체 문자열 exact 비교이거나 **마지막** `*` 하나에 대한 prefix 비교이며,
  중간 `*`는 literal로 남는다.
- response headers는 최대 100개다. 각 이름은 HTTP token, 최대 256자/256바이트, 각 값은
  최대 16,384자/65,536바이트이고 control 문자를 허용하지 않는다. 이름과 값을 합한 rule별
  전체는 64,000자/256,000바이트 이하여야 한다.
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
검사한다. 이 PR은 기존 rule id·저장 순서·HashMap 순회와 trailing-star matcher를 바꾸거나
priority를 도입하지 않는다.

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
  credential 표시는 `[REDACTED]`가 된다. 절대 URL·`..`/`.`·역슬래시·잘못된 percent
  encoding·token-shaped path는 고정 `/[REDACTED_PATH]`로 바꾸고, 안전한 query만
  보존한다. 입력을 넘으면 부분 fixture를 만들지 않는다.
- 파일은 atomic replace와 raw-byte compare-and-swap으로 저장한다. corrupt·oversized·
  symlink/non-file store는 고정 오류로 fail-closed하고 원본 파일을 자동 복구·덮어쓰지
  않는다. 목록은 capture timestamp 내림차순, 동일 timestamp에서는 ID 순으로 정렬한다.
- fixture의 `응답 rule 초안`은 method/path만 편집기에 채우며 status 200·빈 response
  headers/body·delay 0으로 시작한다. rule 저장은 별도 사용자 동작이고, API Playground
  `api-request/v1` handoff(#315)나 request replay/sequence(#362)는 이 범위에 없다.

API Playground 변환 메뉴는 `api-request/v1` handoff(#315)가 준비될 때까지 계속 비활성화한다.

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
  출력한다. raw secret reveal·request replay는 제공하지 않는다.
- builder bounds는 path 4,096자, headers 100개/이름 256자/값 16,384자/합계 64,000자,
  body 256,000자, JSON depth 32·node 10,000개·string 64,000자, 최종 출력 512,000자다.
  status는 100~599, delay는 0~60,000ms 정수만 허용한다. parsing·URI·clipboard 예외는
  화면에 원문을 반향하지 않고 고정된 안전 오류로 처리한다.
- stale rule/address 재검증, copy busy lock, menu keyboard(`Shift+F10`/Menu key)와 Escape
  focus restore를 유지한다. 서버 중지·규칙 삭제·clipboard 실패는 복사 없이 고정 alert로
  알린다.

## 안전 경계

- 기본 bind `127.0.0.1`, LAN 공개(`0.0.0.0`)는 명시적 경고 + 별도 설정
- `Authorization`·`Cookie`·API key 헤더는 일반 history DTO와 기본 복사에서 masking
- example curl도 같은 민감정보 경계를 따르며 raw secret reveal이나 request replay를 제공하지 않는다
- 원본 헤더는 persistence·log·snapshot·일반 DTO에 넣지 않고 현재 프로세스의 bounded history
  entry에만 보관한다. 사용자가 원본 복사 경고를 확인한 뒤 정확한 history ID로 요청한 한 번의
  clipboard write에만 사용한다.
- body 크기 상한(256K자)·history 개수 상한(200건)·요청당 보관 헤더 상한(100개/총 64K자)
- history를 비운 뒤에도 프로세스 안에서 ID를 재사용하지 않아 열린 메뉴가 새 요청을 가리키지 않는다.

## 기술

- Rust 경량 HTTP 서버(`tiny_http`)

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-webhook-lab-design.md`
