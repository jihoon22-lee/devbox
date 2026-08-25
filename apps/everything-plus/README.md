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

## 기술

- 공용 크레이트 `crates/filesystem`(제한 순회)·`crates/search`(FTS5) 사용
- 백그라운드 watcher + 주기 재스캔
- inbound Query는 1~512자로 제한하며 원문을 로그·오류·지속 저장소에 남기지 않는다
- 다른 앱으로 열기는 catalog capability와 설치 manifest를 모두 통과한 대상만 표시하고, 실행 직전 기존 절대 파일 경로를 재검증한다

## 데이터

- 인덱스: `%LOCALAPPDATA%\com.devbox.everythingplus\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
