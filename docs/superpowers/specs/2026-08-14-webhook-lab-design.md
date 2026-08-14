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
