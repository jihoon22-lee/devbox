# api-playground — API Playground

Postman을 축소 재현한 로컬 REST API 테스트 앱. Rust가 HTTP 클라이언트를 담당하므로 CORS 제약 없이 요청할 수 있는 것이 핵심 장점.
산출물: `ApiPlayground.exe`. 모노레포 위치: `devbox/apps/api-playground`.

## 1. 목표
- GET/POST/PUT/DELETE 요청을 만들고 응답을 확인하는 데스크톱 도구
- 요청 이력(history)·컬렉션·환경변수로 재사용성 제공
- Rust(`reqwest`)가 직접 요청 → 브라우저 CORS 문제 없음

## 2. 핵심 기능

### MVP (v1)
| 기능 | 설명 |
|---|---|
| 요청 작성 | Method, URL, Params/Headers/Body(JSON·form·raw) |
| 응답 보기 | 상태코드·시간·크기, JSON Pretty/폴드, Raw |
| Auth | Basic / Bearer / API Key 프리셋 |
| History | 최근 요청 저장·재호출 |
| 새로고침 유형 | 본문 없이 method/url/headers만으로 재요청 |

### v2+
- 컬렉션(폴더 구조로 요청 저장)
- 환경변수 `{{base_url}}` 치환, 환경 전환
- 파일 업로드, 응답 헤더 뷰, cURL 내보내기
- WebSocket/SSE(선택), GraphQL(선택)

## 3. 기술 설계

### 데이터 흐름
```
React (요청 빌더/응답 뷰)
  ↓ invoke('send_request', payload)
Rust reqwest (비동기)
  ↓
HTTP API
```
- 브라우저가 아닌 Rust가 요청 → CORS 불필요, 프록시/인증 제어 자유
- 타임아웃·리다이렉트 설정을 command 파라미터로 전달

### Rust 모듈
- `commands/request.rs` — `send_request(req: ApiRequest) -> Result<ApiResponse, String>`
- `commands/history.rs` — `save_history(item)`, `list_history(limit)`, `delete_history(id)`
- `commands/env.rs` — `list_envs()`, `set_env(name, vars)` (v2)
- `commands/collections.rs` — CRUD (v2)
- `core/req_builder.rs` — ApiRequest → reqwest 구조 변환, `{{var}}` 치환 (단위 테스트 대상)
- `core/body_preview.rs` — JSON 포맷/축약 (developer-toolbox 포맷터 재사용)
- `db.rs` — history/collections/env SQLite (`crates/database` 재사용)

### 데이터 모델
```rust
struct ApiRequest {
  method: String, url: String,
  headers: Vec<KeyValue>,
  params: Vec<KeyValue>,          // query string으로 병합
  body: Option<RequestBody>,      // { kind: "json"|"form"|"raw", text: String }
  auth: Option<AuthConfig>,       // { kind: "none"|"basic"|"bearer"|"apikey", ... }
  timeout_ms: u64,
}
struct ApiResponse {
  status: u16, status_text: String,
  headers: Vec<KeyValue>, duration_ms: u64, size_bytes: usize,
  body: String, is_json: bool,
}
```

### DB 스키마
```sql
CREATE TABLE history (
  id INTEGER PRIMARY KEY, saved_at INTEGER NOT NULL,
  request_json TEXT NOT NULL, status INTEGER, duration_ms INTEGER
);
CREATE TABLE collections (id INTEGER PRIMARY KEY, name TEXT, folder TEXT, request_json TEXT);
CREATE TABLE envs (id INTEGER PRIMARY KEY, name TEXT, vars_json TEXT);
```

## 4. UI 설계
```
[GET ▼] https://api.example.com/users            [SEND]
Tabs: [Params] [Headers] [Body] [Auth]          [History ▾]
────────────────────────────────────────────
200 OK    183ms    2.4KB
{ "id": 1, "name": "John" }
```
- 상단 요청 바 + 메소드 드롭다운(색상 코드), 하단 응답 2분할(Raw/JSON)
- 좌측 탭: History / Collections (v2)
- 키-값 편집기 컴포넌트(Params/Headers 공용)

## 5. 구현 단계
1. 스캐폴드 + `send_request` command (reqwest 기본 GET) + 최소 UI
2. 요청 빌더 (method/params/headers/body/auth) + 키-값 편집기
3. 응답 뷰 (상태·시간·크기, JSON 프리티/하이라이트)
4. History 저장·목록·재호출 (SQLite)
5. 에러·타임아웃 처리 + 단위 테스트 (req_builder, 치환)
6. v2: 환경변수 치환, 컬렉션, cURL 내보내기
7. Windows 빌드 검증

## 6. 테스트
- Rust: req_builder(헤더/본문/auth/변수 치환) 유닛 테스트, 로컬 테스트 서버(예: `axum`)로 실제 요청 통합 테스트
- 프론트: 키-값 편집기, JSON 뷰어 vitest

## 7. 확장/연계
- developer-toolbox: JSON 포맷터·JWT 디코더를 응답 탭에 재사용
- wsl-dashboard: WSL 내부 API 테스트로 자연스럽게 연결
- 공통 추출 후보: `packages/ui`(키-값 편집기·JSON 응답 뷰어·CodeMirror JSON)

## 8. 완료 정의(Done)
- 4개 메소드 + 파라미터/헤더/본문/인증 요청·응답 확인 동작
- History 저장/재호출, 로컬 테스트 서버 통합 테스트 통과
- Windows 배포 빌드 성공
