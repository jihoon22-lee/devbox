# everything-plus — Everything+ 개인 검색기

로컬 파일을 이름·내용으로 초고속 검색하는 앱. Rust 인덱싱·FTS5·파일 감시를 다루는 성능 중심 프로젝트.
산출물: `EverythingPlus.exe` (`apps/everything-plus`).

## 주요 기능

- **파일명 검색** — FTS5 인덱스, 파일명·경로 부분 일치, 밀리초 응답
- **내용 검색** — 텍스트 파일(TXT/MD/JSON/CSV/소스코드) 내용 FTS5, 스니펫 하이라이트
- **정규식 모드** — regex 검색
- **인덱스 관리** — 검색 루트(드라이브/폴더) 추가·제외 규칙, re-index 진행률 표시, 파일 감시로 최신성 유지
- **결과 작업** — 열기·폴더에서 보기·경로/파일명 복사와 설치된 `path` capability 앱으로 열기 context menu
- **앱 간 검색** — catalog `Query`를 cold start와 실행 중 재호출에서 수신해 name/non-regex 검색으로 즉시 연결

## 내용 인덱스 경계

`index content`를 켠 검색 루트만 내용 인덱싱 대상이 된다. Everything+는 파일을 전부
읽지 않고 `src-tauri/src/core/content.rs`의 명시적 source/Markdown/plain-text 확장자와
`README`, `LICENSE`, `Dockerfile`, `.gitignore` 같은 소수의 이름만 선택한다. 현재 text
extractor 버전은 `text-v1`이며 PDF/Office 컨테이너, OCR, semantic search는 이 기능에
포함하지 않는다.

- UTF-8(선택적 BOM)과 UTF-16 little/big endian(BOM, 안정적으로 식별 가능한 BOM 없는
  텍스트)을 strict decode한다. invalid UTF-8/UTF-16, binary/NUL 데이터는 검색 hit가 되지
  않는 `unsupported_encoding` 상태로 남는다.
- 파일 하나는 최대 20 MiB, 보관 text는 최대 2,000,000 Unicode scalar characters,
  후보 하나의 cooperative processing budget은 10초다. 초과 text는 앞부분만 저장하고
  `truncated=true`, `error_code=text_limit`을 기록한다. oversized/read/race/timeout/
  sensitive 파일도 filename index는 유지하면서 고정 `content_status`와 `error_code`만
  기록한다.
- `.env`, credential/netrc/npmrc/secrets 계열 파일과 private-key 확장자/이름은 읽지 않고
  `skipped_sensitive`로 기록한다. FTS snippet은 Authorization/Bearer/password/token/
  secret/API-key/private-key 형태와 독립적으로 노출된 common provider token·AWS access key·
  JWT 형태를 다시 redaction한 뒤 최대 4,096자까지만 UI에 보낸다.
  원문 내용은 app-local SQLite FTS에만 있고 network, log, telemetry, 다른 app snapshot으로
  복제하지 않는다.
- 전체/루트 재인덱스는 250개 파일 배치로 DB lock을 양보하고 UI에서 진행률·indexed/
  truncated/failed 수·마지막 시각을 확인할 수 있다. 실행 중 `Cancel`은 안전한 배치와
  파일 경계에서 협력적으로 중지하며, 이미 커밋된 부분 인덱스는 다음 `Re-index`로
  수렴한다. watcher 증분 반영도 동일 extractor와 bounds를 사용하고 읽기 전후의 크기와
  수정 시각을 열린 파일 및 경로 양쪽에서 다시 확인해 변경된 읽기는
  `changed_during_read`로 폐기한다.
- schema v2 migration은 사용자가 등록한 roots는 보존하고 파생 files/content FTS와
  metadata를 재생성한다. `content_status`, `extractor_version`, `truncated`,
  `indexed_at`, `error_code`, `encoding`, `text_chars`를 함께 저장하므로 검색 결과와
  상태 화면이 단순히 "없음"과 실패를 혼동하지 않는다.

## 기술

- 공용 크레이트 `crates/filesystem`(제한 순회)·`crates/search`(FTS5) 사용
- 백그라운드 watcher + 주기 재스캔
- inbound Query와 search input은 UTF-8 4 KiB 및 control-character 경계를 적용하며 원문을
  로그·오류·지속 저장소에 남기지 않는다
- 파일명 검색은 기본 200개·정규식 prefilter 최대 2,000개, 내용 검색은 최대 200개로
  서로 다른 bounded result 계약을 유지한다
- 다른 앱으로 열기는 catalog capability와 설치 manifest를 모두 통과한 대상만 표시하고, 실행 직전 기존 절대 파일 경로를 재검증한다

## 데이터

- 인덱스: `%LOCALAPPDATA%\com.devbox.everythingplus\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
