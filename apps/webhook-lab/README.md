# webhook-lab — Webhook Lab (로컬 웹훅/콜백 서버)

API Playground가 outbound HTTP 클라이언트라면, Webhook Lab은 **inbound HTTP 요청을 받고 검사·재현**하는 로컬 서버.
산출물: `WebhookLab.exe` (`apps/webhook-lab`).

## 주요 기능

- **서버 시작/정지** — localhost bind 주소·포트 선택 (기본 `127.0.0.1`)
- **request history** — method/path별 headers/query/body/timestamp 기록
- **응답 rule** — 고정 status/header/body, delay와 대표 오류 응답(500/404 등)
- **JSON fixture** — fixture root 관리, 이 앱이 관리
- **API Playground 변환** — 수신 요청을 API Playground request(JSON)로 export

## 안전 경계

- 기본 bind `127.0.0.1`, LAN 공개(`0.0.0.0`)는 명시적 경고 + 별도 설정
- `Authorization`·`Cookie`·API key 헤더는 history에서 기본 masking
- body 크기·history 개수·request rate 상한, fixture root 밖 경로 응답 차단

## 기술

- Rust 경량 HTTP 서버(`tiny_http`)

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-webhook-lab-design.md`
