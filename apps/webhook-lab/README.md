# webhook-lab — Webhook Lab (로컬 웹훅/콜백 서버)

API Playground가 outbound HTTP 클라이언트라면, Webhook Lab은 **inbound HTTP 요청을 받고 검사·재현**하는 로컬 서버.
산출물: `WebhookLab.exe` (`apps/webhook-lab`).

## 주요 기능

- **서버 시작/정지** — localhost bind 주소·포트 선택 (기본 `127.0.0.1`)
- **request history** — method/path별 headers/query/body/timestamp 기록
- **응답 rule** — 고정 status/header/body, delay와 대표 오류 응답(500/404 등)
- **대상별 컨텍스트 메뉴** — history의 마스킹 복사·확인 후 원본 복사·마스킹 헤더
  복사·개별 삭제, rule의 편집·복제·삭제. 우클릭과 `Shift+F10`/Menu 키를 지원하고 닫은 뒤
  원래 행으로 포커스를 돌려보낸다.

JSON fixture 관리·API Playground 변환은 미구현이며, 설계 문서(`docs/superpowers/specs/2026-08-14-webhook-lab-design.md`)의 향후 항목이다.
컨텍스트 메뉴에는 계획된 위치를 먼저 표시하되 API Playground 변환은 `api-request/v1`
handoff(#315), 예시 curl 복사는 안전한 quoting 기능(#283)이 구현될 때까지 비활성화한다.

## 안전 경계

- 기본 bind `127.0.0.1`, LAN 공개(`0.0.0.0`)는 명시적 경고 + 별도 설정
- `Authorization`·`Cookie`·API key 헤더는 일반 history DTO와 기본 복사에서 masking
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
