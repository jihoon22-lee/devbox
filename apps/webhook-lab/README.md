# webhook-lab — Webhook Lab (로컬 웹훅/콜백 서버)

API Playground가 outbound HTTP 클라이언트라면, Webhook Lab은 **inbound HTTP 요청을 받고 검사·재현**하는 로컬 서버.
산출물: `WebhookLab.exe` (`apps/webhook-lab`).

## 주요 기능

- **서버 시작/정지** — localhost bind 주소·포트 선택 (기본 `127.0.0.1`)
- **request history** — method/path별 headers/query/body/timestamp 기록
- **응답 rule** — 고정 status/header/body, delay와 대표 오류 응답(500/404 등)
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

JSON fixture 관리·API Playground 변환은 미구현이며, 설계 문서(`docs/superpowers/specs/2026-08-14-webhook-lab-design.md`)의 향후 항목이다.
API Playground 변환은 `api-request/v1` handoff(#315)가 준비될 때까지 비활성화한다.

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
