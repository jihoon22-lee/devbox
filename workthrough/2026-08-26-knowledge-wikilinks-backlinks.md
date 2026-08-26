# Knowledge Wikilinks and Backlinks

## Overview

Issue #272의 P1-09 Knowledge 범위로 Markdown 원문에 쓰는 `[[target]]`과
`[[target|alias]]`를 편집기·preview·검색 보조 인덱스·backlink에서 일관되게 해석하도록 만들었다.
파일이 계속 source of truth이고 SQLite는 언제든 원문에서 복구할 수 있는 key/link metadata만
보관한다. 외부 도구나 network service 없이 앱 안에서 자동완성, 상태 표시, note 이동과 backlink
source 위치 이동까지 완료한다.

```text
Markdown source
    │
    ├─ bounded Rust parser ────────────────┐
    │                                      │
    ├─ save / watcher / one-time rebuild   ▼
    │                          doc_link_keys + wikilinks
    │                                      │
    │                     0 match ─ missing│
    │                     1 match ─ resolved ─► canonical relative .md path
    │                    2+ match ─ ambiguous
    │                                      │
    ├─ current editor analysis ◄───────────┤
    ├─ sanitized preview ◄─────────────────┤
    └─ backlinks with source line/column ◄─┘

resolved editor / preview / backlink click
    └─ indexed relative path ─► canonical root + .md + 10 MiB validation ─► open

raw [[target]] ─X─► filesystem path
```

Windows packaged autocomplete, decoration, preview와 backlink navigation smoke는 계획된 W1 P1 묶음
checkpoint에서 수행한다. 이 작업은 Linux/WSL에서 검증 가능한 parser/index/command 계약, React UI,
회귀 테스트와 production build를 완료한다.

## Scope

### Included

- `[[target]]`과 `[[target|alias]]`의 단일 Rust parser
- frontmatter, fenced code, inline code와 `\[[` escape 제외
- path stem, filename stem, frontmatter title 기반 note key
- missing/resolved/ambiguous/invalid 상태와 editor decoration
- `[[` 입력 시 indexed note title/path 자동완성
- resolved link의 Ctrl/Cmd+클릭과 preview 링크 이동
- 선택 노트의 backlink source path와 정확한 line/column 이동
- 앱 저장·rename 재색인·watcher에서 FTS와 link metadata의 원자적 갱신
- 기존 DB를 원문에서 한 번 복구하는 `wikilink-schema=1` rebuild marker
- 일반 Markdown 상대 링크 동작과 dirty state의 회귀 방지
- README, architecture, roadmap, opportunity와 상세 계획 문서 동기화

### Excluded

- 노트 rename 전 영향 diff와 inbound link rewrite transaction
- system-wide quick capture, Inbox, attachment와 image 관리
- daily/weekly template와 opt-in Git 기능
- fuzzy alias, heading fragment, transclusion, graph canvas
- 외부 wiki/knowledge 도구 연동, runtime download와 network service
- Windows packaged smoke의 개별 실행

## Parser Contract

`core/wikilink.rs`가 저장, watcher, 현재 편집 내용 분석과 preview에서 사용하는 유일한 parser다.
frontend가 링크 문법을 별도로 해석해 filesystem path를 만들지 않는다.

| Boundary | Contract |
|---|---|
| Current analysis / inbound open | 최대 10 MiB |
| Links per document | 최대 2,000개 |
| Target / alias | 각각 최대 512 UTF-8 bytes |
| Source line | 1-based |
| Source column | 1-based UTF-16 code units |
| Editor offsets | CodeMirror-normalized absolute UTF-16 offsets |
| Preview offsets | CRLF를 보존한 raw UTF-8 byte offsets |

링크는 한 줄을 넘지 않는다. YAML frontmatter, backtick inline code, backtick/tilde fenced code와
escape된 opener는 건너뛴다. target은 case-insensitive `.md` suffix를 제거하고 소문자 lookup key로
정규화한다. 빈 값, root/drive/URI 형태, 빈 path segment, `.`·`..`, backslash, NUL/CR/LF,
`:`·`#`·`[`·`]`·`|`는 invalid다. invalid target도 화면에서 상태를 설명할 수 있지만 resolution query나
파일 열기에 사용하지 않는다.

한글과 supplementary Unicode가 있는 line도 CodeMirror 위치와 맞도록 line/column/absolute editor
offset을 UTF-16 단위로 계산한다. CRLF 문서는 editor offset에서 newline 하나로, preview 치환에서는
원문의 두 byte로 유지해 두 소비자의 좌표계를 섞지 않는다.

## Rebuildable Link Index

기존 FTS SQLite에 다음 재생성 가능 table을 추가했다.

```sql
doc_link_keys(path, key)
wikilinks(source_path, ordinal, target, target_key, line, column)
```

각 Markdown note에는 root-relative path without `.md`, filename stem, frontmatter title을 정규화한
key가 deduplicate되어 들어간다. 검색 가능한 비 Markdown 문서는 key/link metadata를 만들지 않는다.
resolution은 저장 시 고정하지 않고 현재 `doc_link_keys`를 query한다.

- 일치 path 0개: `missing`
- 일치 path 1개: `resolved`
- 일치 path 2개 이상: `ambiguous`
- invalid target: query하지 않는 `invalid`

이 때문에 target note가 나중에 생성되거나 삭제되어도 source 파일을 다시 쓰지 않고 현재 상태와
backlink가 바뀐다. ambiguous link는 임의의 note를 선택하지 않으며 어느 note의 backlink에도 넣지
않는다. 자동완성은 title/path를 LIKE escape한 bounded query로 찾고 최대 100개만 반환한다.

`index_doc_in_transaction`이 FTS row, note key와 outgoing link를 같은 SQLite transaction 안에서
교체한다. 앱 save/create, rename 후 재색인과 watcher 외부 변경이 이 경로를 공유한다. 문서 삭제
trigger는 source의 key/link metadata도 함께 제거한다.

이전 DB의 FTS body에는 frontmatter가 없고 정확한 link 위치도 없으므로 단순 migration만으로는
backlink 위치를 복원할 수 없다. setup에서 marker가 없을 때 Knowledge root를 한 번 순회하며 기존
`read_note` 안전 경계를 통과한 `.md` 원문을 다시 인덱스한다. root 밖 symlink, 10 MiB 초과, 잘못된
확장자와 읽을 수 없는 항목은 건너뛴다. transaction이 전부 성공한 뒤에만 `wikilink-schema=1`을
기록하고, 실패하면 다음 실행에서 다시 시도한다. 기존 문서의 `modified_ts`는 rebuild 때문에
변경하지 않는다.

## Tauri Command Boundary

세 command가 frontend에 필요한 최소 DTO만 제공한다.

- `analyze_wikilinks(content)`: 현재 unsaved source를 같은 parser와 DB key 집합으로 분석한다.
- `wikilink_candidates(query)`: 최대 256 UTF-8 bytes인 query로 indexed note를 최대 100개 찾는다.
- `backlinks(rel)`: target note를 먼저 canonical validation한 뒤 uniquely resolved source만 반환한다.

DTO에는 상태, 표시 target/label, source line/column과 resolved일 때의 root-relative path만 있다.
절대 경로, note body/snippet, database 오류와 OS 오류는 반환하지 않는다. query의 NUL/CR/LF, 과대
입력과 잘못된 target은 고정된 안전 오류로 fail-closed한다.

editor, preview와 backlink 클릭은 raw target을 열지 않는다. backend resolution이 제공한 상대
path도 기존 `openInboundNote`를 거쳐 canonical Knowledge root 안의 실제 `.md`, 10 MiB 제한을 다시
검증한다. symlink가 root 밖으로 향하거나 열기 직전 파일이 바뀌면 중단한다.

## Editor, Preview and Backlink UX

`wikilinkEditor.ts`는 CodeMirror StateField/StateEffect로 backend 분석 결과를 decoration에 반영한다.
resolved link는 구분 색상과 backend가 유일하게 해석한 `data-wikilink-path`를 갖고,
missing/ambiguous/invalid는 path 없이 상태별 title과 물결 밑줄로 표시된다. Ctrl/Cmd+mousedown은
resolved decoration에만 반응한다.

autocomplete는 syntax tree가 code node가 아닌 위치에서 escape되지 않은 `[[` 뒤에만 활성화된다.
alias separator `|` 뒤에는 뜨지 않으며 query가 256 UTF-8 bytes를 넘으면 backend를 호출하지 않는다.
선택 시 root-relative canonical target without `.md`와 필요한 closing `]]`를 삽입한다. candidate는
DB에 실제로 인덱스된 note만 사용한다.

App은 현재 편집 내용을 220 ms debounce해 분석하고, 선택 노트가 바뀐 뒤 늦게 온 response는
버린다. 저장과 watcher metadata revision 뒤에는 link 상태와 backlink를 함께 새로 읽는다. header의
unresolved count와 `Backlinks (N)` panel로 상태를 표시하고, backlink row는 source relative path와
`line:column`만 보여준다. 클릭하면 source note를 안전하게 읽은 뒤 정확한 CodeMirror 위치를 선택,
가운데로 scroll하고 focus한다.

외부 value synchronization이 editor update listener를 다시 호출해 backlink navigation 직후 note를
dirty로 만들던 경로는 synchronization guard로 차단했다. tree open, inbound applink, daily note와
삭제 뒤에는 이전 cursor request를 초기화한다.

preview는 parser가 준 raw byte range를 뒤에서부터 safe anchor/span으로 치환한 후 기존
`crates/markdown` ammonia sanitization을 통과시킨다. alias/target/path는 HTML escape한다. `/`로
시작하고 `wikilink` class인 resolved anchor만 Knowledge note navigation으로 보내며, 기존 일반
Markdown 상대 링크는 원래 `openFile` 동작을 유지한다.

## Dependency and Resource Decision

새 직접 dependency는 공식 MIT `@codemirror/autocomplete` 하나다. 같은 resolved version 6.20.3이
Code Pad 때문에 이미 lockfile, pnpm store와 `THIRD_PARTY_NOTICES.md`에 존재해 새 package download,
license 또는 transitive dependency는 없다. 기능은 설치 후 offline으로 동작하고 sidecar, worker,
daemon, polling service나 외부 executable을 추가하지 않았다.

동일 Node/Vite toolchain의 #271 main 대비 production bundle은 다음과 같다.

| Asset | Main | Feature | Delta |
|---|---:|---:|---:|
| Largest JS exact | 1,374,540 B | 1,411,279 B | +36,739 B |
| Largest JS gzip | 404,457 B | 416,435 B | +11,978 B |
| CSS exact | 7,706 B | 9,361 B | +1,655 B |
| CSS gzip | 2,117 B | 2,466 B | +349 B |

DB metadata는 link당 target/key/position만 보관하고 문서당 2,000 link 상한이 있다. editor 분석은
220 ms debounce, 자동완성은 100 result, backlink는 2,000 result로 제한된다. 별도 background worker는
없다. 로컬 검증은 Cargo/Vitest 단일 worker와 Node 768 MiB heap cap으로 순차 실행해 다른 작업용
메모리를 남겼다.

## Verification

### Frontend

- full Knowledge Base Vitest: 7 files, 28 tests — passed, single worker
- `MarkdownEditor.test.tsx`: 7 tests — passed
- `MarkdownPreview.test.tsx`: wikilink/ordinary relative-link regression — passed after final routing split
- `pnpm --filter knowledge-base build` — passed
- TypeScript compile and Vite production build — passed
- production main/feature exact + gzip comparison — recorded

Frontend coverage는 resolved/unresolved decoration, indexed candidate request와 자동완성 insertion,
Ctrl/Cmd navigation, backlink panel과 line/column selection을 포함한다. preview는 safe resolved anchor,
unresolved span, alias HTML escape와 일반 Markdown relative link 회귀를 검증한다. parser의 syntax/escape
제외와 query/target 실패 경계는 Rust suite가 검증하며, 기존 App/applink test mock도 새 command를
명시해 unrelated flow를 보존했다.

### Rust

- `cargo test -p knowledge-base --jobs 1` — 42 passed
- `cargo check -p knowledge-base --jobs 1` — passed
- `cargo clippy -p knowledge-base --all-targets --jobs 1 -- -D warnings` — passed
- `cargo fmt --all --check` — passed

Rust coverage는 alias/multiple/Unicode, code/frontmatter/escape 제외, invalid/과대 target, CRLF 좌표,
Markdown-only note key, non-Markdown 후보 제외, dynamic resolution, exact backlink 위치, ambiguous exclusion,
one-time rebuild와 modified timestamp 보존, sanitized preview anchor를 포함한다.

### Repository Review

- `pnpm install --frozen-lockfile` — passed
- `pnpm audit --audit-level moderate` — passed
- dependency policy/notices tests — passed
- `git diff --check` — passed
- raw target의 filesystem/open/log 경로 부재 — reviewed
- 변경 전체와 issue acceptance를 PR 직전에 직접 재검토 — completed

전체 workspace Linux/Windows build, dependency/catalog/security gate는 PR GitHub Actions에서 최종
확인한다.

## Remaining Checkpoints and Next Scope

- W1: packaged Windows에서 자동완성, resolved/unresolved/ambiguous/invalid decoration과 IME smoke
- W1: editor Ctrl/Cmd+click, preview wikilink/일반 relative link, backlink exact cursor navigation evidence
- W1: 외부 편집·target 생성/삭제 뒤 dynamic resolution과 schema rebuild smoke
- link-aware rename impact preview와 rewrite transaction은 다음 독립 Knowledge issue에서 수행한다.
- quick capture, attachment와 template는 각 P2/P3 issue 경계를 유지한다.
- Knowledge 0.4.0 version bump는 Wave 9 release preparation에서 별도로 수행한다.
