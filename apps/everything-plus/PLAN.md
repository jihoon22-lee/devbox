# everything-plus — Everything+ 개인 검색기

로컬 파일을 이름+내용으로 초고속 검색하는 앱. Rust 인덱싱·FTS5·파일 감시를 다루는 성능 중심 프로젝트.
산출물: `EverythingPlus.exe`. 모노레포 위치: `devbox/apps/everything-plus`.

## 1. 목표
- 파일명·경로·확장자·크기·수정시각 인덱스를 초고속(밀리초) 검색
- 내용 검색(FTS5)은 v2부터, PDF/DOCX 등은 v3로 단계 확장
- 인덱스 최신성 유지 (파일 감시 + 주기 재스캔)

## 2. 핵심 기능

### MVP (v1) — 파일명 인덱스
| 기능 | 설명 |
|---|---|
| 인덱싱 | 루트(드라이브/폴더) 설정, 병렬 walk, 제외 규칙 |
| 검색 | 파일명 부분 일치, 정렬(이름/크기/수정시각) |
| 결과 | 경로·확장자·크기·수정시각, 더블클릭 → 탐색기 열기 |
| 상태 | 인덱스 진행률/건수, 루트 관리, 수동 재인덱스 |

### v2 — 내용 검색
- TXT/MD/JSON/CSV/소스코드 확장자만 인덱싱 → FTS5 토큰화 저장
- 내용 쿼리 결과에서 스니펫 하이라이트

### v3+ — 문서 추출 / 시맨틱
- PDF/DOCX/XLSX 내용 추출 (별도 파서 선택)
- (장기) 시맨틱 검색은 미정 — 리소스 대비 이득 검토 후 결정

## 3. 기술 설계

### 데이터 흐름
```
[인덱서 스레드] walkdir 병렬 walk
      ↓ 필터(제외규칙) → 정규화
   FTS5 파일 테이블 upsert
      ↓
[notify watcher] 생성/변경/삭제 → 해당 파일만 증분 반영
      ↓
[검색] commands/search
      ↓ invoke
React (입력 → 결과 테이블)
```

### Rust 모듈
- `core/indexer.rs` — 디렉터리 walk + 파일 메타 추출 (병렬 채널) → 이후 `crates/filesystem`으로 추출
- `core/ignore.rs` — 제외 규칙 (.git, node_modules, target 등; gitignore 스타일)
- `core/watch.rs` — `notify` 리시버 → 증분 인덱스 큐
- `core/search.rs` — FTS5 쿼리 빌더, 스니펫 추출(v2) → 이후 `crates/search`로 추출
- `db.rs` — SQLite + FTS5 스키마, 마이그레이션 → 이후 `crates/database`로 추출
- `commands/indexing.rs` — `start_index()`, `index_status()`, `add_root()`, `list_roots()`
- `commands/search.rs` — `search(query, filters, page)`

### DB 스키마
```sql
CREATE TABLE files (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  ext TEXT,
  size INTEGER,
  modified_ts INTEGER,
  root_id INTEGER
);
CREATE VIRTUAL TABLE files_fts USING fts5(name, content='files', content_rowid='id');
-- v2: 별도 내용 테이블 + fts5(content) 
CREATE INDEX idx_files_ext ON files(ext);
CREATE TABLE roots (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, enabled INTEGER);
```
- 실시간 인덱스: 파일 변경 시 `files_fts`에 `delete/insert` 반영
- 성능: walk는 `rayon` 병렬 + 스레드당 커넥션 풀(`r2d2_sqlite`)

### 데이터 모델
- `FileEntry { id, path, name, ext, size, modified_ts }`
- `IndexStatus { total_files, roots, last_scan_ts, indexing: bool, progress }`
- `SearchResult { file: FileEntry, snippet?: string }`

## 4. UI 설계
```
[Everything+]  [🔍  입력하세요...                ]  (엔터=검색)
루트: [C:\] [D:\] [E:\projects]   인덱스: 42,317 files  ● 최신
+----------------+--------+-------+-----------+
| NAME           | PATH   | SIZE  | MODIFIED  |
| PLAN.md        | port-manager | 4.2KB | 2026-08-10|
| tauri.conf.json| ...    | 1.1KB | ...       |
+----------------+--------+-------+-----------+
v2: 내용 탭 토글 [이름] [내용] → 스니펫 표시
```
- 결과 더블클릭 → `explorer /select,<path>` 열기
- 상단 루트 관리, 진행률 바 (인덱싱 중)

## 5. 구현 단계
1. 스캐폴드 + SQLite/FTS5 스키마 + 마이그레이션
2. 인덱서(단일 루트, 제외 규칙) → 색인 완료
3. FTS5 이름 검색 command → 기본 결과 UI
4. 필터(확장자/크기/정렬) + 페이지네이션
5. notify watcher 증분 반영 + 상태 UI
6. 병렬 인덱싱(rayon) + 진행률 + 다중 루트
7. v2: 내용 인덱싱(텍스트 확장자) + 스니펫
8. 성능 벤치(10만 파일 목표), Windows 빌드 검증

## 6. 테스트
- Rust: ignore 규칙, FTS5 쿼리/정렬, watcher 이벤트 반영 (임시 디렉터리 픽스처)
- 벤치: 파일 10만 개에서 검색 < 50ms 목표 검증
- 프론트: 검색 입력/필터/정렬 vitest

## 7. 확장/연계
- knowledge-base: 문서 목록·검색을 재사용 (`crates/filesystem`·`crates/search` 공유)
- developer-toolbox: 확장자별 아이콘 매핑 공용화
- 공통 추출 후보: `crates/filesystem`(walk·ignore·watcher), `crates/search`(FTS5), `crates/database`

## 8. 완료 정의(Done)
- 루트 1개+로 이름 검색 < 50ms, watcher로 변경 즉시 반영
- 10만 파일 인덱스 성능 목표 달성, v2 내용 검색 동작
- Windows 배포 빌드 성공
