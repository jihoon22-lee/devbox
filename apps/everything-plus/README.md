# everything-plus — Everything+ 개인 검색기

로컬 파일을 이름·내용으로 초고속 검색하는 앱. Rust 인덱싱·FTS5·파일 감시를 다루는 성능 중심 프로젝트.
산출물: `EverythingPlus.exe` (`apps/everything-plus`).

## 주요 기능

- **파일명 검색** — FTS5 인덱스, 파일명·경로 부분 일치, 밀리초 응답
- **내용 검색** — 텍스트 파일(TXT/MD/JSON/CSV/소스코드) 내용 FTS5, 스니펫 하이라이트
- **정규식 모드** — regex 검색
- **인덱스 관리** — 검색 루트(드라이브/폴더) 추가·제외 규칙, re-index 진행률 표시, 파일 감시로 최신성 유지
- **결과** — 경로·확장자·크기·수정시각, 더블클릭으로 탐색기 열기

## 기술

- 공용 크레이트 `crates/filesystem`(제한 순회)·`crates/search`(FTS5) 사용
- 백그라운드 watcher + 주기 재스캔

## 데이터

- 인덱스: `%LOCALAPPDATA%\com.devbox.everythingplus\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

상세 계획: [PLAN.md](./PLAN.md)
