# api-playground — API Playground v0.3.2

로컬 REST API 테스트 앱. 데스크톱 실행에서는 Rust backend가 HTTP 클라이언트를 담당해 **CORS 제약 없이** 요청한다.
산출물: `ApiPlayground.exe` (`apps/api-playground`).

## 주요 기능

- **요청 작성** — Method, URL, Params/Header table/Body(JSON·form·raw). Header table은 같은 이름의
  행과 입력 순서를 유지하고 행별 enabled, 복제·삭제, 현재 환경 secret reference 삽입을 지원한다.
- **응답 보기** — 상태코드·시간·크기, JSON Pretty/폴드, Raw
- **Auth 프리셋** — Basic / Bearer / API Key
- **History / Collection** — 최근 요청과 저장 요청을 v2 형식으로 보존·재호출. 항목 우클릭 또는
  `Shift+F10`/Menu 키로 복제·이름 변경·확인 후 삭제·마스킹 cURL 복사를 실행한다.
- **cURL 변환** — 기본 masking cURL 복사, 확인 후 원문 cURL 1회 복사
- **환경(environment)·비밀(secret)** — URL·params·headers·body·auth에서 `${NAME}`과
  `{{NAME}}` 참조를 지원하고, DPAPI로 보호된 secret은 backend가 요청 직전에 메모리에서만
  해제한다 (`crates/secrets`). Header table의 picker에는 현재 환경의 봉인된 secret 이름만
  표시하고 `${NAME}`을 삽입하며 frontend로 DPAPI secret을 unseal하지 않는다.

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
- **요청·응답 redaction** — response headers/body, final URL, redirect 위치와 오류는 secret,
  Authorization, Cookie 및 민감한 token 패턴을 redaction한다. 모든 cross-origin redirect에서는
  Authorization/Cookie/API-key 헤더와 auth를 다음 요청에 전달하지 않고 요청 body도 억제한다.
  메서드를 보존하는 307/308 redirect에도 동일하게 적용하고, 목적지 URL 자체에 민감정보가
  포함된 cross-origin redirect는 follow 전에 차단해 fail-closed로 처리한다.
- **cURL** — 화면과 기본 복사는 masking된 결과만 사용한다. 확인 대화상자 뒤의 원문 복사는
  데스크톱 backend가 일회성으로 생성하며 저장하지 않는다.
- **항목 메뉴** — History·Collection 메뉴는 v2에 저장된 마스킹 request만 사용한다. 복제와
  이름 변경도 backend sanitizer 및 read-back 검증을 다시 통과하며, 삭제는 확인 전 저장소를
  변경하지 않는다. History의 선택적 표시 이름은 기존 v2 항목과 하위 호환된다.
- **브라우저 preview** — Tauri 밖에서는 `fetch` 미리보기만 제공하므로 CORS 제한이 있다.
  DPAPI secret이 포함된 요청과 secret 해제·원문 cURL은 차단하며, 응답·URL도 미리보기 경계에서
  redaction한다.

## 기술

- Rust(`reqwest`)가 직접 요청 → 브라우저 CORS 없음
- 공용 패키지 `packages/tokens`, `packages/context-menu` 사용

## 개발

- 순수 로직: `src-tauri/src/commands/request.rs` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
