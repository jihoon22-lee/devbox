# knowledge-base — 개인 지식 저장소

Markdown-first로 설계한 개인 지식·프로젝트·일일 기록 관리 앱. 파일 자체가 데이터의 원본이 되어 앱이 없어도 내용을 읽을 수 있게 한다.
산출물: `Knowledge.exe`. 모노레포 위치: `devbox/apps/knowledge-base`.

## 1. 목표
- Markdown 파일을 계층 폴더로 관리하고, 편집·검색·백링크·태그 제공
- 파일을 원본(source of truth)으로 두고, SQLite는 검색/백링크용 보조 인덱스만
- Git 연동으로 버전 관리, 매일 데일리 노트 작성

## 2. 핵심 기능

### MVP (v1)
| 기능 | 설명 |
|---|---|
| 폴더/파일 탐색 | 왼쪽 트리, 생성/이름변경/이동/삭제 |
| Markdown 편집 | CodeMirror + 프리뷰 토글, 저장(Ctrl+S) |
| 검색 | 제목+본문 FTS5 (everything-plus와 동일 방식) |
| 태그 | YAML frontmatter(`tags:`) 파싱, 태그 목록/필터 |
| 데일리 노트 | 날짜별 생성·연결 |

### v2+
- 백링크 `[[문서명]]` 감지 → 역링크 패널
- 첨부파일(이미지/PDF) 폴더 관리
- Git 커밋/로그 UI (wsl-dashboard git 헬퍼 재사용)
- 퀵캡처 (전역 단축키로 빠른 메모)

## 3. 기술 설계

### 데이터 구조
```
KnowledgeRoot/            # config로 지정 (기본: Documents/Knowledge)
├ Projects/
├ Notes/
├ Journal/                # 데일리 노트
├ Reference/
└ Archive/
```
- 파일은 단순 마크다운: `FamilyCard.md`, `2026-08-10.md`
- 메타는 frontmatter: `---\ntitle: ...\ntags: [rust, tauri]\n---`

### 아키텍처
```
React (트리/에디터/검색)
  ↓ invoke
commands (file ops, search, backlinks, git)
  ↓
core/fs_store (파일 I/O) + db (SQLite 인덱스)
  ↓
KnowledgeRoot 폴더
```
- 파일 변경은 직접 디스크 반영, DB는 인덱스만 (원본 우선)
- 새 파일/변경 시 `notify`로 DB 갱신 (`crates/filesystem` watch 모듈 재사용)

### Rust 모듈
- `commands/files.rs` — `list_tree()`, `read_file(path)`, `write_file(path, content)`, `create_file()`, `rename()`, `delete()`
- `commands/search.rs` — FTS5 제목/본문 검색
- `commands/tags.rs` — `list_tags()`, `files_by_tag(tag)`
- `commands/backlinks.rs` — `get_backlinks(doc)` (v2)
- `commands/git.rs` — `commit()`, `log()` (v2, wsl-dashboard 헬퍼 재사용)
- `core/frontmatter.rs` — frontmatter 파서 (단위 테스트 대상)
- `core/markdown.rs` — 본문 추출, `[[링크]]` 감지
- `db.rs` — SQLite 인덱스 스키마 (everything-plus의 `crates/database`·`crates/search` 재사용)

### DB 스키마 (보조 인덱스)
```sql
CREATE TABLE docs (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  title TEXT,
  tags TEXT,            -- JSON 배열
  modified_ts INTEGER
);
CREATE VIRTUAL TABLE docs_fts USING fts5(title, body, content='docs');
CREATE TABLE links (from_path TEXT, to_title TEXT);  -- v2 백링크
```

## 4. UI 설계
```
[Knowledge]  [🔍 검색...]
├ Knowledge      ┌──────────────────────────────┐
│ ├ Projects     │ Tauri-study.md      [저장]   │
│ │ ├ FamilyCard│                              │
│ │ └ port-manager       │ # Tauri 스터디               │
│ ├ Notes        │ tags: [tauri, rust]         │
│ ├ Journal      │ ...                         │
└ └ Archive      └──────────────────────────────┘
   [트리]           [에디터 + 프리뷰 탭]
   태그: rust(3) tauri(2) ...
   백링크(v2): port-manager.md, ...
```
- 3분할: 트리 | 에디터(CodeMirror+프리뷰) | 메타(태그/백링크)
- 데일리 노트 버튼 → 오늘 날짜 파일 생성/열기

## 5. 구현 단계
1. 스캐폴드 + 폴더 구조/파일 I/O command + 트리 UI
2. CodeMirror 에디터 + 저장 + 프리뷰
3. frontmatter 파서 + 태그 인덱스 + 태그 UI
4. FTS5 검색 + 검색 결과 패널
5. 파일 생성/이름변경/이동/삭제 (디스크 동기화)
6. 데일리 노트
7. v2: 백링크, 첨부, Git UI
8. Windows 빌드 검증

## 6. 테스트
- Rust: frontmatter 파서, `[[링크]]` 추출, FTS5 검색 (임시 루트 픽스처)
- 통합: 파일을 외부에서 수정 → watcher로 인덱스 갱신 확인
- 프론트: 트리 상태 관리, 검색 vitest

## 7. 확장/연계
- everything-plus: 검색 엔진 재사용 (`crates/search`, `crates/filesystem` 공유)
- life-log: 데일리 노트 작성 건수를 일일 기록의 소스로
- 공통 추출 후보: frontmatter 파서, `packages/ui`(CodeMirror 에디터·트리 컴포넌트)

## 8. 완료 정의(Done)
- 트리·편집·저장·검색·태그·데일리 노트 전부 동작
- 앱 없이 폴더를 열어도 문서를 읽을 수 있음 (파일 원본 보장)
- v2 백링크·git 동작, Windows 빌드 성공
