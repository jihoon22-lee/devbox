# Webhook Lab 설계 — Local Mock/Webhook Server

- 상태: 제안(Proposal) — Stage 5
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
- `Authorization`·`Cookie`·`API key` 헤더는 history에서 기본 masking.
- body 크기·history 개수·request rate 상한.
- fixture root 밖의 파일을 응답하지 못하게 한다 (safe_join, knowledge-base 패턴).
- request를 받아 외부 command를 실행하는 hook은 **MVP 제외**.

## 4. 런타임

- Rust에서 경량 HTTP 서버를 띄운다. 현재 사용 중인 `reqwest`(client)와 별개로
  서버 역할이 필요하다.
- [설계] 서버 구현: `tiny_http` 또는 `axum`. **단일 응답 파일·짧은 history용으로
  `tiny_http` 권장** (경량, 비동기 불필요, 의존 최소). 요청이 밀리면 후보 재검토.

## 5. 아키텍처

```
apps/webhook-lab/
├─ src-tauri/src/
│  ├─ core/
│  │  ├─ server.rs      # 경량 HTTP 서버 (요청 수신 → history 기록 → rule 응답)
│  │  ├─ rules.rs       # response rule 파싱·매칭 (순수)
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
무시한 HTTP token 비교를 한다. path는 저장 문자열과 요청 URL의 전체 exact match가 기본이며,
rule path의 마지막 `*`일 때만 앞부분 prefix match를 한다. 중간 `*`는 literal이고 query를
별도로 제거하거나 path를 normalize하지 않는다. `status`·`headers`·`body`는 요청 조건이
아닌 반환 response metadata이며 `delay`는 응답 직전 대기 시간이다.

`set_rule` IPC와 `core/rules.rs::upsert`가 frontend보다 우선하는 storage 경계다. rule 최대
200개, id 128자/128 UTF-8 바이트, method 16자/16바이트, path 4,096자/16,384바이트,
response headers 100개(이름 256자/256바이트, 값 16,384자/65,536바이트, 합계 64,000자/
256,000바이트), body 256,000자/1,024,000바이트, collection 문자열 합계 2,000,000자/
8,000,000바이트, status 100~599, delay 0~60,000ms를 적용한다. path/header value의
control 문자는 거부하고 method/header name은 ASCII token이어야 한다. char는 Unicode scalar
count, byte는 UTF-8 byte count이며 JS `Array.from`/`TextEncoder`와 Rust 구현이 같은 단위를
사용한다. 빈 신규 id는 UUID가 되기 전 36자 footprint를 예약한다.

검증 실패는 raw input·secret·경로를 포함하지 않는 `규칙 입력이 유효하지 않습니다`로만
응답하고 map을 변경하지 않는다. editor는 같은 validator로 add/edit/duplicate를 검사하며
invalid raw draft를 유지하고 stale id, double action, 접근성 오류를 처리한다. 이 PR은 기존
HashMap 저장/순회·id·순서·matcher semantics를 변경하지 않고 priority를 만들지 않는다.
fixture 저장, captured request replay/sequence, API Playground handoff와 예시 curl은 각각
후속 이슈 범위다. 따라서 위 MVP 목록에 적힌 fixture/handoff는 제품 설계의 전체 목표이고,
#282 완료 상태는 rule 설명·검증까지로 기록한다.
