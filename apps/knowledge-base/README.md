# knowledge-base — 개인 지식 저장소

Markdown-first로 설계한 개인 지식·프로젝트·일일 기록 관리 앱. 파일 자체가 데이터의 원본이 되어 앱이 없어도 내용을 읽을 수 있다.
산출물: `Knowledge.exe` (`apps/knowledge-base`).

## 주요 기능

- **폴더/파일 탐색** — 왼쪽 트리, 생성/이름변경/이동/삭제
- **Markdown 편집** — CodeMirror(공용 `packages/editor`) + 프리뷰 토글, Ctrl+S 저장, mermaid 다이어그램 렌더
- **검색** — 제목+본문 FTS5 (`crates/search`)
- **태그** — YAML frontmatter(`tags:`) 파싱, 태그 목록·필터
- **데일리 노트** — 날짜별 생성·연결

## 기술

- 파일을 원본(source of truth)으로 두고 SQLite는 검색용 보조 인덱스
- `crates/markdown` `sanitize()`로 HTML 살균, mermaid `securityLevel: "strict"`
- `crates/filesystem` safe_join으로 루트 밖 경로 차단

## 데이터

- 노트 파일: `Documents\Knowledge`
- 검색 인덱스: `%LOCALAPPDATA%\com.devbox.knowledgebase\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

