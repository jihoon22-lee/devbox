# api-playground — API Playground

Postman을 축소 재현한 로컬 REST API 테스트 앱. Rust가 HTTP 클라이언트를 담당해 **CORS 제약 없이** 요청할 수 있다.
산출물: `ApiPlayground.exe` (`apps/api-playground`).

## 주요 기능

- **요청 작성** — Method, URL, Params/Headers/Body(JSON·form·raw)
- **응답 보기** — 상태코드·시간·크기, JSON Pretty/폴드, Raw
- **Auth 프리셋** — Basic / Bearer / API Key
- **History** — 최근 요청 저장·재호출
- **cURL 변환** — 현재 요청을 curl 명령으로 복사
- **환경(environment)·비밀(secret)** — `{{변수}}` 치환, DPAPI 보호 (`crates/secrets`)

## 기술

- Rust(`reqwest`)가 직접 요청 → 브라우저 CORS 없음
- 공용 패키지 `packages/tokens` 사용

## 개발

- 순수 로직: `src-tauri/src/commands/request.rs` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

