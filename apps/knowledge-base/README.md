# knowledge-base — 개인 지식 저장소

Markdown-first로 설계한 개인 지식·프로젝트·일일 기록 관리 앱. 파일 자체가 데이터의 원본이 되어 앱이 없어도 내용을 읽을 수 있다.
산출물: `Knowledge.exe` (`apps/knowledge-base`).

## 주요 기능

- **폴더/파일 탐색** — 왼쪽 트리, 생성/이름변경/이동/삭제
- **Markdown 편집** — CodeMirror(공용 `packages/editor`) + 프리뷰 토글, Ctrl+S 저장, mermaid 다이어그램 렌더
- **검색** — 제목+본문 FTS5 (`crates/search`)
- **태그** — YAML frontmatter(`tags:`) 파싱, 태그 목록·필터
- **데일리 노트** — 날짜별 생성·연결
- **앱 간 열기** — catalog의 `Path`로 Knowledge root 안의 Markdown 노트를 열고, `Query`로 즉시 검색. cold start와 실행 중 재호출 모두 같은 pending-open 경로를 사용
- **활동 snapshot** — 오늘 작성·수정된 노트 수와 경로 없는 불투명 식별자를 Life Log용 `activity/v1` view로 발행

## 기술

- 파일을 원본(source of truth)으로 두고 SQLite는 검색용 보조 인덱스
- `crates/markdown` `sanitize()`로 HTML 살균, mermaid `securityLevel: "strict"`
- `core/store.rs`의 자체 `safe_join`으로 루트 밖 경로 차단
- inbound Path는 canonical Knowledge root 내부의 실제 `.md` 파일만 허용하고 10 MiB로 제한한다. 실패 시 raw path·OS 오류를 UI에 반향하지 않는다
- `crates/integration`의 multi-view envelope을 사용해 `%LOCALAPPDATA%\devbox\integration\knowledge-base\v1\summary.json`을 원자 교체한다
- `activity/v1` entry는 `notesModifiedToday`, `lastModifiedAtMs`, `noteIds`, `identifiersTruncated`만 포함한다. `noteIds`는 DB row에서 만든 `note-<양의 정수>` 형식이며 최대 512개다
- 노트 경로·제목·본문·tag·credential은 snapshot에 포함하지 않는다. 앱 저장·생성·이름변경·삭제·데일리 노트 생성과 watcher가 감지한 외부 편집 뒤에 같은 snapshot을 best-effort로 갱신한다

## 데이터

- 노트 파일: `Documents\Knowledge`
- 검색 인덱스: `%LOCALAPPDATA%\com.devbox.knowledgebase\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
