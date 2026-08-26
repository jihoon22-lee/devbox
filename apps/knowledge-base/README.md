# knowledge-base — 개인 지식 저장소

Markdown-first로 설계한 개인 지식·프로젝트·일일 기록 관리 앱. 파일 자체가 데이터의 원본이 되어 앱이 없어도 내용을 읽을 수 있다.
산출물: `Knowledge.exe` (`apps/knowledge-base`).

## 주요 기능

- **폴더/파일 탐색** — 왼쪽 트리, 생성/이름변경/이동/삭제. 우클릭·Shift+F10·Menu 키 메뉴에서 경로 복사, 탐색기 표시, 설치된 catalog 대상 앱으로 열기를 지원
- **Markdown 편집** — CodeMirror(공용 `packages/editor`) + 프리뷰 토글, Ctrl+S 저장, mermaid 다이어그램 렌더. CM6 DOM event 경유 메뉴에서 잘라내기·복사·명시적 붙여넣기·Markdown 링크 삽입을 지원
- **Wikilink / backlink** — `[[target]]`·`[[target|alias]]` 자동완성, resolved/unresolved 표시,
  Ctrl/Cmd+클릭 노트 이동과 backlink source line·column 이동. fenced/inline code와 escape된 문법은
  링크로 인덱스하지 않는다
- **안전한 이름 변경** — 파일·폴더의 새 경로와 깨질 위키링크만 diff로 먼저 표시하고 전체
  승인을 받은 뒤 하나의 one-shot transaction으로 적용. 별칭은 보존하고 title 등으로 계속
  유일하게 해석되는 링크는 불필요하게 다시 쓰지 않는다
- **검색** — 제목+본문 FTS5 (`crates/search`)
- **태그** — YAML frontmatter(`tags:`) 파싱, 태그 목록·필터
- **데일리 노트** — 날짜별 생성·연결
- **앱 간 열기** — catalog의 `Path`로 Knowledge root 안의 Markdown 노트를 열고, `Query`로 즉시 검색. cold start와 실행 중 재호출 모두 같은 pending-open 경로를 사용
- **활동 snapshot** — 오늘 작성·수정된 노트 수와 경로 없는 불투명 식별자를 Life Log용 `activity/v1` view로 발행

## 기술

- 파일을 원본(source of truth)으로 두고 SQLite는 검색용 보조 인덱스
- SQLite의 `doc_link_keys`·`wikilinks`도 재생성 가능한 보조 인덱스다. path stem·filename·title이
  정확히 한 노트에만 대응할 때 resolved로 판정하며 중복 title/filename은 ambiguous unresolved로
  처리한다. 새 노트가 생기거나 watcher가 외부 편집을 반영하면 source를 다시 쓰지 않아도 현재
  key 집합 기준으로 resolution과 backlink가 갱신된다
- wikilink schema 최초 실행에는 root의 안전한 Markdown 원문을 한 번 읽어 source position을
  복구한다. root 밖 symlink·비 Markdown·10 MiB 초과·읽기 실패 항목은 인덱스에 넣지 않고,
  일반 링크 DTO에는 절대 경로와 본문을 포함하지 않는다
- `crates/markdown` `sanitize()`로 HTML 살균, mermaid `securityLevel: "strict"`
- `core/store.rs`의 자체 `safe_join`으로 루트 밖 경로 차단
- 트리 메뉴의 filesystem·launch 명령은 실행 직전에 항목과 기존 조상을 canonicalize하고 symlink 경유 루트 탈출을 거부한다. absolute path는 사용자가 경로 복사를 선택한 경우에만 frontend에 반환하며, 다른 앱으로 열기는 catalog capability와 실제 설치 상태를 다시 검증한다
- 이름 변경 미리보기는 canonical root 안에서 파일·폴더 목적지를 다시 검증하고 root 경로 목록,
  모든 Markdown 원문, 이동 대상 내부 파일의 SHA-256 스냅샷을 만든다. root 10,000항목·스냅샷
  64 MiB·rewrite 200파일/5,000링크 상한을 넘으면 변경 전에 중단한다. plan 원문은
  `Serialize`/`Debug`하지 않는 app-managed slot 한 개에만 보관하고 opaque ID로 한 번만 적용하거나
  명시적으로 폐기한다. 파일 rename은 link index의 파일 종류가 preview와 달라지지 않도록 Markdown
  여부(`.md`/비 Markdown)를 유지해야 한다
- 적용 직전 같은 스냅샷을 다시 계산하고 destination·경로 종류·Knowledge root가 달라졌으면 전체를
  중단한다. 통과하면 링크 파일별 atomic replace, source rename, SQLite FTS/link transaction을
  수행한다. 파일 또는 DB 단계가 실패하면 이미 쓴 링크와 rename, 새로 만든 빈 parent directory를
  되돌린다. 이는 여러 파일을 한 OS primitive로 바꾸는 전역 원자성이 아니라 bounded preflight와
  파일별 atomic replace 및 rollback으로 제공하는 실행 중 오류의 all-or-rollback 계약이다. 프로세스나
  OS가 apply 도중 강제 종료되는 경우를 복구하는 영속 journal은 이 범위에 포함하지 않는다
- 폴더 삭제 시 하위 FTS/link row를 함께 제거한다. 이름 변경은 이동 대상의 읽을 수 있는 파일과
  외부에서 rewrite된 Markdown을 새 상대 경로·내용으로 같은 SQLite transaction에 재색인한다
- 자동완성은 root-relative path에서 `.md`를 뺀 canonical target을 삽입한다. raw target을 파일
  경로로 직접 열지 않으며, editor/preview/backlink 이동은 backend가 유일하게 resolve한 상대
  경로를 기존 canonical root·`.md`·10 MiB 검증 경계에서 다시 연다
- inbound Path는 canonical Knowledge root 내부의 실제 `.md` 파일만 허용하고 10 MiB로 제한한다. 실패 시 raw path·OS 오류를 UI에 반향하지 않는다
- `crates/integration`의 multi-view envelope을 사용해 `%LOCALAPPDATA%\devbox\integration\knowledge-base\v1\summary.json`을 원자 교체한다
- `activity/v1` entry는 `notesModifiedToday`, `lastModifiedAtMs`, `noteIds`, `identifiersTruncated`만 포함한다. `noteIds`는 DB row에서 만든 `note-<양의 정수>` 형식이며 최대 512개다
- 노트 경로·제목·본문·tag·credential은 snapshot에 포함하지 않는다. 앱 저장·생성·이름변경·삭제·데일리 노트 생성과 watcher가 감지한 외부 편집 뒤에 같은 snapshot을 best-effort로 갱신한다
- clipboard IPC는 `allow-read-text`만 허용하며 편집기에서 사용자가 붙여넣기를 고른 순간의 plain text만 읽는다. clipboard history나 background 수집은 하지 않는다
- 이름 변경은 외부 binary, network, runtime download 없이 동작한다. 직접 추가한 `sha2 0.11`은
  기존 workspace lock과 고지에 있던 MIT/Apache-2.0 dependency이며, preview UI는 기존
  `@devbox/diff-view`를 세 번째 소비자로 재사용한다

## 데이터

- 노트 파일: `Documents\Knowledge`
- 검색 인덱스: `%LOCALAPPDATA%\com.devbox.knowledgebase\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
