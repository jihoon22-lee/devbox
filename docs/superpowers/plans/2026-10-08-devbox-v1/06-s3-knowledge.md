# S3 기록 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: **상세화 완료 · 실행 중(트랙 1)** · 브랜치 `v1/s3-knowledge`(origin/main `329b2ee9`에서 시작) · worktree `../devbox-wt/v1-s3-knowledge`. S5(트랙 2)와 동시 진행. 공유 파일 예외 후보(`crates/core/src/config.rs`)는 PROGRESS §2 기록 뒤 Task 4 앞에서 진행한다.
- 상세화 수준: Task 1에서 실제 S0b–S2 코드(main `329b2ee9`)와 v0.9.0 태그를 대조해 Interfaces·파일·실패 테스트·검증 명령·커밋을 확정했다. 각 코드 조각은 그 과제 구현의 시작점이며 그 아래 핵심 테스트 전부가 완료 조건이다. 계획 코드는 컴파일해 본 것이 아니다. 같은 의도로 고쳐도 되고, 고친 것은 PROGRESS §5에 적는다(00 §3.5).

**Goal:** 개발하면서 남기는 기록을 devbox 안에서 끝낸다.
- 노트 vault(Markdown·위키링크·백링크·템플릿·이미지)
- 일일 기록과 "오늘 한 일" 요약
- 빠른 캡처
- Windows 활동 기록(개인정보 규칙 fail-closed)
- 노트·코드·파일 이름 통합 검색
- 노트에서 에이전트 작업 만들기
- MCP 노트 도구

**Architecture:**
- `crates/documents`가 문서 읽기·저장·저널·이름 바꾸기·외부 변경 감시와 **쓰기 허용 범위(`WriteScope`)**를 소유한다.
  - 저장은 CAS(`baseRev`)와 원자적 교체로 한다. 같은 문서는 lease를 가진 창 하나만 편집한다.
- 화면의 `DocumentStore`는 01-design §8.4 상태 기계를 그대로 구현한 순수 리듀서 + 효과 실행기다. S4 코드 편집기도 같은 것을 쓴다.
- `crates/notes`는 vault 규칙(frontmatter·링크·템플릿·첨부·일일 기록·살균 렌더·링크 재작성)을 맡는다. 쓰기는 모두 `documents`를 거친다.
- `crates/search`는 `index.db`의 `search` 도메인 FTS5와 `grep-searcher`·`ignore`를 쓴다. 노트 본문은 CLI 조립 계층이 notes 색인 결과를 search에 넘겨 넣는다(두 crate는 서로 의존하지 않는다).
- `crates/activity`는 v0.9.0 `activity-engine` 순수 로직(`model`)을 이식하고 저장을 묶어 쓴다. `model`은 `default-features = false`로 Windows 앱(`crates/win`·`app/src-tauri`)이 그대로 재사용한다. Windows 쪽 수집기는 `crates/win`에 둔다.
- 데몬 조립은 `crates/cli/src/daemon/knowledge.rs`(서비스 생성·root 계산·색인 연결)와 `knowledge_routes.rs`(라우터 등록)에 모은다. 공유 등록 파일(`routes.rs`·`state.rs`·`mod.rs`)에는 줄 추가만 한다.

**Tech Stack:** S2 스택 + CodeMirror 6(`@codemirror/lang-markdown`, `@codemirror/autocomplete` 직접 의존 추가. 둘 다 이미 lock에 전이 의존으로 있음), `pulldown-cmark` 0.13·`ammonia` 4(v0.9.0 `crates/markdown`과 같은 판), `mermaid`(화면, `securityLevel: "strict"`), `rusqlite` 0.32 bundled FTS5(`SQLITE_ENABLE_FTS5` 확인), `grep-searcher`·`grep-regex`·`ignore`, `rustix` 1.1(`fs`, `renameat_with`·`RenameFlags`·`OFlags::NOFOLLOW`. 이미 lock에 있음), `sha2`(기존. 내용 해시), `data-encoding`(기존. 첨부 base64), `notify` 8.2(기존. inotify), `regex`(기존), `idb-keyval`(화면 임시 저널).
- 06 초안의 `nix`(`renameat2`)·`blake3`는 쓰지 않는다. 이유는 아래 "계획과 달라진 점" 표.

**Spec:** [01-design.md](01-design.md) §2.2·§6.1 활동·§6.2·§8.4·§9 `documents`·`notes`·`search`·`activity`·§9.1-2·3·5·6·8·16·§9.3 이식 표, §5.3(오류)·§5.5 R8·§8.6(정합성)·§12(쓰기 범위)

## Global Constraints

- S0b–S2의 Global Constraints를 따른다.
- 작업 위치: S3 전체가 브랜치 `v1/s3-knowledge`(origin/main에서) 하나와 worktree `../devbox-wt/v1-s3-knowledge` 하나다. Task 1(상세화)이 이 브랜치의 첫 커밋이다. PR은 Task 12에서 한 번 연다(00-roadmap §3).
- **S5와 동시에 진행한다**(사용자 결정 2026-10-08). 공유 파일·잠금 파일·생성 파일·main 반영·squash 머지 규칙은 00-roadmap §3.6과 아래 "공유 파일 경계"를 따른다. S3에서 S5 기능(API·변환 도구)을 쓰거나 고치지 않는다. 두 쪽을 잇는 기능(API 응답을 노트에 붙이기 등)은 S6-1b다.
- 문서 쓰기·삭제는 등록한 프로젝트·그 에이전트 작업 폴더(`<상위>/<이름>-agents`)·vault·devbox 데이터 폴더 안에서만 한다. `realpath`로 확인하고 `O_NOFOLLOW`로 연다(01-design §12). 범위 계산은 `documents::WriteScope`가 하고 root 목록은 CLI 조립 계층이 매 요청 주입한다(S2 `daemon/logs.rs::roots`와 같은 방식).
- 원자적 교체는 `renameat2(RENAME_EXCHANGE)`, 새 파일·이동은 `RENAME_NOREPLACE`를 쓴다. `unsafe_code = "deny"`이므로 `rustix::fs::renameat_with`로 부른다. 데몬 배포 바이너리는 musl(`scripts/build-linux.sh`)이라 gnu 전용인 `nix::fcntl::renameat2`는 쓸 수 없다. `ENOSYS`·`EINVAL`(지원하지 않는 파일 시스템)이면 덮어쓰기 대체 경로 없이 `documents.unsupported`로 거절한다. mode·EOL·BOM을 보존하고, symlink는 realpath 대상에 쓴다(01-design §9.1-2).
- 이진·비 UTF-8·크기 초과(노트 2 MiB, 코드 5 MiB) 파일은 읽기 전용으로 연다. 프레임 상한 16 MiB(`protocol::frame::MAX_JSON_BYTES`, 화면 `frame.ts` 같은 값)를 넘지 않도록 문서 본문을 담는 응답·요청은 **JSON 이스케이프 후 12 MiB**(`documents::MAX_WIRE_TEXT`)를 넘으면 읽기 전용(`too_large`)·저장 거절로 처리한다. 충돌 오류는 본문을 싣지 않고 rev만 싣는다(본문은 `documents.read`로 따로 받는다).
- 문서 내용·제목·경로·창 제목은 로그(tracing)에 남기지 않는다. 오류 세부에도 경로 원문을 넣지 않는다(키는 화면이 이미 알고 있다).
- 활동 개인정보 규칙은 fail-closed다. 규칙을 받지 못했거나 정규식 컴파일에 실패하면 제목을 보내지 않는다.
- vault 기본 위치는 `~/notes`다(Q4 확정).
- 두 PC가 같은 vault를 Git으로 공유한다. 일일 기록 요약은 PC 이름 소제목 아래에만 쓴다.
- RPC 오류 세부 필드 이름은 한 단어 소문자다(01 §5.3). 2^53을 넘을 수 있는 정수(mtime ns·inode)와 해시는 문자열로 보낸다. 텍스트 위치는 UTF-16 code unit이고 필드 이름에 `_utf16`을 붙인다(01 §9.1-15).

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 저장 중 외부 편집기가 같은 파일을 고침 | 덮어쓰지 않고 비교 화면. 버퍼·저널 보존 | Task 2·3 |
| 여러 노트 링크를 고치는 이름 바꾸기 도중 데몬 종료 | 다음 시작 때 진행 저널로 남은 파일을 이어서 고치거나 목록을 보여 줌 | Task 5 |
| 노트 A·B를 오가며 편집 후 Ctrl+Z | 각 노트 자기 undo 기록만 되돌림(AU-K1) | Task 3 |
| 활동 저장이 디스크 부족으로 실패 | 화면·트레이에 "저장 중단", 뒤 세션이 조용히 사라지지 않음(CV-F08) | Task 8·11 |
| 반복 관찰 뒤 idle로 끝남(AU-K2) | 마지막 입력 뒤 꼬리 구간을 활동으로 세지 않음(입력 1,000,000 → 끝 1,000,000) | Task 8 |
| 검색 결과를 보는 중 색인이 갱신됨 | 마지막 성공 결과와 선택을 유지, rev가 바뀐 경우에만 다시 조회(CV-F07) | Task 7·11 |
| 충돌 화면에서 [디스크 것]을 고른 뒤 앱을 다시 엶 | 그 문서가 복구 후보로 다시 나오지 않음(AR-F13) | Task 2·3 |
| dirty 노트를 rename → 저장 → 다시 엶 | 옛 경로 저널 0개(CV-F03) | Task 2·3 |
| 기본 템플릿(빈 title)으로 만든 노트를 파일 이름으로 검색 | 1건 찾음(AR-F10) | Task 4·7 |
| `.devbox-tmp-*`·`.trash/` 아래 노트 | 트리·검색·백링크·링크 재작성에 나오지 않음(CV-F06) | Task 2·4·5·7 |
| 늦게 끝난 활동 하루 조회 | "수집 중/일시중지" 표시를 덮지 않음(AR-F12) | Task 11 |
| 규칙을 못 받았거나 정규식이 틀림 | 제목 없이 앱 이름·시간만 저장 | Task 8·9 |

---

## 작업 묶음

S3 전체가 브랜치 `v1/s3-knowledge` 하나, PR 하나다(00-roadmap §3). 작업 위치는 worktree `../devbox-wt/v1-s3-knowledge`다.
묶음은 로컬 커밋·세션 인계 지점이다. 과제별 좁은 검증과 별도 검토 뒤 `PROGRESS.md` 묶음 행·현재 위치를 갱신한다. 묶음별 push와 전체 검사는 하지 않는다. 하위 프로젝트 끝에 브랜치 전체 검토 결과를 PROGRESS에 남기고 최종 상세 검증·Biome CI 뒤 push·PR을 한 번 진행한다(00-roadmap §3).
PR은 마지막 묶음이 끝난 뒤 한 번 열고, 그 CI가 S3 전체를 한 번 검사한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| 상세화 | Task 1 | 커밋(이 브랜치의 첫 커밋) |
| S | Task 2–3 | 로컬 커밋·인계 |
| T | Task 4–6 | 로컬 커밋·인계(Task 4 앞에 공유 파일 예외 커밋이 있으면 함께) |
| U | Task 7 | 로컬 커밋·인계 |
| V | Task 8–9 | 로컬 커밋·인계 |
| W | Task 10–11 | 로컬 커밋·인계 |
| 마무리 | Task 12 | **S3 PR 열기** → 필수 CI 3개·자동 리뷰 3회 상한 판정(CONVENTIONS §8) → squash 머지 |

---

## 실제 코드 계약과 검증 명령 (Task 1 확정)

**RPC 선언과 등록(실제 S0b–S2 코드)**
- `method!`·`topic!`·`domain_error!`는 `crates/protocol`이 **정의**하고, 메서드·주제·오류는 각 **도메인 crate의 `lib.rs`·`model.rs`에서 선언**한다(예: `crates/observe/src/lib.rs`, `crates/runtime/src/lib.rs`). 그래서 S3의 새 메서드는 동결된 `crates/protocol`을 고치지 않는다. 표기는 `method!(pub NotesTree = "notes.tree", query, NotesTreeParams => NotesTree, NotesError);`, 확인 정책은 `mutation(always)`·`mutation(reversible)`, 스트림은 `stream`이고 결과는 `devbox_protocol::StreamOpened`다.
- 오류는 `domain_error! { pub enum DocumentsError { Conflict { rev: DiskRev } = "documents.conflict", … } }`. 핸들러 반환은 `Result<T, devbox_protocol::Fail<E>>`, 하부 오류는 `.internal("what")?`(`devbox_protocol::Internal`).
- 라우터는 `devbox_protocol::Router<Services>::register::<M, _, _>(|s, ctx, p| async move { … })`다(01 §5.2 예시의 `add`가 아님). 스트림은 `ctx.open_stream()? -> StreamEnds { id, tx, input }`, `tx.write(&bytes).await`·`tx.finish().await`·`tx.closed()`(취소 토큰). 응답 `StreamOpened { stream_id }`를 먼저 돌려주고 쓰기는 백그라운드에서 한다(S2 `logs.open`과 같음).
- 등록: 새 파일 `crates/cli/src/daemon/knowledge_routes.rs`의 `pub(super) fn register(router: Router<Services>) -> Router<Services>`(S2 `secret_routes::register` 방식)를 `routes.rs`의 `let router = super::secret_routes::register(router);` 다음 한 줄로 부른다. `routes::declared()`에 `devbox_documents::export(&mut n); devbox_notes::export(&mut n); devbox_search::export(&mut n); devbox_activity::export(&mut n);` 4줄을 더한다. `every_declared_method_is_routed`가 빠진 등록을 잡는다.
- 생성기: `xtask/src/main.rs`에 같은 `export` 4줄, `xtask/Cargo.toml`에 4개 crate 의존을 더한 뒤 `pnpm gen`(= `cargo run -q -p xtask -- gen-ts`)으로 `app/src/rpc/gen/rpc.ts`를 다시 만든다. 손으로 합치지 않는다. 확인은 `cargo run -q -p xtask -- gen-ts --check`.
- 화면 문구: 새 오류 코드는 `app/src/rpc/messages.ts`의 `TABLE`에 줄 추가만 한다(빠지면 컴파일 오류).
- 앱 전용 메서드: `activity.ingest`는 `crates/cli/src/daemon/server.rs` `handle` 안의 `let allowed = if native { … } else if method == "secrets.export_key" { … } …` 체인에 `else if method == "activity.ingest" { ch != 0 && state.client == Some(ClientKind::App) }` 한 분기를 더해 렌더러(App, 채널≠0)만 부르게 한다. MCP·CLI·dev-gateway(`Dev`)는 `method_not_found`. 채널 0은 `NativeSecrets`·Windows 실행이 쓰므로 활동은 채널 0에 싣지 않는다.

**데몬 조립(`crates/cli/src/daemon`)**
- `Services`(`state.rs`)에 다섯 필드를 줄 추가한다: `pub index: devbox_core::IndexDb`, `pub documents: Arc<devbox_documents::DocumentsService>`, `pub notes: Arc<devbox_notes::NotesService>`, `pub search: Arc<devbox_search::SearchService>`, `pub activity: Arc<devbox_activity::ActivityService>`. **`Services.index`는 S5의 `api` 도메인도 같은 핸들을 쓴다**(두 번째 `IndexDb::open`을 만들지 않는다. 머지 때 같은 줄이 겹치면 하나만 남긴다).
- `mod.rs::build_services_with_runtime`에서 `IndexDb::open(&paths.index_db_path)?`, `migrations()`에 `devbox_activity::MIGRATIONS`·`devbox_search::MIGRATIONS`(저장된 검색) 두 줄, 서비스 생성은 `knowledge::build(&paths, &config, &db, &index, &hub, &projects, &agents)` 한 호출로 한다.
- 실제 경로: `Paths { data_dir, index_db_path, db_path, config_dir, home, … }`(`crates/core/src/paths.rs`). 저널 `<data_dir>/journals/`, 임시 파일 장부 `<data_dir>/documents/tmp/`, 링크 재작성 저널 `<data_dir>/notes-rename/`, 활동 내보내기 `<data_dir>/exports/`.
- `IndexDb::ensure_domain(&self, domain: &str, version: u32, create_sql: &str) -> Result<bool, CoreError>`: 도메인 이름은 ASCII 소문자로 시작하고 소문자·숫자만, 밑줄 불가, `index`·`sqlite` 예약. 테이블은 `<domain>_` 접두사, 다른 도메인 테이블·교차 FK·transaction 제어문 금지. 동기 함수이므로 서비스 시작 때 한 번 부른다. `IndexDb::write(|tx| …)`/`read(|tx| …)`는 async. IndexDb에는 rev가 없으므로 notes·search는 **데몬 메모리 카운터**를 rev로 쓴다(01 §5.5 R4). 
- `Db`(devbox.db): `Db::write(|tx| …) -> (T, rev)`, 이벤트는 `hub.publish::<Topic>(rev, &payload)`로 커밋 뒤에 낸다. 활동·저장된 검색은 devbox.db 마이그레이션 도메인 `activity`·`search`(`Migration { domain, version, sql }`)다. index.db의 `search` 도메인과 이름이 같아도 DB가 달라 충돌하지 않는다.
- 알림: `devbox_core::notify::Notification`은 `project_id: String`이 필수이고 `NotificationReason` enum이 동결된 `crates/core`에 있다. 그래서 S3 알림은 `notify.raised`를 쓰지 않고 **도메인 주제**(`notes.rename_incomplete`, `activity.status_changed`)로 내고, 화면 `features/notifications`가 알림 센터 항목으로 바꾼다.
- 입력 상한: 빠른 캡처·노트 덧붙이기·MCP 텍스트는 `devbox_core::input::validate_command_text`(UTF-8 64 KiB·NUL 거부, 부수 효과 전)를 재사용하고 도메인 오류 `notes.invalid_input`으로 바꾼다. 목록 응답은 256행·768 KiB 원칙(S1)을 따르고 넘치면 `truncated`를 붙인다.
- PC 이름: `config.pc.name`, 없으면 `daemon/hostname.rs::local_hostname()`.
- S2 `daemon/logs.rs::roots`는 `~/notes`를 직접 넣는다(주석 "configurable notes.vault belongs to S3 Task 4"). Task 4에서 설정된 vault로 바꾼다.

**설정(`crates/core/src/config.rs`) — 공유 파일 예외 후보**
- `Config`는 `#[serde(default, deny_unknown_fields)]`이고 `read_table`이 `config.toml`·`config.local.toml`을 각각 `Config`로 검증한다. 그래서 `[notes]`·`[search]`·`[activity]` 표가 하나라도 있으면 데몬과 MCP의 `Config::load`가 실패한다. `[secrets]`처럼 core에 필드를 선언하고 도메인 crate가 `toml_edit`로 그 표를 고치는 것이 기존 방식이다.
- 개인정보 규칙의 원장은 `config.toml`이다(01 §7 표). 다른 파일로 옮기면 설계 변경이다. 따라서 Task 4·7·8은 아래 "공유 파일 예외 후보 1" 기록 뒤 진행한다. Task 2·3은 설정이 필요 없다.

**화면(`app/src`)**
- 서버 데이터는 `rpc/query.ts`의 `useRpcQuery(method, params, opts)`·`useRpcMutation(method)`(`mutate`·`mutateAsync`, 같은 params의 응답 불명확 재시도만 같은 `clientRequestId`)·`useTopicInvalidation(topic, keys)`·`confirmPolicy(method)`만 쓴다. `query.ts`·`client.ts`는 동결이므로 메서드별 도우미를 넣지 않는다. 문서 효과·스트림은 기능 코드에서 `useRpcSession().client.call(...)`·`client.openStream("search.query", params, handlers)`·`client.subscribe(topic, (rev, payload) => …, onResync)`를 직접 쓴다. 조회 결과에는 `rev`를 담아 `shareByRevision`이 적용되게 한다.
- 섹션·라우터: `shell/sections.tsx`에 `/notes`(digit 5)가 이미 있다. 페이지·분리 창·하단 탭 등록은 `shell/Shell.tsx`의 `pages`·`popouts`·`registerBottomTab` 한 줄씩이다(S5도 같은 파일에 `/tools`를 더한다). 통합 검색은 `shell/router.tsx`에 `/search` 경로를 줄 추가한다(`paths` 집합 + 경로 1개). 명령·단축키는 `features/notes/NotesCommands.tsx`·`features/search/SearchCommands.tsx`에 두고 `Shell.tsx`에는 컴포넌트 한 줄만 둔다.
- 레이아웃: `shell/SectionLayout.tsx`의 `SectionLayout({ list, children })`, 하단 탭 `registerBottomTab({ id, label, render })`·`useBottomTabs`.
- 분리 창: `features/terminal/popout.ts`의 `openPopout(kind, id?)`(창 이름 `popout-<kind>-<hex>`, Tauri capability `popout-*`와 맞음). `PopoutKind`에 `"note" | "capture"`를 더한다.
- 단축키: `shell/keymap.ts`의 `useKeymap(bindings)`는 window capture 단계에서 먼저 잡는다. 터미널·편집기 포커스(`data-keyscope`)에서는 Ctrl+Shift/Alt+문자·Ctrl+숫자·F키만 통과한다. 그래서 Ctrl+Shift+F는 터미널 포커스에서도 앱 keymap에 먼저 잡히므로, `keymap.ts`가 포커스 판정을 내보내지 않는 점을 감안해 `features/search/SearchCommands.tsx`에 같은 판정의 지역 함수 `terminalFocused()`(`document.activeElement?.closest("[data-keyscope]")?.getAttribute("data-keyscope") === "terminal"`)를 두고 `enabled: () => !terminalFocused()`로 터미널 검색에 양보한다. 그리고 Ctrl+S는 앱 keymap이 아니라 CodeMirror `Prec.highest` keymap으로 둔다.
- 코드 보기: `ui/code/{CodeView,theme,languages,phrases}`. `EditorHost`는 `CodeView`를 고치지 않고 `codeTheme`·`codePhrases`·`loadLanguage`를 import해 자기 `EditorView`를 만든다(문서별 `EditorState` 전환이 필요하기 때문).
- 기존 컴포넌트: `ui/index.ts`가 내보내는 `Badge·Banner·Button·ConfirmDialog·Dialog·DiffView·EmptyState·ErrorState·IconButton·Kbd·Menu·Select·SplitPane·StatusDot·Tabs·TextArea·ToastHost·useToasts·Tooltip`. `VirtualList`·`KeyValueEditor`·`DataTable`은 index에 없으므로 S2처럼 파일 경로로 import한다(`import { VirtualList } from "../../ui/VirtualList"`, 예: `features/runs/LogView.tsx`). **`Tree`·`Timeline`은 아직 없다.** `app/src/ui`가 동결이므로 S3는 `features/notes/NoteTree.tsx`·`features/activity/ActivityBars.tsx`로 기능 폴더에 두고, 공용화는 S4-9(공용 컴포넌트 합치기)에서 판단한다.
- 에이전트 작업 만들기: `features/agents/ui.ts`의 `useAgentsUi`(`creating`, `setCreating`)와 `NewTaskDialog`(240줄). 지시문을 채워 여는 `draft` 필드를 더한다.
- 알림 센터: `features/notifications/store.ts`(`NoticeTarget.to`에 `/notes` 있음)와 `AttentionBridge.tsx`(주제 구독).

**Windows(`app/src-tauri`, `crates/win`)**
- 실제 파일: `app/src-tauri/src/lib.rs`(플러그인·명령 등록), `src/platform/tray.rs`(트레이 메뉴), `src/platform/shortcuts.rs`·`src/core/shortcuts.rs`(전역 단축키 등록·`ShortcutStatus`), `src/core/intents.rs`(`AppIntent`), `src/core/settings.rs`(`Settings`, `%LOCALAPPDATA%` 앱 설정), `src/platform/window.rs`(`configure_main`·`focus_main`·브라우저 가속키 끄기). 06 초안의 `src-tauri/src/{tray,shortcuts}.rs` 경로는 없다.
- `app/src-tauri`는 workspace 밖(`exclude`)이고 자체 `Cargo.lock`이 있다. `devbox-win`·`devbox-protocol`·`devbox-mux`만 의존한다. `crates/core`는 `std::os::unix`를 조건 없이 써서 Windows에서 빌드되지 않으므로 앱이 의존하면 안 된다. 그래서 `devbox-activity`는 `daemon` 기능(기본 켬) 아래에만 core·rusqlite·tokio를 두고, 앱·`crates/win`은 `default-features = false`로 `model`만 쓴다.
- `crates/win`은 `[lints]`를 workspace에서 받지 않아 `unsafe`를 쓴다(기존 dpapi·toast). Linux에서도 빌드되는 workspace member다. Windows 전용 코드는 `#[cfg(windows)]`.
- 기존 MCP: `crates/cli/src/mcp.rs` + `mcp/{tools,runs,protocol,input}.rs`. `tools::list(write)`·`Server::call_tool`, 쓰기 도구는 `[mcp] write = true`일 때만. S3는 `mcp/notes.rs`를 더하고 `tools.rs`에 줄 추가만 한다.
- 신뢰 승인(`crates/projects/src/trust.rs`)은 저장소 명령 실행에만 걸린다. S3는 저장소 명령을 실행하지 않는다. 노트에서 만드는 에이전트 작업은 기존 `agents.create`의 S1 신뢰 흐름을 그대로 탄다.
- 기존 감시 코드: `notify` 8.2(observe 로그 tail, runtime toml 감시). documents 감시도 같은 crate를 쓴다.

**검증 명령(트랙 1)**
- 좁은 검사는 공통 잠금 없이 자원 제한만 건다(00 §3.6): `python3 scripts/verify-resources.py --no-lock -- <명령>`. supervisor가 `CARGO_BUILD_JOBS=2`·`RUST_TEST_THREADS=2`·nice +10·CPU 4·지원 cgroup 6/8 GiB·swap 1 GiB를 건다. 아래에서 이 접두사를 `$NARROW`로 적는다(`source ~/.cargo/env` 뒤). 트랙 1은 기본 target-dir(`~/.cache/targets/devbox`)이다.
  - Rust: `$NARROW cargo test -p devbox-documents <filter> -- --test-threads=2`
  - 데몬 통합: `$NARROW cargo test -p devbox-cli --test <파일> <filter> -- --test-threads=2`(`crates/cli/tests/support::TestDaemon::{fixture, start, launch, client, stop, restart}`. 기본 fixture는 `DEVBOX_NO_SCOPE=1`)
  - 화면: `$NARROW pnpm --filter app exec vitest run src/<경로>.test.tsx --maxWorkers=2`, 타입 `$NARROW pnpm --filter app typecheck`
  - 생성물: `$NARROW pnpm gen` 뒤 `$NARROW cargo run -q -p xtask -- gen-ts --check`
  - Windows 앱 순수 로직: `$NARROW cargo test --manifest-path app/src-tauri/Cargo.toml <filter> -- --test-threads=2`(Linux에서는 `core` 모듈만 빌드됨). Windows cfg 컴파일 확인(아래 "MSVC 좁은 compile")은 S2 Task 15c가 쓴 인자와 환경을 따르되 공통 잠금(`--wait`) 대신 무잠금으로 돌린다: `DEVBOX_APP_CHANNEL=accept TAURI_CONFIG="$(cat app/src-tauri/tauri.accept.conf.json)" $NARROW cargo check --manifest-path app/src-tauri/Cargo.toml --lib --target x86_64-pc-windows-msvc --locked`(S2 근거는 `.git/devbox-archive/local-verification-20261021.tar.gz` 안 `devbox-verification/s2-task15c/parent-commands.json`의 `msvc-accept-fixed`: 같은 argv, 환경은 `DEVBOX_APP_CHANNEL=accept`·`TAURI_CONFIG`=accept 설정 내용뿐이며 xwin 등 추가 환경 없음. target은 설치돼 있음). 링크·실행을 하지 않으므로 Windows 실행 근거가 아니다.
- 잠금 경합 exit 75·환경 준비 실패는 RED가 아니다. 새 crate는 먼저 workspace member·Cargo.toml·빈 lib.rs·시험 파일 뼈대를 연결한다. `package not found`·테스트 0개는 RED가 아니며 목표 테스트의 미구현 심벌 또는 assertion 실패를 RED로 남긴다.
- 전체 검사(`bash scripts/check.sh`)·E2E(`bash scripts/e2e.sh`)는 Task 12에서 한 번이다. 두 스크립트는 항상 `--wait`(최대 60분)로 공통 잠금을 잡으므로 다시 감싸지 않는다. 기한 초과 exit 75는 "검사 대기 초과"로 보고하고 재시도한다.
- 모든 커밋 단계: 직접 영향 검사 → **별도 하위 에이전트 검토(00 §3.4)** → `pnpm exec biome ci .` → 지정 파일만 stage → `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit -m '<메시지>'`. 중간 push·PR 없음. 결과·RED/GREEN·미실행은 PROGRESS §5에 한 줄 요약 + `devbox-verification/s3-task<N>/` 근거로 남긴다.

### 공유 파일 경계 (S5 동시 진행, 00 §3.6)

| 경로 | S3가 하는 일 | 규칙 |
|---|---|---|
| `crates/protocol`, `app/src/ui`, `app/src/rpc`(gen 밖. 단 `messages.ts`는 00 §3.6 표가 줄 추가 등록 파일로 따로 명시), `.github`, `scripts` | 고치지 않는다. 메서드는 도메인 crate에 선언, 컴포넌트는 기능 폴더에, 스트림은 기존 `client.openStream` | 동결 |
| `crates/core` | `config.rs`만 예외 후보 1(아래). `paths.rs`(06 초안의 `WriteScope`)는 고치지 않는다 | 동결 · 예외는 PROGRESS §2 |
| `crates/cli/src/daemon/{routes,state,mod,server}.rs`, `xtask/src/main.rs`·`xtask/Cargo.toml`, `app/src/shell/{Shell,router}.tsx`, `app/src/rpc/messages.ts`, workspace `Cargo.toml`, `app/package.json`, `README.md`(Task 8 잠금 파일 안내 한 줄) | 줄 추가만 | 충돌하면 양쪽 줄을 모두 남긴다 |
| `app/src/rpc/gen/rpc.ts`, `Cargo.lock`, `pnpm-lock.yaml`, `app/src-tauri/Cargo.lock` | 생성·설치 명령으로만 만든다(`pnpm gen`, `cargo metadata --format-version 1 >/dev/null`, `pnpm install`, `cargo metadata --manifest-path app/src-tauri/Cargo.toml --format-version 1 >/dev/null`) | 손으로 합치지 않는다 |
| `PROGRESS.md` | S3 행·S3 묶음·"트랙 1 위치" 줄만 | 충돌하면 양쪽 보존 |
| 계획 문서 | 06과 07의 S4 절만 | — |

### 공유 파일 예외 후보 (PROGRESS §2 기록 후 진행)

1. **`crates/core/src/config.rs`(500줄) — `[notes]`·`[search]`·`[activity]` 표 허용.** 불가피하다(위 "설정" 근거).
   - 최소 변경: `Config`에 `pub notes: toml::Table, pub search: toml::Table, pub activity: toml::Table` 세 필드(`#[serde(default)]`)와 `read_table`의 "must be a table" 목록에 세 이름을 더한다(제품 약 10줄 + 시험 약 15줄, 합 약 25줄. 시험 `knowledge_sections_are_tables_and_left_to_their_domains`). 각 표의 키 검증(`deny_unknown_fields`)은 해당 도메인 crate가 `NotesSettings::from_table(&config.notes)` 등으로 한다. core가 activity 규칙 타입을 몰라도 된다.
   - 대안(설계 변경이라 채택하지 않음): 규칙을 별도 파일로 옮기면 01 §7 표와 어긋난다.
   - 시점: Task 4 Step 0의 별도 커밋 `feat(core): accept notes search and activity settings sections`. S5가 같은 파일을 고치지 않는다면 S3가 맡고, 둘 다 고치면 먼저 머지할 쪽이 맡는다(00 §3.6).
2. (예외 아님, 확인용) `crates/cli/src/daemon/server.rs`(474줄): `activity.ingest` 허용 분기 1줄(Task 8)과 claim·take의 dedupe 우회 분기 약 15줄(Task 2). cli는 동결 대상이 아니며 공유 파일 표의 "줄 추가만" 행이다(기존 분기 수정 없음).

### 사용자 결정 필요

- **D1–D16·Q1–Q4를 바꾸는 항목: 없음.** 아래 "계획과 달라진 점"은 모두 01 범위 안의 구현 선택이다. D8(v0.9.0 데이터 가져오지 않음)은 v0.9.0 Knowledge 폴더의 존재만 읽기 전용으로 확인하고 데이터를 읽지 않으므로 지킨다. D15(vault `~/notes`)는 기본값 그대로다. D9(Windows는 화면·트레이·단축키·알림·활동 수집)도 그대로다.
- **공유 파일 예외(PROGRESS §2 기록 후 진행):** 위 예외 후보 1(`crates/core/src/config.rs`). 00 §3.6은 동결 경로를 "꼭 필요하면 PROGRESS §2에 적고, 먼저 머지할 쪽이 맡는다"로 정하므로 별도 승인 대기 없이 기록 뒤 진행한다. Task 4·7·8의 선행 조건이고 최소안은 opaque 표 3개다. 사용자가 막으면 Task 4 전에 멈춘다.
- **기본값으로 진행하고 PROGRESS §5에 기록할 해석(이의가 있으면 그 Task 전에 바꾼다):**

| 항목 | 01 문구 | 진행할 기본값 | 영향 Task |
|---|---|---|---|
| 통합 검색 위치 | §8.2 "검색(본문 탭)" | 섹션이 아닌 `/search` 경로(왼쪽 섹션 목록에 없음, Ctrl+Shift+F로만 열림) | 11 |
| 링크 해석 동률 | 06 초안 "가까운 폴더 우선" | 정확한 경로 → 유일 key → 같은 폴더 → 공통 조상이 가장 깊은 것 → 그래도 동률이면 모호 | 4·5 |
| v0.9.0 활동 공존 안내 | §6.2 "켜져 있으면 안내" | `%LOCALAPPDATA%\com.devbox.v08.knowledge.i*\active-stores.json` 중 하나라도 있으면 안내(켜짐 여부는 SQLite라 읽지 않음, 꺼져 있어도 안내가 보일 수 있음) | 9 |
| S3 알림 경로 | §9 "알림 센터" | `notify.raised` 대신 도메인 주제 → 화면 알림 센터 | 5·8·11 |
| 수용 6번 | 빠른 캡처 단축키 | accept 설정에 겹치지 않는 소유 키를 지정해 단축키·SC2를 실제 판정, 등록 실패 때만 트레이 경로 + 단축키 미실행 | 12 |

### 600줄·300줄 근접 파일과 분할

| 파일(실측) | S3 변경 | 결정 |
|---|---|---|
| `crates/core/src/config.rs` 500 | 예외 후보 1, +약 25(제품 10·시험 15) | 분할 불필요 |
| `crates/cli/src/daemon/server.rs` 474 | +약 18(lease 분기 15·ingest 3) | 분할 불필요 |
| `crates/cli/src/daemon/routes.rs` 270 | +5(등록은 `knowledge_routes.rs`) | 분할 불필요 |
| `crates/cli/src/daemon/streams.rs` 526 | 변경 없음 | — |
| `app/src-tauri/src/platform/windows.rs` 553 | 변경 없음(Windows 실행 어댑터이고 창과 무관) | — |
| `app/src-tauri/src/core/shortcuts.rs` 253 | `ShortcutStatus.capture`·`default_capture` +약 20 | 분할 불필요 |
| `app/src-tauri/src/platform/shortcuts.rs` 198 | 캡처 등록 +약 30 | 분할 불필요 |
| `app/src/features/agents/NewTaskDialog.tsx` 240(`NewTaskForm` 약 210) | 초기 지시문 +5 | 300줄 아래 유지. 넘으면 폼 필드 하위 컴포넌트로 먼저 이동(별도 커밋) |
| `app/src/shell/Shell.tsx` 204 | +6(등록 줄) | 분할 불필요 |
| 새 파일 | 아래 각 Task의 Files가 600/300 아래가 되게 미리 나눴다 | 넘으면 같은 과제 안에서 나눈다 |

### v0.9.0 이식 대상 (태그 `v0.9.0`, `git show v0.9.0:<경로>`로 확인)

| 원본 | 원본 테스트 | 대상(Task) | 방식 | 이식 시험(이름 그대로) |
|---|---|---|---|---|
| `crates/activity-engine/src/core/sessionizer.rs`(199줄) | 6 | `activity::model::sessionizer`(Task 8) | 그대로 | `idle_retroactively_excludes_all_last_input_tail_intervals`, `explicit_pause_keeps_actual_pause_time_and_wall_clock_reversal_never_makes_negative_spans`, `same_app_title_extends_session`, `app_change_closes_previous`, `title_change_within_same_app_also_closes`, `finish_with_no_session_returns_none` |
| `crates/activity-engine/src/core/idle.rs`(90줄) | 4 | `activity::model::idle`(Task 8). Task 9는 `duration_from_input_ticks`를 부르기만 한다 | 그대로(`duration_from_input_ticks`의 `#[cfg(any(windows, test))]`는 `crates/win`이 쓰므로 조건 없이 공개) | `idle_boundary`, `session_ends_at_idle_start`, `parse_threshold_clamps`, `input_ticks_share_the_low_32_bit_domain_across_uptime_wrap` |
| `crates/activity-engine/src/core/privacy.rs`(356줄) | 9 | `activity::model::privacy`(Task 8, 앱도 재사용) | 그대로(`PrivacyRules`·`CompiledRules::{compile, fail_closed, apply}`·`InvalidRule{field,index,problem}`) | `empty_rules_pass_titles_through`, `quantifier_with_comma_is_a_single_working_pattern`, `spaces_and_hangul_are_matched_verbatim`, `excluded_process_drops_session_case_insensitively`, `mask_all_titles_keeps_session_without_title`, `syntax_error_is_reported_with_field_and_index`, `empty_too_long_and_too_many_are_rejected`, `fail_closed_never_keeps_a_title`, `stored_rules_parse_missing_as_default_and_garbage_as_error` |
| `crates/activity-engine/src/core/attribution.rs`(170줄) | 6 | `activity::model::attribution`(Task 8) | 그대로 | `matches_basename_in_title`, `longest_basename_wins`, `unmatched_long_alias_does_not_override_a_more_specific_match`, `case_insensitive`, `no_match_is_unattributed`, `aggregates_without_double_count` |
| `crates/activity-engine/src/core/models.rs` | — | `activity::model::ClosedSession` | 필요한 타입만(`ClosedSession{app,title,start_ts,end_ts}`) | — |
| `crates/wsl-helper/src/document.rs`(282줄) | 3 | `documents::sys`(Task 2) | 적응: `unsafe libc::syscall(SYS_renameat2)`·`fstatfs` → `rustix`; `.devbox-save-*/submitted.md·previous.md` → 같은 폴더 `.devbox-tmp-<난수>`, 교체 뒤 옛 내용 삭제 | `private_modes_exchange_and_no_clobber_preserve_both_writers`(mode 600·640·644 보존, 대상이 있으면 NOREPLACE 거절, EXCHANGE 뒤 새 내용), `unrelated_paths_and_symlinks_are_rejected`(→ `WriteScope` 거절로 적응). **제외 1:** `wsl1_fallback_never_overwrites_a_creator_in_the_missing_path_window` — v1은 WSL2+systemd 전용이고 비원자적 대체 경로를 두지 않는다(01 §9.1-2) |
| `crates/knowledge-vault-engine/src/platform/document_publish.rs`(523줄) | 3 | `documents::sys::move_no_replace`(Task 2) | 적응(rustix) | `moves_files_and_directories_but_preserves_existing_destinations`. **제외 2:** `permission_finalization_follows_our_handle_not_a_replaced_path`, `windows_replacement_preserves_target_dacl_and_recovery_is_private` — Windows DACL 경로이며 v1 문서 쓰기는 WSL 데몬만 한다 |
| `crates/markdown/src/lib.rs`(377줄) | 12 | `notes::render`(Task 4) | 그대로(`pulldown-cmark` 옵션 TABLES·STRIKETHROUGH·TASKLISTS·FOOTNOTES, `ammonia` `add_generic_attributes(["class","data-idx","data-reason"])`·`url_schemes(http,https,data)`·`attribute_filter(filter_url_attribute)`·`UrlRelative::PassThrough`, `ImageResult`, mermaid 자리표시 `<div class="mermaid-block" data-idx="N">` + 원문 목록). **바꾸는 것 하나:** 원격 `http(s)` 이미지는 `Passthrough` 대신 `<span class="md-image-missing" data-reason="remote-blocked">`로 바꾸고, 원시 HTML `<img src="http…">`도 `filter_url_attribute`에서 지운다(01 §6.4, CSP `img-src 'self' data:`) | 12개 모두 기대값 그대로: `mermaid_block_becomes_placeholder`, `multiple_mermaid_blocks_are_indexed_in_order`, `script_tags_are_sanitized`, `relative_link_href_is_preserved`, `javascript_href_is_removed`, `failed_image_load_shows_fallback`, `successful_image_load_inlines_data_uri`, `unsafe_link_schemes_are_removed`, `network_path_and_obfuscated_data_links_are_removed`, `http_and_relative_links_are_preserved`(링크 href는 유지), `data_uri_is_limited_to_raster_image_sources`, `non_mermaid_code_block_is_untouched`. 새 시험 `remote_images_are_blocked_with_a_notice` |
| `crates/knowledge-vault-engine/src/core/frontmatter.rs`(148줄) | 7 | `notes::frontmatter`(Task 4) | 이식 후 정리(YAML 라이브러리 없는 `title`·`tags` 파서) + AR-F10 | `parses_title_and_tags`, `parses_inline_tag_list`, `parses_json_quoted_capture_scalars`, `parses_dash_tags`, `parses_crlf_frontmatter_without_rewriting_the_body`, `closing_frontmatter_fence_must_be_an_entire_line`, `no_frontmatter_returns_default` |
| `crates/knowledge-vault-engine/src/core/wikilink.rs`(311줄) | 5 | `notes::links`(Task 4) | 이식 후 정리(`parse_wikilinks`·`normalize_link_key`·`note_link_keys`, UTF-16 위치). `#제목줄`은 key 정규화 전에 분리 | `parses_aliases_multiple_links_and_utf16_positions`, `ignores_fenced_inline_and_escaped_syntax`, `invalid_targets_are_detected_without_becoming_paths`, `note_keys_include_path_stem_filename_and_title_without_duplicates`, `utf16_offsets_follow_codemirror_newline_normalization_but_bytes_keep_crlf` |
| `crates/knowledge-vault-engine/src/core/templates.rs`(354줄) | 5 | `notes::templates`(Task 4) | 이식 후 정리: 변수 `{{title}}·{{date}}·{{time}}·{{pc}}`(+기존 `{{vault-relative-path}}` 유지), 한 번 치환·치환값 재해석 금지 | `substitutes_all_supported_values`(`{{pc}}` 추가), `substituted_values_are_not_interpreted_as_template_syntax`, `rejects_unknown_and_malformed_placeholders`, `rejects_unsafe_target_and_oversized_output`, `rejects_calendar_invalid_dates_and_windows_paths` |
| `crates/knowledge-vault-engine/src/core/assets.rs`(402줄) | 5 | `notes::attachments`(Task 4) | 이식 후 정리: 2 MiB·이미지 형식 판별 유지, 이름은 `attachments/<sha256 앞 12자>-<정리한 이름>.<확장자>` | `recognizes_supported_formats_and_dimensions`, `rejects_empty_oversized_unsupported_and_malformed_inputs`, `rejects_jpeg_without_header_dimensions_and_webp_bad_dimensions`, `creates_deterministic_hash_name_and_note_relative_links`(이름 규칙 기대값만 바꿈), `rejects_unsafe_note_and_asset_paths_without_echoing_them` |
| `crates/knowledge-vault-engine/src/core/rename.rs`(1053줄) | 11 | `notes::rename`(Task 5) | **참고만**(01 §9.3). preview/plan store·rollback 모델은 옮기지 않는다. 규칙만 가져온다: 깨질 링크만 고침, 별칭 보존, 새 key가 모호하면 경로형 링크로 씀, 대상 존재·외부 수정은 덮어쓰지 않음 | 이름 그대로 옮기지 않음. 대응 시험: Task 5 `rename_rewrites_wiki_and_markdown_links_in_all_backlinking_notes`(별칭 보존 포함), `conflicting_file_is_reported_not_overwritten` |
| `crates/knowledge-vault-engine/src/core/vault.rs`(829줄) | 7(`native_wsl_owned_fixture` 포함) | `documents::scope`(Task 2) | **참고만**. vault identity·root lease는 옮기지 않고 경계 규칙만: traversal·바깥 symlink·조상 symlink 거절, 값은 오류에 싣지 않음 | 대응 시험: Task 2 `write_outside_registered_roots_is_refused` |
| AU-K2 출처 `72ff50c7:docs/superpowers/plans/2026-10-03-product-readiness/08-traceability.md` 52행 | — | Task 8 | "K2 · P2 · 반복 observe 뒤 idle 종료, 마지막 입력 후 298초 집계 · `sessionizer.rs:55` · ACTIVITY-01: idle-tail 구간 제외, pause/clock 경계 보존" | v0.9.0 수정본 시험 2개(위) + 데몬 경로 회귀 `ingest_with_idle_tail_stores_end_at_last_input` |

합계: 그대로 이식 37개(activity 25·markdown 12), 적응 이식 25개(documents 3·frontmatter 7·wikilink 5·templates 5·assets 5), 제외 3개(WSL1 1·Windows DACL 2), 참고만 18개(rename 11·vault 7).

### Task 1 결과: 계획과 달라진 점 (PROGRESS §5로 옮긴다)

| 위치 | 06 초안 | 실제 | 이유 |
|---|---|---|---|
| Task 2 Files | `crates/core/src/paths.rs`에 `WriteScope` | `crates/documents/src/scope.rs`, root는 CLI가 주입 | `crates/core` 동결. 범위에 필요한 프로젝트·에이전트 폴더 목록은 projects/agents 서비스가 알고 core는 모른다 |
| Task 2 sys | `nix`(`renameat2`) | `rustix::fs::renameat_with` | 데몬 배포가 musl이고 nix 0.30의 `renameat2`는 `target_env = "gnu"` 전용. workspace `unsafe_code = "deny"`라 libc 직접 호출 불가. rustix는 이미 lock에 있음 |
| Task 2 rev | `DiskRev.hash` blake3 | SHA-256(`sha2`, 기존 의존) | 새 의존을 피함(CONVENTIONS §9). 문서 2–5 MiB에서 성능 차이 무시 가능 |
| Task 2 충돌 | `documents.conflict{ diskRev, diskText }` | `documents.conflict{ rev }` + `documents.read{ key }` | 세부 필드는 한 단어(01 §5.3), 오류에 2–5 MiB 본문을 싣지 않음(프레임 16 MiB) |
| Task 2 lease | lease 수명 미정 | 1차: lease를 claim한 **연결·채널의 수명**에 묶어 `ChannelClose`·`ChannelOpen` 재사용·연결 종료 때 즉시 해제(01 §5.1 "그 채널의 구독·스트림·문서 lease를 일괄 정리"). 2차(fallback): `documents.renew` 20초 주기, 60초 무갱신이면 만료 | `server.rs`가 이미 `Control::ChannelClose`에서 채널 상태(`Channel`)를 지우고 `Drop`이 `generation` 토큰을 취소한다. claim·take만 dedupe를 거치지 않고(dedupe는 mutation의 `Ctx.cancel`을 새 토큰으로 바꿈) 이 토큰의 자식을 `Ctx.cancel`로 받아 `Router::dispatch`를 직접 부르면 동결된 `Ctx`·`Router`를 바꾸지 않고 연결별로 구분된다. renew는 반쯤 열린 연결(브리지 정지·소켓 미종료) 대비 |
| Task 2 임시 파일 정리 | 시작 때 등록 root 전체에서 1시간 넘은 `.devbox-tmp-*` 탐색 | 임시 파일을 만들기 전 데이터 폴더 장부(`documents/tmp/<id>`)에 경로를 적고, 시작·매시 장부만 확인해 1시간 넘은 것 삭제 | 프로젝트 트리 전체 탐색은 느리고(node_modules) gitignore 폴더를 놓침. 장부는 정확하고 싸다 |
| Task 2 이동 이벤트 | 없음 | 주제 `documents.moved{ from, to }` | rename된 dirty 문서를 연 창이 새 키로 바꿔야 함(CV-F03) |
| Task 4 렌더 | Mermaid `<pre class="mermaid">` | v0.9.0 자리표시 `<div class="mermaid-block" data-idx>` + 원문 목록 | v0.9.0 시험 12개를 그대로 쓰고, mermaid 원문을 HTML에 넣지 않음 |
| Task 4 렌더 | 원격 이미지 차단(v0.9.0 그대로 이식이라고 적음) | v0.9.0은 원격 이미지를 통과시킴 → 그 한 분기만 바꿔 차단 알림으로 | 01 §6.4·CSP. 나머지 ammonia 설정은 그대로 |
| Task 4 링크 해석 | "가까운 폴더 우선(v0.9.0 vault.rs 규칙)" | v0.9.0은 유일한 key만 해석(모호하면 미해석). v1 규칙: 정확한 경로 key → 유일한 key → 같은 폴더 → 공통 조상이 가장 깊은 것, 그래도 동률이면 모호(후보 목록) | v0.9.0에 가까운 폴더 규칙이 없음. 06 의도를 살리고 결정적으로 정함 |
| Task 4 설정 | `crates/core/src/config.rs`에 `[notes]` | 같지만 **공유 파일 예외 후보 1**(core 세 필드는 opaque 표, 키 검증은 도메인) | `deny_unknown_fields` 때문에 불가피. Task 7·8의 `[search]`·`[activity]`도 같은 커밋 |
| Task 5 알림 | `notify.raised`로 "링크를 고치지 못한 노트 n개" | 주제 `notes.rename_incomplete{ opId, count }` → 화면 알림 센터 | `NotificationReason`·필수 `project_id`가 동결 core에 있음 |
| Task 7 저장된 검색 | 위치 미정 | devbox.db 마이그레이션 도메인 `search`(`search_saved`) | index.db는 백업 없는 파생 데이터라 사용자 자료를 두면 안 됨 |
| Task 7 MCP 검색 | `search.query`(stream) | MCP·빠른 열기는 `search.notes{ text, limit }` query | MCP 클라이언트는 스트림을 읽지 않음 |
| Task 8 ingest | `{ at, app, title?, idle?, dropped? }` | 관찰마다 `lastInputMs`, 종류 `foreground·input·idle·pause` | AU-K2 수정은 `observe_with_input(..., last_input)`·`close_at_last_input`으로만 동작. 입력 시각이 없으면 수정이 우회됨 |
| Task 8 상태 | `activity.status` 주제 | 조회 `activity.status`(query) + 주제 `activity.status_changed` | 메서드와 주제 이름을 구분. AR-F12는 별도 query |
| Task 9 privacy | `crates/win/src/privacy.rs` 새 구현 | `devbox-activity`(`default-features = false`)의 `CompiledRules`를 앱이 재사용 | 같은 규칙의 두 구현이 갈라지는 것을 막음. core 없이 빌드됨 |
| Task 9 앱→데몬 | 앱이 `activity.ingest` 직접 | 네이티브 수집기 → 네이티브 버퍼(1,000건) → Tauri 명령 `activity_drain`/`activity_ack` → 렌더러가 `activity.ingest` | 채널 0은 `NativeSecrets`·Windows 실행이 점유. 렌더러 채널은 App으로 식별됨. 제목은 네이티브에서 규칙 적용 뒤에만 JS로 감 |
| Task 9 v0.9.0 공존 | `%LOCALAPPDATA%` v0.9.0 설정 파일로 켜짐 확인 | v0.9.0 동의는 SQLite(`%LOCALAPPDATA%\com.devbox.v08.knowledge.i{suffix}\active-stores.json` → `stores/<세대>/activity/data.db`의 `settings.product_activity_collection_v1`. 근거 `git show v0.9.0:apps/devbox-agent/src/lib.rs` 80행 부근 `com.devbox.v08.knowledge.i{suffix}`, `.github/scripts/windows-suite-legacy-upgrade-ui.mjs` 316행 부근). 앱에 rusqlite를 넣지 않고 **`com.devbox.v08.knowledge.i*\active-stores.json` 중 하나라도 있으면 안내**(보수적 상위 집합) | Windows 앱에 SQLite를 추가하지 않기 위함. 켜짐 여부까지 보지 않아 꺼져 있어도 안내가 보일 수 있음 |
| Task 10 Files | `app/src/ui/Tree.tsx`·`Timeline.tsx` | `features/notes/NoteTree.tsx`·`features/activity/ActivityBars.tsx` | `app/src/ui` 동결. 공용화는 S4-9 |
| Task 11 Files | `app/src-tauri/src/{shortcuts,tray}.rs` | `src/platform/{shortcuts,tray,capture,activity}.rs`, `src/core/{shortcuts,intents,settings,activity}.rs` | 실제 경로 |
| Task 11 검색 탭 | "본문 탭" | `/search` 경로(router.tsx 줄 추가) | 본문 탭 체계가 없음. 경로는 분리 창·뒤로 가기와도 맞음 |
| Task 11 캡처 300ms | 측정만 | 숨긴 캡처 창을 미리 만들어 두고 단축키는 show만 | 창 생성(WebView2)은 300ms를 넘기 쉬움 |
| 전역 | `scripts/loc.sh` | 그대로(`bash scripts/loc.sh 120000`) | — |

---

### Task 1: 실제 코드에 맞춘 상세화와 이식 대상 확정

**Files:**
- Modify: 이 문서

- [x] **Step 1:** `PROGRESS.md` §5(S2 착수 요약 ~ S2 머지·인계)를 읽고 S0b–S2 실제 이름으로 Interfaces를 고쳤다(위 "실제 코드 계약").
- [x] **Step 2: 이식 대상 확정.** v0.9.0 태그와 `72ff50c7`의 K2 항목을 읽어 위 이식 표로 확정했다.
- [x] **Step 3:** Task 2–12를 아래처럼 펼쳤다. "핵심 테스트"와 [00-roadmap §7](00-roadmap.md) 추적표의 S3 시험(CV-F02·F03·F06·F07·F08, AR-F10·F12·F13·D01)을 입력값 그대로 연결했다.
- [ ] **Step 4:** 별도 검토·Biome CI 뒤 커밋(오케스트레이터).

```bash
git add docs/superpowers/plans/2026-10-08-devbox-v1/06-s3-knowledge.md docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md
git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit -m "docs(plan): detail S3 against the S2 code"
```

---

### Task 2: `crates/documents` — 범위·lease·읽기·CAS 저장·저널·이동·감시

**단계별 실행:**
- [ ] **1 · 실패 테스트:** workspace member·`crates/documents/{Cargo.toml,src/lib.rs}` 뼈대를 연결하고 `crates/documents/src/{sys_tests,service_tests,journal_tests}.rs`에 아래 출발 assertion과 핵심 테스트를 먼저 쓴다.
```rust
// crates/documents/src/service_tests.rs (요점)
let (svc, root) = fixture_with_note("a.md", "v1\n");
let c = svc.claim(claim("a.md", "w1")).await?;
std::fs::write(root.join("a.md"), "external\n")?;
let err = svc.save(save(&c, c.rev.clone(), "mine\n")).await.unwrap_err();
assert!(matches!(err, Fail::Domain(DocumentsError::Conflict { .. })));
assert_eq!(std::fs::read_to_string(root.join("a.md"))?, "external\n");
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test -p devbox-documents save_with_stale_base_rev_returns_conflict -- --test-threads=2`. 미구현 심벌·assertion 실패를 RED로 남긴다.
- [ ] **3 · 구현:** 아래 Interfaces. `sys.rs`(rustix 교체·EOL/BOM·mode), `scope.rs`, `lease.rs`(메모리, 주입 가능한 시계), `journal.rs`(데이터 폴더, 원자적 쓰기), `watch.rs`(`notify` 8.2), `service.rs`(RPC 처리). 데몬 조립은 `crates/cli/src/daemon/knowledge.rs`에 `DocumentsService::new(paths, roots_fn, hub)`와 `knowledge_routes.rs` 등록, `state.rs`·`mod.rs`·`routes.rs` 줄 추가, `pnpm gen`, `messages.ts` 문구.

```rust
// crates/documents/src/sys.rs (요점)
use rustix::fs::{renameat_with, unlinkat, AtFlags, RenameFlags};
// `dir` is the parent opened by WriteScope::resolve through an O_NOFOLLOW|O_DIRECTORY chain;
// every name below is a single component relative to that fd (no path re-resolution).
pub fn atomic_replace(dir: &OwnedFd, name: &str, bytes: &[u8], ledger: &TmpLedger, exists: bool) -> Result<(), SysError> {
    let tmp = format!(".devbox-tmp-{}", random_hex(12));
    let entry = ledger.record(dir, &tmp)?;                 // fsync before the temp file exists
    write_new_file_at(dir, &tmp, bytes, mode_at(dir, name)?)?; // openat O_CREAT|O_EXCL|O_NOFOLLOW, fsync
    let flags = if exists { RenameFlags::EXCHANGE } else { RenameFlags::NOREPLACE };
    renameat_with(dir, tmp.as_str(), dir, name, flags).map_err(SysError::rename)?;
    if exists { unlinkat(dir, tmp.as_str(), AtFlags::empty())?; } // tmp now holds the previous bytes
    rustix::fs::fsync(dir)?;
    ledger.remove(entry)
}
```

- [ ] **4 · 통과:** 같은 RED 명령과 `$NARROW cargo test -p devbox-documents -- --test-threads=2`, 데몬 통합 `$NARROW cargo test -p devbox-cli --test documents -- --test-threads=2`, `every_declared_method_is_routed`(`$NARROW cargo test -p devbox-cli every_declared_method_is_routed -- --test-threads=2`), `gen-ts --check`, `pnpm --filter app typecheck`. 이식 시험 3개 포함.
- [ ] **5 · 검토·커밋:** 별도 검토(00 §3.4: 범위 밖 쓰기 0, 로그에 경로·내용 없음, 600줄) · Biome CI 뒤 `feat(documents): add leased atomic document storage`. PROGRESS §5에 RED/GREEN·미실행을 적는다.

**Files:**
- Create: `crates/documents/{Cargo.toml,src/lib.rs,src/model.rs,src/scope.rs,src/sys.rs,src/lease.rs,src/journal.rs,src/watch.rs,src/service.rs}`
- Test: `crates/documents/src/{sys_tests,service_tests,journal_tests}.rs`, `crates/cli/tests/documents.rs`
- Create: `crates/cli/src/daemon/{knowledge.rs,knowledge_routes.rs}`
- Modify(줄 추가): workspace `Cargo.toml`(member·`devbox-documents`·`rustix = { version = "1.1", features = ["fs"] }`), `crates/cli/Cargo.toml`, `crates/cli/src/daemon/{state,mod,routes,server}.rs`(server는 아래 lease 토큰 1분기), `xtask/{Cargo.toml,src/main.rs}`, `app/src/rpc/messages.ts`; 생성 `app/src/rpc/gen/rpc.ts`, `Cargo.lock`

**Interfaces → Produces:**
- `DocKey { root: String, path: String }`(ts: `{ root, path }`). `root`는 `"vault"`·`"project:<projectId>"`·`"agents:<projectId>"`(에이전트 작업 폴더 상위 `<상위>/<이름>-agents`)·`"data"`. `path`는 root 기준 `/` 구분 상대 경로이며 **realpath 해석 뒤** 값이다. 화면은 경로 문자열 대신 이것을 쓴다(symlink로 열면 응답의 `key`가 대상 키다).
- `DiskRev { hash: String /*sha256 hex*/, size: u64, mtime_ns: String, ino: String }`(ts camelCase `mtimeNs`). 같음 = 네 필드 모두 같음.
- `DocPolicy { kind: Note | Code }` → 상한 `Note 2 MiB`·`Code 5 MiB`, `MAX_WIRE_TEXT = 12 MiB`(JSON 이스케이프 길이, 한 응답 안 문서 본문 합계). S3는 Note만 쓴다.
- `WriteScope { roots: Vec<Root { id, path }> }`(`scope.rs`): `resolve(&self, key) -> Result<Resolved { abs, key }, ScopeError>` — `..`·절대 경로·NUL 거부, 부모 체인을 `O_NOFOLLOW`로 열어 realpath 확인, 최종 symlink는 따라가되 대상이 어느 root 안이어야 함. `resolve`는 부모 폴더 fd(`OwnedFd`, `openat(O_NOFOLLOW|O_DIRECTORY)` 체인)와 마지막 이름 한 성분을 돌려주고, `sys`의 모든 쓰기·이동은 그 fd 기준 `*at` 호출로 한다(확인 뒤 경로를 다시 해석하지 않음). `fn is_internal(rel: &str) -> bool`: 상대 경로의 **성분마다** 검사해 하나라도 `.devbox-tmp-` 접두사·`.trash`·`.git`이면 참(`a/.trash/b.md`, `x/.devbox-tmp-1/y`도 참). 감시·트리·notes 색인·search·링크 재작성이 모두 이 함수로 거른다(CV-F06).
- `type Roots = Arc<dyn Fn() -> BoxFuture<Vec<Root>> + Send + Sync>`: CLI가 등록 프로젝트(`projects.list`)·각 프로젝트의 `<상위>/<이름>-agents`·설정 vault·`data_dir`로 만든다.
- 메서드(`crates/documents/src/lib.rs`):
  - `documents.claim{ key, policy, windowId }`(mutation) → `Claimed { key, leaseId: string|null, rev: DiskRev|null, text: string|null, eol: "lf"|"crlf", bom: bool, readOnly: { reason: "binary"|"encoding"|"too_large"|"leased_elsewhere" }|null, journal: { seq, baseRev, text }|null }`. 파일이 없으면 `rev: null`(새 문서, Orphaned 아님). 같은 `windowId`가 이미 가진 lease면 그 lease를 돌려주고 소유 채널을 이번 요청의 채널로 옮긴다. 응답 예산: `text`와 `journal.text`의 JSON 이스케이프 길이 **합**이 `MAX_WIRE_TEXT`(12 MiB) 이하여야 한다. 넘으면 `journal`은 `{ seq, baseRev, text: null, deferred: true }`로 보내고 화면이 `documents.journal_read{ leaseId } → { seq, baseRev, text }`(query, lease가 가리키는 키의 저널. lease가 없으면 `documents.lease_lost`, 저널이 없으면 `documents.not_found`, 이스케이프 12 MiB 초과면 `documents.too_large`)로 따로 받는다. 이 세 코드는 `DocumentsError`와 `messages.ts`에 이미 있는 행을 쓴다.
  - **lease 수명(1차, 채널 묶음):** `documents.claim`·`documents.take`는 채널 수명 토큰을 받는다. 일반 요청 경로는 `d.dedupe.dispatch`를 지나는데, `crates/cli/src/daemon/dedupe.rs`의 `Dedupe::dispatch`는 mutation이면 `Ctx.cancel`을 새 `CancellationToken::new()`로 바꾼다("An accepted mutation owns its execution"). 그래서 토큰만 바꿔서는 핸들러에 닿지 않는다. `server.rs`의 일반 요청 경로에 **dedupe를 거치지 않는 분기**를 하나 둔다(약 15줄, 공유 파일 표의 "줄 추가만" 행):
    - `if super::knowledge_routes::channel_scoped(&method)`(claim·take 두 이름만 참)이면 `owner = state.generation.child_token()`을 `Ctx.cancel`로 넣어 `d.router.dispatch(&method, Ctx { channel: ch, cancel: owner, streams: Some(opener) }, params)`를 **직접** 부르고, 응답 전송 방식은 기존 분기와 같다. `clientRequestId`는 무시한다(claim·take는 `windowId` 기준 멱등이라 재전송이 같은 lease를 돌려준다).
    - `inflight`에는 owner와 무관한 **별도 토큰**을 등록한다. 그래서 중복 id 검사는 그대로이고, `Cancel{id}`는 그 토큰만 취소해 lease에 영향이 없다(응답 전송과 `inflight.remove` 사이에 도착한 Cancel 경합도 없음). 화면은 claim·take에 `signal`을 주지 않는다.
    - `state.generation`은 연결마다 따로 만든 `Channel`의 토큰이고 `Channel::drop`이 취소하므로, `ChannelClose`·같은 번호의 `ChannelOpen`·연결 종료(`drop(channels)`) 때 취소된다. 다른 연결의 같은 채널 번호는 다른 `Channel`·다른 토큰이라 영향이 없다. `Router::dispatch`의 `select!`가 owner 취소 시 요청 중인 claim을 `cancelled`로 끝낸다.
    - 핸들러는 lease를 표에 넣는 **같은 임계 구역 안에서** `ctx.cancel.is_cancelled()`를 확인하고, 이미 취소됐으면 넣지 않고 `cancelled`를 돌려준다. 넣었다면 `owner.cancelled()`를 기다리는 작업을 붙여, 그 lease가 **아직 같은 owner일 때만** 해제한다(같은 `windowId` 재claim으로 owner가 바뀌었으면 무시). 채널이 닫히는 중의 in-flight claim이 lease를 남기지 않는다.
    - 새 분기의 실행도 기존 분기처럼 `d.dedupe.tracker.spawn(...)` 아래에서 돈다(데몬 종료 때 `tracker.wait()` drain 대상).
    - 핸들러에서 lease 삽입과 owner watcher 등록 사이에는 `await`가 없어야 한다(같은 동기 임계 구역에서 삽입·`is_cancelled` 확인·watcher `spawn`).
    - 동결된 `Ctx`·`Router`는 바꾸지 않는다. 다른 모든 메서드는 지금과 같이 dedupe 경로를 탄다(분기 조건이 두 메서드 이름뿐).
  - `documents.renew{ leaseIds: string[] }`(mutation) → `{ lost: string[] }`(**2차, fallback**). 20초마다. 60초 무갱신 lease는 만료된다. 채널·연결 종료를 데몬이 보지 못하는 경우(반쯤 열린 연결: 브리지가 멈췄지만 소켓이 닫히지 않음)만 대비한다. **다른 창에서 최대 60초 `leased_elsewhere`가 보이는 부작용은 이 fallback 경로에서만 생긴다.** 정상 창 닫기·새로 고침·연결 종료는 즉시 풀린다.
  - `documents.release{ leaseId }`(mutation) → `Done`
  - `documents.take{ key, windowId }`(mutation) → `Claimed`. 이전 lease 보유 창에 주제 `documents.lease_lost`.
  - `documents.save{ leaseId, seq, baseRev: DiskRev|null, expect: "present"|"absent", text }`(mutation) → `{ rev: DiskRev }`. 성공하면 같은 문서 락 안에서 lease의 **현재 키** 저널을 저장된 seq ≤ `seq`일 때 지운다(저장과 저널 삭제 사이 종료로 같은 내용의 복구 후보가 남지 않게). 오류 `documents.conflict{ rev }`(디스크가 baseRev와 다름. 삭제됐으면 `rev: null`)·`documents.lease_lost{ reason: "taken"|"expired" }`·`documents.out_of_scope`·`documents.exists`(expect absent인데 있음)·`documents.too_large{ limit }`·`documents.unsupported`(renameat2 미지원 FS). 저장 텍스트는 `\n`·BOM 없음이고 claim 때 기록한 EOL·BOM으로 바꿔 쓴다(`sys::normalize_for_disk`).
  - `documents.read{ key }`(query) → `{ rev: DiskRev|null, text: string|null, readOnly: … }`: lease 없이 지금 디스크 내용(비교 화면·충돌·외부 변경 다시 읽기용).
  - `documents.journal{ leaseId, seq, baseRev, text }`(mutation) → `Done`. 키를 받지 않고 **lease가 지금 가리키는 키**에 쓴다. 같은 lease에서 저장된 seq보다 큰 것만 쓴다. 아니면 `documents.stale_journal`. 문서별 락(`lease.rs`의 키별 `tokio::sync::Mutex`)으로 save·journal·rename을 직렬화한다.
  - `documents.journal_drop{ key, uptoSeq }`(mutation) → `Done`: 저장된 seq ≤ uptoSeq일 때만 지운다(CAS). lease 없이 가능(Recovering 버림·닫기 버림).
  - `documents.rename{ leaseId|null, key, to: DocKey }`(mutation) → `{ key }`: 문서 락을 옛 키·새 키 순서(키 문자열 정렬)로 잡고 ① 옛 키 저널을 새 키 이름으로 복사·fsync ② `RENAME_NOREPLACE` ③ lease의 키를 새 키로 바꾸고(같은 `leaseId`·owner 유지) 감시 대상을 옮김 ④ 옛 키 저널 삭제 ⑤ 주제 `documents.moved`. ② 실패면 새 키 저널을 지우고 원래대로. 같은 root 안만. 락 밖에서 대기하던 `documents.journal{leaseId}`는 ③ 뒤에 실행돼 새 키에 쓴다. 시작 때 같은 내용의 옛·새 키 저널이 둘 다 있으면(④ 전 종료) 파일이 있는 쪽만 남긴다. 링크 재작성은 notes가 이 위에서 한다(Task 5).
- 주제: `documents.changed { key, rev: DiskRev|null }`(감시 중인 문서·vault 파일, 삭제면 null. 자기 저장 echo는 rev가 같아 화면이 버림), `documents.moved { from, to }`, `documents.lease_lost { key, leaseId }`.
- 감시: 열린(claim된) 문서의 부모 폴더와 vault 전체만 inotify로 본다(프로젝트 트리 전체 감시 안 함). 300ms 묶음. 내용 해시가 같으면 `changed`를 내지 않는다(R8).
- 저널: `<data>/journals/<sha256(root+"\0"+path) hex>.json` `{ key, seq, baseRev, text, savedMs }`. 문서당 최신 하나. 임시 파일 장부 `<data>/documents/tmp/<id>`(대상 경로 한 줄): 시작 때와 1시간마다 1시간 넘은 항목의 `.devbox-tmp-*`와 장부를 지운다.
- 서비스 공개 API(다른 crate용): `DocumentsService::{read_disk(key), write_disk(key, baseRev, expect, text) -> DiskRev, move_entry(from, to), scope() -> WriteScope}`. notes가 lease 없는 파일 쓰기(링크 재작성·첨부·캡처·일일 기록)에 쓴다. 열린 문서에 쓸 때도 CAS라 덮어쓰지 않는다.
- 오류 enum `DocumentsError`: `Conflict { rev: Option<DiskRev> }`, `LeaseLost { reason: LeaseLoss }`, `OutOfScope`, `NotFound`, `Exists`, `TooLarge { limit: u64 }`, `StaleJournal`, `Unsupported`, `InvalidInput`.

**핵심 테스트:**
- `save_with_stale_base_rev_returns_conflict`: claim → 밖에서 `external\n` → save(`mine\n`) → `documents.conflict`, 디스크 `external\n` 그대로, 저널 그대로(CV-F02 서버 쪽)
- `save_preserves_mode_crlf_and_bom`: mode 0640·CRLF·BOM 파일 → `"a\nb\n"` 저장 → 디스크 `\u{feff}a\r\nb\r\n`, mode 0640
- `save_through_symlink_writes_the_target`: `link.md → real/a.md`(같은 root) → claim의 `key.path == "real/a.md"`, 저장 뒤 link는 symlink 그대로
- `binary_non_utf8_and_oversized_files_open_read_only`: NUL 포함·`0xff`·2 MiB+1 노트 → `readOnly.reason` 각각 `binary`·`encoding`·`too_large`, 이스케이프 12 MiB 초과 Code 문서도 `too_large`
- `write_outside_registered_roots_is_refused`: `../x.md`, 바깥을 가리키는 symlink, 바깥을 가리키는 조상 폴더 symlink → `documents.out_of_scope`, 바깥 파일 불변, 오류 세부에 경로 없음(vault.rs 참고)
- `second_window_gets_read_only_until_take`: w1 claim → w2 claim `leased_elsewhere` → w2 take → w1에 `documents.lease_lost`, w1 save `lease_lost{taken}`
- `owner_cancel_releases_only_that_lease`(서비스 단위, 토큰 주입): owner A로 `a.md`, owner B로 `b.md` claim → A 취소 → w2의 `a.md` claim이 바로 lease를 받음, `b.md`는 `leased_elsewhere`. 같은 windowId가 owner C로 `b.md`를 재claim한 뒤 B 취소 → `b.md` lease 유지(owner가 바뀜)
- `lease_survives_reconnect_with_same_window_and_expires_without_renewal`(주입 시계, fallback): owner 토큰을 취소하지 않은 채(반쯤 열린 연결 흉내) w1 claim → 50초 뒤 renew → 같은 windowId 재claim은 같은 leaseId. 61초 무갱신 → w2 claim이 lease를 받음
- `oversized_claim_defers_the_journal`: 1.9 MiB 본문(이스케이프 시 2배가 되는 `"`·`\\` 위주)과 같은 크기 저널 → 합계가 12 MiB 이하면 한 응답, 시험용 `MAX_WIRE_TEXT` 주입값(4 MiB)을 넘으면 `journal.deferred == true`·`text == null` → `documents.journal_read{leaseId}`가 같은 seq·text. 다른 창 leaseId로는 `lease_lost`
- `journal_rejects_older_seq_and_drop_is_cas`: seq 5 저장 → seq 4 `stale_journal` → drop(upto 4) 저널 남음 → drop(upto 5) 저널 0
- `external_edit_emits_changed_with_new_rev`: claim → 밖에서 수정 → 1초 안에 `documents.changed{rev}`(새 해시). 같은 내용으로 `touch`만 3회 → changed 0회
- `rename_noreplace_refuses_existing_target`: `b.md` 있음 → rename a→b `documents.exists`, 두 파일 불변
- `rename_moves_the_journal_to_the_new_key_and_drops_the_old_one`: dirty 저널(seq 3)이 있는 `a.md` → rename `b.md` → 새 키 저널 1개, 옛 키 저널 0개, lease의 키가 `b.md`·같은 leaseId
- `dirty_rename_then_save_leaves_no_old_key_journal`(00 §7 CV-F03 입력 그대로: dirty → rename → **저장** → 다시 열기): `a.md` claim → journal seq 1 → 시험 hook `before_rename_commit`에서 멈춘 rename(a→b)과 동시에 `documents.journal{leaseId, seq:2}` 요청 → hook 해제 → rename 완료 → `documents.save{leaseId, seq:2}` 성공 → 서비스 재생성 → `a.md` claim의 `journal == null`, `b.md` claim의 `journal == null`, `<data>/journals/` 안 파일 0개, 디스크 `b.md` = seq 2 내용
- `internal_temp_files_are_invisible_and_swept_on_start`: 실패 주입(`after_tmp_written`)으로 `.devbox-tmp-*`와 장부를 남김 → `documents.changed`가 오지 않음 → 파일·장부 시각을 2시간 전으로 → 서비스 재생성 → 둘 다 지워짐. 장부에 없는 손으로 만든 vault의 `.devbox-tmp-x`(mtime 2시간 전)도 시작 때 지워지고, 같은 이름의 10분 전 파일은 남음(01 §9 `documents` "데몬 시작 때 1시간 넘은 것을 지운다"). 장부 밖 탐색은 vault와 claim 이력이 있는 문서의 부모 폴더만(프로젝트 트리 전체를 걷지 않음). `is_internal("a/.trash/b.md")`·`is_internal("x/.devbox-tmp-1/y")` 참
- 이식: `private_modes_exchange_and_no_clobber_preserve_both_writers`, `unrelated_paths_and_symlinks_are_rejected`, `moves_files_and_directories_but_preserves_existing_destinations`
- 데몬 통합 `crates/cli/tests/documents.rs::claim_save_read_round_trip_over_rpc`: 실제 데몬에서 claim → save → read, vault root가 `~/notes`(설정 전 기본값)
- 데몬 통합 `crates/cli/tests/documents.rs::channel_close_releases_its_leases`: raw wire(기존 `crates/cli/tests/daemon_cases/channels.rs`의 `control_frame` 방식)로 연결 A 채널 1에서 `a.md`, 연결 B 채널 1(같은 번호)에서 `b.md` claim → A가 `Control::ChannelClose`(채널 1) → 연결 C의 `a.md` claim은 즉시 lease(`readOnly == null`), `b.md` claim은 `leased_elsewhere`(B 유지)
- 데몬 통합 `connection_drop_releases_its_leases`: 연결 A 소켓을 닫음 → 1초 안에 C의 `a.md` claim이 lease를 받음(60초 fallback을 기다리지 않음)
- 데몬 통합 `inflight_claim_on_a_closing_channel_leaves_no_lease`: 시험 hook `before_lease_insert`(시험 빌드 전용)로 A의 claim을 멈춤 → A `ChannelClose` → hook 해제 → A는 응답을 받지 못하거나 `cancelled`, C의 `a.md` claim은 즉시 lease
- 데몬 통합 `cancel_after_claim_does_not_release_the_lease`: A claim 성공 직후 같은 id로 `Cancel` → A의 save 성공(lease 유지)
- 데몬 통합 `claim_bypasses_dedupe_but_other_mutations_do_not`: 같은 `clientRequestId`로 claim 두 번 → 같은 leaseId(windowId 멱등). 이어서 같은 `clientRequestId`에 **다른 params**(다른 `key`, 또 다른 `windowId`)로 claim → 우회 경로이므로 각각 정상 `Claimed`(dedupe 경로였다면 `invalid_params`). 대조로 같은 `clientRequestId`·다른 params의 `documents.save` → `invalid_params`. 기존 dedupe 회귀 시험(`crates/cli/src/daemon/dedupe.rs` tests)은 그대로 통과

---

### Task 3: 화면 `DocumentStore`와 노트 편집기 바탕

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `app/src/features/documents/machine.test.ts`·`store.test.ts`·`EditorHost.test.tsx`에 아래 출발 assertion과 핵심 테스트(01 §8.4 표의 행마다 하나 이상)를 먼저 쓴다.
```ts
// app/src/features/documents/machine.test.ts (요점)
let s = reduce(clean("a"), { type: "edit", text: "x" }).doc;
s = reduce(s, { type: "saveTick" }).doc;            // Saving
const r = reduce(s, { type: "saveFailed", code: "documents.conflict", rev: diskRev("d2") });
expect(r.doc.state).toBe("Conflict");
expect(r.effects).not.toContainEqual(expect.objectContaining({ type: "journalDrop" }));
```
- [ ] **2 · 실패 확인:** `$NARROW pnpm --filter app exec vitest run src/features/documents/machine.test.ts --maxWorkers=2` → 모듈 없음이 아닌 assertion RED가 나올 때까지 빈 `reduce` 뼈대를 둔다.
- [ ] **3 · 구현:** 순수 리듀서 → 효과 실행기(`client.call`) → Zustand store → `offlineJournal`(주입 가능한 저장소, 기본 `idb-keyval`) → `EditorHost`. `app/package.json`에 `idb-keyval`·`@codemirror/lang-markdown`·`@codemirror/autocomplete`를 줄 추가하고 `pnpm install`로 lock을 만든다.

```ts
// app/src/features/documents/machine.ts (요점)
case "Saving":
  if (e.type === "saveOk") {
    const done = d.editSeq === d.inflight!.seq;
    return out({ ...d, diskRev: e.rev, savedSeq: d.inflight!.seq, inflight: undefined,
      pendingExternal: d.pendingExternal.filter((r) => !sameRev(r, e.rev)), state: done ? "Clean" : "Dirty" },
      done ? [{ type: "journalDrop", uptoSeq: d.inflight!.seq }] : [{ type: "scheduleSave", ms: 1000 }]);
  }
  if (e.type === "saveFailed" && e.code === "documents.conflict")   // classify by generated code only (CV-F02)
    return out({ ...d, inflight: undefined, conflictRev: e.rev, state: "Conflict" }, [{ type: "readDisk" }]);
```

- [ ] **4 · 통과:** 같은 RED 명령, `store.test.ts`·`EditorHost.test.tsx`, `pnpm --filter app typecheck`.
- [ ] **5 · 검토·커밋:** 별도 검토(표의 모든 행에 시험이 있는지, 오류를 문구로 분류하지 않는지) · Biome 뒤 `feat(app): add the shared document state machine and editor host`. **묶음 S 끝:** PROGRESS의 묶음 S 행·트랙 1 위치를 갱신해 로컬 커밋한다.

**Files:**
- Create: `app/src/features/documents/{machine.ts,store.ts,effects.ts,offlineJournal.ts,EditorHost.tsx,editorExtensions.ts,windowId.ts}`
- Test: `app/src/features/documents/{machine.test.ts,store.test.ts,EditorHost.test.tsx}`
- Modify(줄 추가): `app/package.json`; 생성 `pnpm-lock.yaml`

**Interfaces → Produces:**
- `machine.ts`: `reduce(doc: Doc, event: DocEvent): { doc: Doc; effects: Effect[] }`(순수)
  - 상태: `Closed·Loading·Clean·Dirty·Saving·Recovering·Conflict·Orphaned·ReadOnly`
  - `Doc { key, state, leaseId?, diskRev: DiskRev|null, editSeq, savedSeq, inflight?: { seq, baseRev }, journalSeq, pendingExternal: (DiskRev|null)[], conflictRev?, text, eol, bom, readOnly?, policy }`
  - 이벤트: `open{key,policy}`·`claimed{Claimed}`·`edit{text}`·`journalTick`·`journalOk{seq}`·`saveTick`·`save`·`saveOk{rev}`·`saveFailed{code, rev?}`·`reconnected`·`external{rev|null}`·`diskLoaded{rev,text}`·`restore`·`discard`·`resolve{choice:"mine"|"disk"|"merged", text?}`·`leaseLost`·`moved{to}`·`close{choice:"save"|"discard"|"keep"}`
  - 효과: `claim`·`save{seq,baseRev,expect,text}`·`journal{seq,baseRev,text}`·`journalDrop{uptoSeq}`(**서버 `documents.journal_drop`과 이 기기 `offlineJournal.drop(key, uptoSeq)`를 함께** 실행. 하나만 지우는 효과는 두지 않는다)·`scheduleJournal{ms:300}`·`scheduleSave{ms:1000}`(Note 정책만)·`readDisk`(`documents.read`)·`showCompare`·`applyText{text, undoable:true}`·`release`·`offlineJournal{…}`
  - 오류 분류는 `RpcError.code`(생성된 `RpcErrorCode`)로만 한다. `connection_lost`·`internal`은 Dirty 유지 + 재연결 뒤 같은 baseRev로 재시도, `documents.lease_lost`는 `{reason:"expired"}`면 조용히 재claim, `taken`이면 ReadOnly.
- `store.ts`(Zustand): `useDocuments` — `open(key, policy)`, `edit(key, text)`, `save(key)`, `close(key, choice)`, `resolve(key, choice, text?)`, `restore(key)`, `discard(key)`, 주제 `documents.changed`·`documents.moved`·`documents.lease_lost` 구독과 20초 `documents.renew`.
- `effects.ts`: 효과를 `client.call`로 실행하고 결과를 이벤트로 되돌린다. 타이머는 주입 가능(`vi.useFakeTimers`).
- `offlineJournal.ts`: `OfflineJournal { put(key, entry), get(key), drop(key, uptoSeq) }`(entry `{ seq, baseRev, text }`, drop은 저장된 seq ≤ uptoSeq일 때만), 기본 구현 `idb-keyval`(문서당 최신 1개). 항목은 `{ kind: "draft", seq, baseRev, text }` 또는 서버 drop 실패 때 남기는 `{ kind: "tombstone", dropUpto }`다. 재연결하면 tombstone의 `documents.journal_drop`을 **저장 재시도보다 먼저** 다시 보내고 성공하면 지운다. 시험은 메모리 구현을 주입한다. 다시 연결되면 **남아 있는** 항목만 같은 `baseRev`로 저장을 다시 시도하고 성공하면 지운다. 문서를 열 때 서버 journal이 없어도 offline 항목이 디스크와 다르면 Recovering 후보다. 그래서 [디스크 것]·복구 버림·닫기 버림은 반드시 `journalDrop`(양쪽)을 낸다.
- `windowId.ts`: Tauri는 `getCurrentWindow().label`, 브라우저는 `window.name || "main"`.
- `EditorHost({ docKey, language, readOnly, onSave })`: CodeMirror `EditorView` 하나. `codeTheme`·`codePhrases`·`loadLanguage`(`app/src/ui/code`, 고치지 않음)를 쓴다. 문서별 `Map<keyString, EditorState>`를 두고 문서를 바꿀 때 `view.setState(state)`(AU-K1). 외부 변경 다시 읽기는 `addToHistory` 기본(undo 가능) 트랜잭션. Ctrl+S는 `Prec.highest(keymap.of([{ key: "Mod-s", run: save }]))`. 컴포넌트 300줄 아래(확장은 `editorExtensions.ts`).

**핵심 테스트(01 §8.4 표의 행마다 하나 이상):**
- `own_save_echo_is_ignored_while_saving`: Saving 중 `external{rev:r2}` 보류 → `saveOk{r2}` → Clean, readDisk 효과 없음
- `edit_during_save_stays_dirty_with_new_base_rev`
- `external_change_while_dirty_goes_to_conflict`
- `external_change_while_clean_reloads_as_undoable_transaction`: `readDisk` → `diskLoaded` → `applyText{undoable:true}`
- `external_delete_goes_orphaned_and_save_recreates_with_expect_absent`
- `lease_lost_goes_read_only`(taken) / `expired_lease_reclaims_quietly`(expired)
- `close_dirty_requires_a_choice`: `close` 선택 없이 → 상태 그대로. `discard` → `journalDrop{uptoSeq: journalSeq}`, `keep` → journalDrop 없음
- `io_error_keeps_dirty_and_retries_same_base_after_reconnect`: `saveFailed{connection_lost}` → Dirty·offlineJournal 효과 → `reconnected` → `save{baseRev: 이전 baseRev}`
- `conflict_take_disk_drops_the_journal`: Conflict → `resolve("disk")` → 효과에 `journalDrop{uptoSeq: 현재 journalSeq}` → 같은 문서 재`claim`(가짜 응답 journal null) → Recovering 아님(AR-F13)
- `store.test.ts` `take_disk_also_clears_the_offline_journal_and_reconnect_does_not_resave`(AR-F13, 메모리 `OfflineJournal` 주입): 연결 끊김 중 편집 → offline 항목 seq 4 → 재연결 전 충돌 → [디스크 것] → `offlineJournal.get(key) === undefined`, 서버 `documents.journal_drop{uptoSeq:4}` 1회 → `reconnected` → `documents.save` 호출 0 → 문서 닫고 다시 열기 → 상태 Clean(Recovering 아님)
- `store.test.ts` `server_drop_failure_is_retried_from_a_tombstone`: 끊긴 상태에서 [디스크 것] → 서버 `journal_drop`이 `connection_lost` → offline 항목 대신 tombstone `{ dropUpto: 4 }`가 남음 → `reconnected` → `journal_drop{uptoSeq:4}` 재시도 1회·save 0회 → 성공 뒤 tombstone 없음, 다시 열면 Recovering 아님
- `store.test.ts` `recovering_discard_and_close_discard_clear_both_journals`: 두 경로 모두 서버 drop 1회 + offline 항목 없음, 다시 열면 Recovering 아님
- `recovering_discard_and_close_discard_drop_the_journal`: 두 경로 모두 `journalDrop`
- `repeated_conflict_errors_never_overwrite_the_buffer_or_journal`: `documents.conflict`를 두 번 연속 → 버퍼·journalSeq 그대로, 상태 Conflict, journalDrop 0(CV-F02)
- `moved_event_rekeys_the_document`: `moved{to}` → key 교체, 저널 효과 없음(CV-F03 화면 쪽)
- `EditorHost.test.tsx`: 노트 A 편집 → B 편집 → A로 돌아와 Ctrl+Z → A의 마지막 편집만 되돌림, B 그대로(AU-K1). Ctrl+S가 `onSave`를 한 번 부름

---

### Task 4: `crates/notes` — vault·frontmatter·링크·백링크·템플릿·첨부·렌더

**Step 0(선행, 공유 파일 예외 후보 1):** PROGRESS §2에 기록하고(무엇·왜·최소 범위) 진행한다. `crates/core/src/config.rs`에 `notes`·`search`·`activity` opaque 표와 시험 `knowledge_sections_are_tables_and_left_to_their_domains`(`[notes]\nvault="~/n"`가 load되고 `notes = 1`은 "notes must be a table")를 RED→GREEN(`$NARROW cargo test -p devbox-core config -- --test-threads=2`) → 별도 검토 → `feat(core): accept notes search and activity settings sections`. 기록 전에는 Task 4 나머지를 시작하지 않는다(Task 2·3 결과는 영향 없음).

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `crates/notes` 뼈대 뒤 `crates/notes/src/{index_tests,render_tests,vault_tests}.rs`와 이식 시험(frontmatter·links·templates·attachments·render)을 먼저 옮긴다.
```rust
// crates/notes/src/index_tests.rs (요점)
let n = fixture(&[("idea.md", "---\ntitle: \"  \"\n---\nbody\n")]).await;
assert_eq!(n.index_title("idea.md").await?, "idea");
assert_eq!(n.resolve("idea").await?.paths, ["idea.md"]);
assert_eq!(std::fs::read_to_string(n.vault().join("idea.md"))?, "---\ntitle: \"  \"\n---\nbody\n");
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test -p devbox-notes blank_title_falls_back_to_the_file_name -- --test-threads=2`.
- [ ] **3 · 구현:** 이식 → `NotesSettings::from_table` → 색인(`IndexDb::ensure_domain("notes", 1, NOTES_SCHEMA)`, 시작 전체 스캔 + `documents` vault 감시 이벤트) → RPC → CLI 조립(vault root를 `documents` Roots에 넣고, `daemon/logs.rs::roots`의 `~/notes`를 설정 vault로 교체) → 노트 색인 결과를 search에 넘길 sink 자리(`NoteSink` trait, Task 7에서 연결).

```rust
// crates/notes/src/index.rs (요점)
pub const NOTES_SCHEMA: &str = "
CREATE TABLE notes_files(path TEXT PRIMARY KEY, title TEXT NOT NULL, hash TEXT NOT NULL);
CREATE TABLE notes_keys(key TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY(key, path));
CREATE TABLE notes_links(src TEXT NOT NULL, raw TEXT NOT NULL, key TEXT, heading TEXT,
  kind TEXT NOT NULL, from_utf16 INTEGER NOT NULL, to_utf16 INTEGER NOT NULL);
CREATE INDEX notes_links_key ON notes_links(key);
CREATE TABLE notes_tags(path TEXT NOT NULL, tag TEXT NOT NULL, PRIMARY KEY(path, tag));";
fn title_of(meta: &DocMeta, path: &str) -> String {
    meta.title.as_deref().map(str::trim).filter(|t| !t.is_empty())
        .map(str::to_owned).unwrap_or_else(|| file_stem(path))   // AR-F10, source untouched
}
```

- [ ] **4 · 통과:** 같은 RED 명령, `$NARROW cargo test -p devbox-notes -- --test-threads=2`(이식 34개 포함), `$NARROW cargo test -p devbox-cli --test notes -- --test-threads=2`, observe 로그 roots 영향 시험 `$NARROW cargo test -p devbox-cli --test run_logs -- --test-threads=2`(필터 없음: `file_tail_append_search_and_close_use_actual_rpc`·`rpc_rejects_outside_symlink_invalid_regex_and_unknown_run` 포함 4개), `gen-ts --check`, typecheck.
- [ ] **5 · 검토·커밋:** 별도 검토(살균 설정이 v0.9.0과 같은지, 원격 이미지 차단, CV-F06 제외) · Biome 뒤 `feat(notes): add vault indexing, links and sanitized rendering`.

**Files:**
- Create: `crates/notes/{Cargo.toml,src/lib.rs,src/model.rs,src/settings.rs,src/vault.rs,src/frontmatter.rs,src/links.rs,src/index.rs,src/templates.rs,src/attachments.rs,src/render.rs,src/service.rs}`
- Test: `crates/notes/src/{index_tests,render_tests,vault_tests}.rs`(이식 시험은 각 모듈 `#[cfg(test)]`), `crates/cli/tests/notes.rs`
- Modify(줄 추가): workspace `Cargo.toml`(`pulldown-cmark = "0.13"`, `ammonia = "4"`), `crates/cli/src/daemon/{knowledge,knowledge_routes,logs}.rs`, `state.rs`, `routes.rs`(declared), `xtask`, `messages.ts`; 생성 rpc.ts·lock

**Interfaces → Produces:**
- 설정 `[notes]`(`NotesSettings`, `deny_unknown_fields`): `vault = "~/notes"`, `daily_folder = "daily"`, `inbox = "inbox.md"`, `templates_folder = "templates"`. 경로는 `expand_home`. **vault 폴더가 없어 devbox가 처음 만들 때만** 새 `.gitignore`(`.trash/` 한 줄)를 함께 만든다. 이미 있는 vault(두 PC가 Git으로 공유하는 vault 포함)의 `.gitignore`는 읽지도 고치지도 않고, `.trash/`가 무시되지 않으면 설정 › 노트에 "`.trash/`를 .gitignore에 넣으면 휴지통이 커밋되지 않습니다" 안내만 보인다(`git check-ignore -q .trash/`, 읽기 전용).
- 메서드(`crates/notes/src/lib.rs`, 경로는 vault 상대 `path: string`):
  - `notes.tree{ cursor: string|null }`(query) → `{ rev, items: [{ path, dir: bool, title }], nextCursor: string|null, truncated }`: 숨김·`.git`·`is_internal`·`.trash` 제외. 한 페이지 1–256행·JSON 768 KiB(S2 `runtime.list`의 256행/768KiB 페이지 규칙, 05-s2-runs Task 13). `cursor`는 rev와 위치를 담고, 데몬은 첫 페이지를 만들 때 그 rev의 목록 snapshot을 **rev를 키로** 60초 보관해(동시 보관은 최근 2개 rev까지, 넘치면 오래된 것부터 버림) 이어지는 페이지를 같은 snapshot에서 준다(순회 중 갱신돼도 같은 rev로 끝남). snapshot이 만료됐으면 `notes.stale_cursor` → 화면이 처음부터 1회만 다시 받고, 또 실패하면 오류 상태와 [다시 시도]를 보인다(무한 재시작 없음). vault 전체 상한 20,000개(넘으면 마지막 페이지 `truncated: true`)
  - `notes.create{ folder, title, template: string|null, clientRequestId는 RPC 공통 }`(mutation) → `{ path, rev }`: 이름은 제목을 파일 이름으로 정리(금지 문자 제거, 겹치면 ` 2`…), 템플릿 변수 치환, `documents.write_disk(expect absent)`
  - `notes.delete{ path }`(mutation(reversible)) → `{ trashPath }`: vault `.trash/<날짜>/<경로>`로 이동(`move_no_replace`), 10초 되돌리기
  - `notes.restore{ trashPath, path }`(mutation) → `Done`(되돌리기 실행)
  - `notes.tags{}`(query) → `{ rev, items: [{ tag, count }] }`
  - `notes.backlinks{ path }`(query) → `{ rev, items: [{ src, title, line, fromUtf16, toUtf16 }] }`
  - `notes.resolve{ target }`(query) → `{ paths: string[], ambiguous: bool }`. 규칙: `#제목줄`·`|별칭` 분리 → `normalize_link_key` → 정확한 경로 key → 유일한 key(경로·파일 이름·제목) → 같은 폴더 → 공통 조상이 가장 깊은 것 → 동률이면 `ambiguous: true` 후보 전부. 대소문자 무시. Markdown 상대 링크 `[a](../b.md)`도 같은 함수(경로 해석).
  - `notes.render{ path: string|null, text: string|null }`(query) → `{ html, mermaid: string[] }`: 살균 HTML. 로컬 이미지는 vault 안·2 MiB 이하만 `data:` URI로 넣고, 아니면 `md-image-missing`(이유 `not-found`·`too-large`·`outside-root`·`remote-blocked`·`budget`). 응답 예산은 **응답 전체**(html + `mermaid` 원문 배열의 JSON 이스케이프 합) 12 MiB, 그중 인라인 이미지 base64 합계 8 MiB(프레임 16 MiB). 이미지가 넘치면 뒤 이미지부터 `budget` 자리표시로, mermaid 원문이 넘치면 뒤 블록부터 원문을 빈 문자열로 보내고 자리표시에 `data-reason="budget"`을 단다. 그래도 넘으면(본문 자체가 큼) `notes.too_large`.
  - `notes.attach{ notePath, name, data /*base64*/ }`(mutation) → `{ path, markdown }`: 2 MiB, 이미지 형식 판별, `attachments/<sha256 앞 12자>-<정리한 이름>.<확장자>`, 같은 내용이면 같은 이름(이미 있으면 재사용)
  - `notes.templates{}`(query) → `{ items: [{ name, path }] }`
- 주제 `notes.changed { rev, paths: string[] }`(색인이 실제로 바뀐 경우만. 메모리 카운터 rev).
- 색인: `index.db` 도메인 `notes`(위 스키마). 시작할 때 vault 전체 스캔(해시가 같으면 건너뜀)과 `documents`의 vault 감시 이벤트로 갱신. 링크 종류 `wiki`·`md`. 노트 하나 처리 결과를 `NoteSink::indexed(path, title, body)`·`removed(path)`로 넘긴다(Task 7이 search에 연결).
- 오류 `NotesError`: `NotFound`, `Exists`, `OutOfVault`, `InvalidName`, `InvalidTemplate { problem }`, `InvalidImage { problem }`, `InvalidInput`, `Busy`(재작성 진행 중), `StaleCursor`(`notes.stale_cursor`), `TooLarge { limit }`(`notes.too_large`), 하부 `Documents(…)`는 같은 코드로 매핑하지 않고 notes 코드로 바꾼다.

**핵심 테스트:**
- `wiki_links_resolve_with_alias_heading_and_nearest_folder`: `a/x.md`·`b/x.md`·`a/sub/note.md`에서 `[[x]]` → `a/x.md`(공통 조상), `[[b/x|별칭]]` → `b/x.md`, `[[x#요약]]` heading `요약`, 루트의 `[[x]]` → ambiguous 후보 2개
- `backlinks_update_after_external_edit`: `b.md`에 `[[a]]`를 밖에서 추가 → 1초 안에 `notes.backlinks{a.md}` 1건, `notes.changed` 1회
- `render_strips_scripts_event_handlers_and_remote_images` + 이식 12개 + `remote_images_are_blocked_with_a_notice`(Markdown `![x](https://e/x.png)`는 pulldown 이벤트 단계에서 `<img>` 대신 `<span class="md-image-missing" data-reason="remote-blocked">`로 바꿔 → 출력에 `<img` 0. 원시 HTML `<img src="http://e/y.png">`는 ammonia `filter_url_attribute`가 `src`를 지워 → `src="http` 0, `src` 없는 `<img>`만 남음(CSP `img-src 'self' data:`로 요청 0))
- `image_budget_replaces_extra_images_with_budget_placeholders`: 2 MiB PNG 5개 → 처음 3개(base64 합 ≤ 8 MiB)만 `data:`, 나머지 `data-reason="budget"`
- `attachments_are_content_addressed_and_stay_inside_the_vault`: 같은 PNG 두 번 → 같은 경로 1개, `name = "../../x.png"` → 정리된 이름, vault 밖 생성 0
- `delete_moves_to_trash_and_undo_restores`
- `blank_title_falls_back_to_the_file_name`: `title: ""`·`title: "  "`·title 없음인 `idea.md` 세 경우 → 색인 제목 `idea`, `[[idea]]`가 이 파일로 풀림, 원본 바이트 불변(AR-F10)
- `tree_pages_are_bounded_and_share_one_rev`: 600개 노트 → 3페이지(256·256·88), 각 페이지 JSON ≤ 768 KiB, 모두 같은 rev. 첫 페이지 뒤 노트 1개 추가 → 나머지 페이지도 옛 rev snapshot(합 600). 주입 시계 61초 뒤 이어 받기 → `notes.stale_cursor`
- `trash_and_temp_files_are_not_indexed_or_linked`: `.trash/old.md`(`[[a]]` 포함)와 `.devbox-tmp-1` → 트리·태그·백링크·`notes.resolve` 후보에 없음(CV-F06)
- `first_vault_creation_adds_trash_to_gitignore`: 빈 HOME(vault 없음) → 시작 → `~/notes/.gitignore` = `.trash/\n`, 다시 시작해도 같은 바이트
- `existing_vault_gitignore_is_never_modified`: 이미 있는 vault(`.gitignore` = `node_modules/\n`, 또는 `.gitignore` 없음) → 시작 → `notes.delete`로 `.trash/` 첫 생성 → `.gitignore` 바이트·mtime 그대로(없던 경우 여전히 없음)
- 데몬 통합 `crates/cli/tests/notes.rs::configured_vault_is_used_for_documents_and_logs`: `[notes] vault = "~/n2"` → `notes.create` 파일이 `~/n2`, `logs.open{file: ~/n2/x.log}` 허용·`~/notes/x.log` 거절

---

### Task 5: 여러 노트 링크 재작성 (AR-D01)

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `crates/notes/src/rename_tests.rs`. 중단 지점은 시험 전용 주입 `StopAt::{AfterMove, AfterFile(n)}`(`rename_with(.., stop)`)로 만든다.
```rust
// crates/notes/src/rename_tests.rs (요점)
let v = vault(&[("a.md", "x"), ("b.md", "[[a]]"), ("c.md", "[a](a.md)")]);
let op = v.svc.rename_with(rename("a.md", "z/a2.md"), Some(StopAt::AfterMove)).await?;
drop(v.svc);                                    // simulated daemon exit right after the move
let svc = v.restart().await?;                   // resume reads <data>/notes-rename/<op>.json
svc.wait_rename(&op).await?;
assert_eq!(read(&v, "b.md"), "[[a2]]");
assert_eq!(read(&v, "c.md"), "[a](z/a2.md)");
assert!(!v.data().join(format!("notes-rename/{op}.json")).exists());
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test -p devbox-notes crash_right_after_the_move_resumes_link_rewrites -- --test-threads=2`.
- [ ] **3 · 구현:** 대상 확정 → 진행 저널 → `documents.move_entry`(노트가 열려 있으면 `documents.rename`과 같은 저널 이동) → 파일별 CAS → 저널 삭제. 열린 dirty 대상은 `notes.rewrite_request`, 30초 시한. 시작 때 재개, 남은 실패는 `notes.rename_incomplete`.

```rust
// crates/notes/src/rename.rs (요점)
let plan = Plan { op, from, to, files: targets.iter().map(|t| FilePlan { path: t.path.clone(), base: t.rev.clone(), done: false }).collect() };
journal.write(&plan)?;                          // durable before any visible change
if !plan.moved { docs.move_entry(&from_key, &to_key).await?; journal.mark_moved(&op)?; }
for file in plan.pending() {
    match rewrite_one(&docs, &leases, file, &from, &to).await {
        Ok(()) => journal.mark_done(&op, &file.path)?,
        Err(Skip::Conflict | Skip::OpenDirtyTimeout) => failed.push(file.path.clone()), // never overwrite
    }
}
```

- [ ] **4 · 통과:** 같은 RED 명령, `$NARROW cargo test -p devbox-notes rename -- --test-threads=2`, 데몬 통합 `$NARROW cargo test -p devbox-cli --test notes rename -- --test-threads=2`(실제 재시작).
- [ ] **5 · 검토·커밋:** 별도 검토(덮어쓰기 0, 저널이 이동보다 먼저 durable, CV-F06 제외) · Biome 뒤 `feat(notes): rewrite links across notes with a resumable journal`.

**Files:**
- Create: `crates/notes/src/{rename.rs,rename_journal.rs}`, Test: `crates/notes/src/rename_tests.rs`, `crates/cli/tests/notes.rs`(Task 4 파일에 시험 추가)
- Modify: `crates/notes/src/{lib,service}.rs`, `crates/cli/src/daemon/knowledge_routes.rs`, `messages.ts`, 생성 rpc.ts

**Interfaces → Produces:**
- `notes.rename{ path, newPath, rewriteLinks: bool }`(mutation) → `{ opId }`(진행 작업 ID, 01 §8.6-4)
  1. 대상 목록과 각 파일의 `baseRev`를 확정한다(백링크 색인 + 각 파일 내용 다시 파싱. 깨질 링크만 고친다: 제목 key로 걸린 `[[제목]]`은 제목이 그대로면 건드리지 않음, 별칭·heading 보존, 새 key가 모호하면 vault 경로형 `[[폴더/새이름]]`으로 씀).
  2. 진행 저널 `<data>/notes-rename/<opId>.json`: `{ from, to, moved: bool, files: [{ path, baseRev, done }] }`(원자적 쓰기, fsync).
  3. 노트 자체를 옮긴다(`RENAME_NOREPLACE`, 열린 문서면 저널 이동 + `documents.moved`).
  4. 파일마다 링크를 고쳐 CAS 저장(`write_disk(baseRev)`). 끝난 파일은 `done`.
  5. 다 끝나면 저널을 지운다.
- 열린 dirty 대상: 주제 `notes.rewrite_request { opId, key, edits: [{ fromUtf16, toUtf16, text }] }`(lease 보유 창만 반응). 창은 버퍼에 적용하고 저장한 뒤 `notes.rewrite_reply{ opId, key, applied: bool }`(mutation). 30초 안에 응답이 없으면 그 파일은 "열려 있어 고치지 못함".
- CAS 충돌 파일도 목록에 둔다. 덮어쓰지 않는다.
- `notes.rename_status{ opId }`(query) → `{ state: "running"|"done"|"incomplete", failed: [{ path, reason: "conflict"|"open"|"missing" }] }`
- 데몬 시작 때 남은 저널을 다시 시도한다. 저널의 `baseRev`와 지금 디스크가 다르면(재시작 사이 외부 수정) 그 파일은 다시 파싱해 깨질 링크가 남았을 때만 새 rev로 고친다. 그래도 실패한 파일이 있으면 주제 `notes.rename_incomplete { opId, count }`를 낸다(주제 payload라 오류 세부의 한 단어 규칙과 무관, 다른 S3 payload와 같은 camelCase). 화면은 알림 센터에 "링크를 고치지 못한 노트 n개"로 보이고 누르면 목록(`/notes?rename=<opId>`).

**핵심 테스트:**
- `rename_rewrites_wiki_and_markdown_links_in_all_backlinking_notes`: `[[a]]`·`[[a|별칭]]`·`[[a#h]]`·`[t](a.md)`·`[t](./sub/../a.md)` → 모두 새 경로/이름, 별칭·heading 보존
- `conflicting_file_is_reported_not_overwritten`: 대상 확정 뒤 `b.md`를 밖에서 수정(주입 hook) → b 바이트 = 외부 내용, status `incomplete`·`conflict`
- `crash_midway_resumes_from_the_progress_journal`: 파일 2개 처리 뒤(`AfterFile(2)`) 서비스 drop → 새 서비스 → 나머지 처리, 각 파일 한 번만 고쳐짐(중복 치환 0)
- `crash_right_after_the_move_resumes_link_rewrites`: 이동 직후·첫 참조 재작성 전 drop → 다음 시작에 모든 참조가 새 경로로 고쳐지거나 목록으로 보임(AR-D01)
- `crash_after_the_first_rewrite_resumes_the_rest`: `AfterFile(1)` drop → 나머지 재개(AR-D01 재현 지점)
- `dirty_open_document_receives_a_rewrite_request_instead_of_disk_write`: 대상이 w1에서 dirty → `notes.rewrite_request` 1회, 디스크 불변, `rewrite_reply{applied:true}` 뒤 done. 응답 없음(주입 시계 31초) → `failed.open`
- `trash_and_temp_files_are_not_rewritten`: `.trash/old.md`의 `[[a]]`는 그대로(CV-F06)
- `incomplete_rename_raises_one_notice_after_restart`: 재개 뒤 실패 1개 → `notes.rename_incomplete{count:1}` 1회
- 데몬 통합 `crates/cli/tests/notes.rs::rename_resumes_after_daemon_restart`: 실제 데몬에 `DEVBOX_TEST_NOTES_STOP=after_move`(시험 빌드 전용 hook, `cfg(debug_assertions)`)를 주고 `notes.rename{a.md → z/a2.md}` → 데몬이 이동 직후 멈춤 → `TestDaemon::stop` → hook 없이 `restart` → 10초 안에 `notes.rename_status{opId}`가 `done`, `b.md`·`c.md`의 링크가 새 경로, `<data>/notes-rename/` 비어 있음(AR-D01)

---

### Task 6: 일일 기록과 "오늘 한 일" 요약

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `crates/notes/src/daily_tests.rs`(블록 교체 순수 함수)와 `crates/cli/src/daemon/summary_tests.rs`(가짜 원천 주입).
```rust
// crates/notes/src/daily_tests.rs (요점)
let doc = "# 2026-10-21\n\n## pc-a\n<!-- devbox:summary:pc-a -->\nold\n<!-- /devbox:summary -->\n\n## pc-b\n<!-- devbox:summary:pc-b -->\nkeep\n<!-- /devbox:summary -->\n메모\n";
let out = replace_summary(doc, "pc-a", "### 오늘 한 일\n- new\n");
assert!(out.contains("<!-- devbox:summary:pc-a -->\n### 오늘 한 일\n- new\n<!-- /devbox:summary -->"));
assert!(out.contains("keep") && out.ends_with("메모\n") && !out.contains("old"));
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test -p devbox-notes summary_block_is_replaced_in_place -- --test-threads=2`.
- [ ] **3 · 구현:** `daily.rs`(경로·템플릿·블록 교체), `notes.append`, CLI `summary.rs` 조립(trait `SummarySources`로 주입), CLI 명령 `devbox note add`.
- [ ] **4 · 통과:** 같은 RED 명령, `$NARROW cargo test -p devbox-cli summary -- --test-threads=2`, 데몬 통합 `$NARROW cargo test -p devbox-cli --test notes daily -- --test-threads=2`.
- [ ] **5 · 검토·커밋:** 별도 검토(다른 PC 블록·사용자 글 보존, 커밋 메시지 외 개인 데이터 미수집) · Biome 뒤 `feat(notes): add daily notes and the daily summary block`. **묶음 T 끝:** PROGRESS 묶음 T 행·트랙 1 위치 갱신·로컬 커밋.

**Files:**
- Create: `crates/notes/src/daily.rs`, Test: `crates/notes/src/daily_tests.rs`
- Create: `crates/cli/src/daemon/summary.rs`, Test: `crates/cli/src/daemon/summary_tests.rs`, `crates/cli/tests/notes.rs`(`daily_and_append_round_trip` 추가)
- Modify: `crates/notes/src/{lib,service}.rs`, `crates/cli/src/main.rs`(서브명령 `note add`), `crates/cli/src/daemon/knowledge_routes.rs`, `messages.ts`, 생성 rpc.ts

**Interfaces → Produces:**
- `notes.daily{ date: string|null /*YYYY-MM-DD, 없으면 오늘(로컬)*/ }`(mutation) → `{ path, created: bool }`: `<daily_folder>/<YYYY-MM-DD>.md`가 없으면 `templates/daily.md`(없으면 기본 `# {{date}}\n`)로 만든다. 기본 템플릿 frontmatter는 `title: ""`(AR-F10이 이 경우를 막는다).
- `notes.insert_summary{ date }`(mutation) → `{ path, rev }`: 핸들러는 CLI(`summary::insert`)가 원천을 모아 `NotesService::insert_summary(date, pc, markdown)`을 부른다.
  - 일일 기록의 `## <PC 이름>` 소제목(없으면 끝에 추가) 아래 `<!-- devbox:summary:<pc> -->` … `<!-- /devbox:summary -->` 블록을 만들거나 바꾼다. 다른 PC 블록·표시 밖 글은 그대로. CAS 저장(열려 있고 dirty면 Task 5와 같은 `rewrite_request` 경로).
  - 내용(`### 오늘 한 일`):
    - 활동 상위 앱·프로젝트 시간(Task 8 `ActivityService::day`. Task 8 전에는 이 줄을 생략)
    - 등록 프로젝트의 그날 커밋: 프로젝트마다 `git config user.email`을 읽고 `devbox_git::run::output(dir, ["log", "--since=<로컬 0시>", "--until=<다음 0시>", "--author=<email>", "--format=%h%x1f%s", "-n", "50"])`, 프로젝트당 3초 제한
    - 그날 닫힌 에이전트 작업: `AgentsService::list(ListAgents{ include_closed_hours: Some(h), .. })`(`h` = 지금부터 그 날짜 로컬 0시까지의 시간을 올림, 과거 날짜도 포함되게)에서 `closed_ms`가 **그 날짜의 [로컬 0시, 다음 0시)** 안인 것의 `title`·`closed_how`·`pr_url`
    - 실패한 실행·예약: `RuntimeService::runs(..)`/`schedules(..)`에서 끝난 시각이 같은 [로컬 0시, 다음 0시) 안인 실패 결과(제목·작업 ID만). 커밋도 같은 범위(`--since`/`--until`). "지금부터 24시간"을 쓰지 않는다
- `notes.append{ target: "inbox"|"daily", text, format: "plain"|"capture" }`(mutation) → `{ path, rev }`: inbox(설정 `inbox`) 또는 오늘 일일 기록 끝에 덧붙인다. `capture`는 `- hh:mm <text>`(여러 줄이면 두 번째 줄부터 2칸 들여쓰기). 입력은 `validate_command_text`(64 KiB·NUL). CAS 재시도 3회. 대상이 열려 있고 dirty면 디스크에 쓰지 않고 `rewrite_request`(끝에 붙이는 edit)로 보낸다.
- CLI `devbox note add [--daily] <text>` → `notes.append`(MCP·스크립트용). `[mcp]` 설정과 무관(로컬 CLI).

**핵심 테스트:**
- `summary_block_is_replaced_in_place_and_other_pc_blocks_survive`(위 입력)
- `user_text_outside_markers_is_preserved`: 표시 앞뒤 글·같은 PC 소제목 아래 사용자 글 그대로, CRLF 파일은 CRLF 유지
- `missing_pc_heading_is_appended_once`: 소제목 없음 → 끝에 `## pc-a` + 블록, 두 번 실행해도 한 번
- `summary_lists_closed_agent_tasks_and_commits_of_the_day`(가짜 `SummarySources`): 어제 닫힌 작업 제외, 오늘 PR 링크 포함, 커밋 `abc123 fix x`
- `past_date_summary_uses_that_calendar_day`: 주입 시각 2026-10-21 10:00에 `date = 2026-10-19` → 10-19 23:30에 닫힌 작업 포함, 10-20 00:10·10-18 23:50 작업 제외
- `append_capture_formats_time_and_rejects_nul`: `"a\nb"` → `- 09:05 a\n  b`, NUL → `notes.invalid_input`, 파일 불변
- 데몬 통합 `daily_and_append_round_trip`: `notes.daily` 두 번 → created true/false, `devbox note add --daily hi` → 오늘 파일 끝에 `hi`

---

### Task 7: `crates/search` — 노트 FTS·코드·파일 이름·저장된 검색

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `crates/search/src/{fts_tests,code_tests,files_tests}.rs`.
```rust
// crates/search/src/fts_tests.rs (요점)
let s = fixture().await;
s.index_note("a.md", "제목", "어제 데몬재시작 했다").await?;
let hits = s.notes("재시작", 10).await?;
assert_eq!(hits.items[0].path, "a.md");
assert_eq!(&hits.items[0].snippet[hits.items[0].ranges[0].clone()], "재시작"); // UTF-16 ranges
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test -p devbox-search korean_substring_matches_in_note_fts -- --test-threads=2`.
- [ ] **3 · 구현:** `fts.rs`(index.db 도메인 `search`, 한글 2-gram 보조 열), `code.rs`(`ignore::WalkBuilder` + `grep_searcher::Searcher` + `grep_regex::RegexMatcher`, 취소 토큰), `files.rs`(프로젝트 `git ls-files -z` 캐시 30초 + vault 경로, 퍼지 점수), `saved.rs`(devbox.db 마이그레이션 `search`), `service.rs`·스트림. CLI 조립에서 notes의 `NoteSink`를 search에 연결하고 `notes` 시작 스캔 후 search 도메인이 다시 만들어졌으면(`ensure_domain`이 true) 전체 노트를 다시 넣는다.

```rust
// crates/search/src/fts.rs (요점)
pub const SEARCH_SCHEMA: &str = "CREATE VIRTUAL TABLE search_notes USING fts5(path UNINDEXED, title, body, grams, tokenize='unicode61');";
pub fn grams(text: &str) -> String {      // adjacent Hangul syllable bigrams: 데몬재시작 → 데몬 몬재 재시 시작
    hangul_runs(text).flat_map(|run| run.windows(2).map(|w| w.iter().collect::<String>())).collect::<Vec<_>>().join(" ")
}
// query "재시작" → grams : "\"재시 시작\"" (phrase = substring); ASCII terms → body/title prefix match.
```

- [ ] **4 · 통과:** 같은 RED 명령, `$NARROW cargo test -p devbox-search -- --test-threads=2`, 데몬 통합 `$NARROW cargo test -p devbox-cli --test search -- --test-threads=2`, `gen-ts --check`.
- [ ] **5 · 검토·커밋:** 별도 검토(다른 도메인 테이블 불변, R8, 1 MiB·이진 건너뜀, 취소) · Biome 뒤 `feat(search): search notes, code and file names`. **묶음 U 끝:** PROGRESS 갱신·로컬 커밋.

**Files:**
- Create: `crates/search/{Cargo.toml,src/lib.rs,src/model.rs,src/settings.rs,src/fts.rs,src/code.rs,src/files.rs,src/saved.rs,src/service.rs}`
- Test: `crates/search/src/{fts_tests,code_tests,files_tests}.rs`, `crates/cli/tests/search.rs`
- Modify(줄 추가): workspace `Cargo.toml`(`grep-searcher`, `grep-regex`, `ignore`), `crates/cli/src/daemon/{knowledge,knowledge_routes,mod,state,routes}.rs`, `xtask`, `messages.ts`; 생성 rpc.ts·lock

**Interfaces → Produces:**
- 설정 `[search]`(`SearchSettings`): `exclude = ["**/dist/**"]`(glob, `ignore::overrides`). 
- index.db 도메인 `search` 버전 1(`SEARCH_SCHEMA`). 버전이 바뀌면 `search_*`만 다시 만들고 다시 색인한다(백업 없음). 다른 도메인(`notes_*`, S5의 `api_*`)은 건드리지 않는다.
- `search.query{ text, scopes: ("notes"|"code"|"files")[], projectIds: string[]|null, regex: bool, caseSensitive: bool, limit /*1..500*/ }`(stream) → `StreamOpened`. 스트림 본문은 JSON Lines `SearchChunk { scope, rev, items, done: bool, truncated: bool }`:
  - 노트: FTS5(`title`·`body` unicode61 + `grams` 한글 2-gram), `snippet()`의 표시 문자를 `ranges_utf16`으로 바꿔 보낸다. 노트 먼저 한 묶음.
  - 코드: 등록 프로젝트(또는 `projectIds`)를 `ignore`(gitignore·사용자 제외 존중, `is_internal` 제외)로 걷고 `grep-searcher`로 찾는다. 파일 1 MiB 초과·이진은 건너뛴다. 줄 조각 512자, 결과는 50건씩 묶음. `tx.closed()`가 취소되면 걷기를 멈춘다.
  - 파일 이름: 프로젝트 `git ls-files -z` 캐시 + vault 경로(notes sink가 넣은 path), 부분 일치·단어 시작 가산 퍼지 점수.
  - 각 묶음의 `rev`는 search 메모리 카운터다.
- `search.notes{ text, limit /*≤50*/ }`(query) → `{ rev, items: [{ path, title, snippet, rangesUtf16 }] }`(MCP·빠른 열기용, 노트만).
- 주제 `search.changed { rev }`: 색인 내용이 실제로 바뀐 경우만(해시가 같은 다시 색인·관찰 시각만 바뀐 갱신은 rev를 올리지 않음, 01 §5.5 R8).
- `search.saved{}`(query) → `{ rev, items: [{ id, name, query }] }`, `search.save{ name, query }`(mutation) → `{ id, rev }`, `search.unsave{ id }`(mutation(reversible)) → `Done`. devbox.db `search_saved(id TEXT PRIMARY KEY, name TEXT NOT NULL, query TEXT NOT NULL /*JSON*/, created_ms INTEGER NOT NULL)`.
- 오류 `SearchError`: `InvalidRegex { position }`, `InvalidInput`, `ProjectNotFound`, `NotFound`.
- 같은 crate를 S4 편집기의 프로젝트 검색도 쓴다(`SearchService::code(..)` 공개).

**핵심 테스트:**
- `korean_substring_matches_in_note_fts`: "데몬재시작" 안의 "재시작" → 1건, 범위가 정확히 "재시작"
- `code_search_respects_gitignore_and_user_excludes`: `.gitignore`의 `build/`, `exclude = ["**/dist/**"]` → 두 폴더의 일치 0, `src/` 일치 1
- `search_index_rebuilds_only_its_domain_when_version_changes`: `api_y`·`notes_x`에 행 → search 버전 2로 → `search_*`만 비고 `api_y`·`notes_x` 그대로
- `cancel_stops_the_code_walk`: 파일 20,000개 fixture에서 첫 묶음 뒤 스트림 닫기 → 걷기 중단(방문 파일 수 < 전체), 1초 안에 작업 종료
- `observed_time_only_changes_do_not_bump_the_rev`: 같은 내용으로 `touch`(감시기 관찰 시각만 바뀜) 3회 → 결과 묶음 rev 그대로, `search.changed` 0회(01 §5.5 R8, CV-F07·AR-F11)
- `blank_title_note_is_found_by_file_name`: 빈 title의 `idea.md`를 이름 검색 `idea` → 1건(AR-F10)
- `trash_and_temp_files_are_not_searched`: `.trash/`·`.devbox-tmp-*` 안의 일치는 노트·코드·파일 이름 결과에 없음(CV-F06)
- `invalid_regex_is_rejected_with_position`: `"(a"` → `search.invalid_regex{position}`
- 데몬 통합 `query_stream_sends_notes_then_code`: 노트 묶음이 코드 묶음보다 먼저, 마지막 `done: true`

---

### Task 8: `crates/activity` — 이식·묶어 쓰기·규칙·저장 실패

**단계별 실행:**
- [ ] **1 · 실패 테스트:** 이식 시험 25개를 `crates/activity/src/model/*.rs`로 그대로 옮기고, `crates/activity/src/{service_tests,store_tests}.rs`에 아래와 핵심 테스트를 쓴다.
```rust
// crates/activity/src/service_tests.rs (요점, AU-K2 through the daemon path)
let a = fixture().await;
for (at, title) in [(900_000, "first"), (1_000_000, "first"), (1_100_000, "tail1"), (1_200_000, "tail2"), (1_298_000, "tail2")] {
    a.ingest(fg("owned", title, at, if at <= 1_000_000 { at } else { 1_000_000 })).await?;
}
a.ingest(idle(1_300_000, 1_000_000)).await?;
a.flush().await?;
let rows = a.sessions().await?;
assert_eq!((rows[0].start_ms, rows[0].end_ms), (900_000, 1_000_000));
assert_eq!(rows.len(), 1);
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test -p devbox-activity ingest_with_idle_tail_stores_end_at_last_input -- --test-threads=2`. 이식 시험은 이식 직후 GREEN이 정상이며 RED로 세지 않는다.
- [ ] **3 · 구현:** `model/`(이식, `default-features = false`로 빌드되는 부분), `rules.rs`(config `[activity]` 읽기·`toml_edit` 쓰기), `store.rs`(devbox.db), `writer.rs`(1초/100건 묶음, 재시도 큐 10,000), `service.rs`, `export.rs`.

```rust
// crates/activity/src/writer.rs (요점)
pub async fn flush(&self) {
    let batch: Vec<Session> = self.queue.lock().drain_upto(100);
    match self.sink.insert(&batch).await {               // INSERT OR IGNORE by session id
        Ok(rev) => { self.status.ok(rev); }
        Err(e) => { self.queue.lock().requeue_front(batch, MAX_PENDING /*10_000, drops oldest, counts*/);
                    self.status.failed(code_of(&e)); }   // activity.status_changed; message stays in logs
    }
}
```

- [ ] **4 · 통과:** 같은 RED 명령, `$NARROW cargo test -p devbox-activity -- --test-threads=2`, `$NARROW cargo test -p devbox-activity --no-default-features model -- --test-threads=2`(core 없이 빌드), 데몬 통합 `$NARROW cargo test -p devbox-cli --test activity -- --test-threads=2`(`ClientKind::App`만 ingest), `gen-ts --check`.
- [ ] **5 · 검토·커밋:** 별도 검토(이식 동작 불변, fail-closed, 큐 상한, 로그에 제목 없음) · Biome 뒤 `feat(activity): port sessionizing and privacy rules with durable batching`.

**Files:**
- Create: `crates/activity/{Cargo.toml,src/lib.rs,src/model/mod.rs,src/model/{sessionizer,privacy,idle,attribution,event}.rs,src/rules.rs,src/store.rs,src/writer.rs,src/service.rs,src/export.rs}`
- Test: `crates/activity/src/{service_tests,store_tests}.rs`, `crates/cli/tests/activity.rs`
- Modify: workspace `Cargo.toml`, `crates/cli/src/daemon/{knowledge,knowledge_routes,mod,state,routes,server}.rs`(server는 ingest 허용 1분기), `crates/secrets/{Cargo.toml,src/service.rs}`(`save_name` 설정 잠금·rustix 의존), `README.md`(잠금 파일 안내 한 줄), `xtask`, `messages.ts`; 생성 rpc.ts·lock

**Interfaces → Produces:**
- `Cargo.toml` 기능: `[features] default = ["daemon"]`, `daemon = ["dep:devbox-core", "dep:rusqlite", "dep:tokio", "dep:toml_edit", "dep:chrono"]`. `model`·DTO·`method!` 선언은 기능과 무관(의존: `devbox-protocol`, `serde`, `serde_json`, `ts-rs`, `regex`, `thiserror`).
- `model/*`: v0.9.0 이식(위 이식 표). AU-K2 수정은 v0.9.0에 이미 있다. `idle::duration_from_input_ticks(now: u64, last_input: u32) -> i64`는 조건 없이 공개.
- `model::event::Observation { seq: u64, atMs: i64, kind: "foreground"|"input"|"idle"|"pause", app: string|null, title: string|null, lastInputMs: i64 }`(ts camelCase, `seq`는 네이티브가 `runId`마다 1부터 붙이는 관찰 순번). Windows 앱이 만든다(Task 9).
- `activity.ingest{ batch: { runId, upto } | null, events: Observation[] /*≤1000*/, dropped: u32 }`(mutation, **App 클라이언트만**) → `{ accepted }`. `runId`별 마지막 `upto`를 메모리에 두고 그 이하 묶음은 `accepted: 0`으로 무시한다
  - 서버에서 규칙을 다시 적용한다(앱 캐시가 오래됐을 때 대비. 규칙 컴파일 실패면 `CompiledRules::fail_closed()`).
  - `foreground` → `sessionizer.observe_with_input(app, title, at, lastInput)`, `input` → 같은 앱·제목으로 `observe_with_input`(열린 세션 연장·대기 구간 확정), `idle` → `close_at_last_input(lastInput)`, `pause` → `finish_all(at)`.
  - 확정 세션은 프로젝트 귀속(`attribution::attribute_title`, 등록 프로젝트 basename) 뒤 쓰기 큐로. 1초 또는 100건 단위로 묶어 쓴다(01 §4.4).
  - `dropped`(앱 오프라인 버퍼에서 버린 수)는 상태에 더한다.
- 저장 실패(디스크 부족·DB 오류): 확정 세션은 메모리 재시도 큐(상한 10,000건, 넘치면 가장 오래된 것부터 버리고 수를 셈). 세션 ID는 **결정적**으로 `sha256(runId + "\0" + 세션을 연 관찰의 seq + "\0" + start_ms)` 앞 32자(앱 이외 경로·`runId` 없음은 `uuid`)이고 `INSERT OR IGNORE`해 재시도 중복 0. 메모리의 `runId`별 마지막 `upto`가 데몬 재시작으로 사라져 같은 묶음이 다시 와도 같은 세션 ID가 나와 중복 저장이 없다. 알려진 한계: sessionizer의 열린 세션은 메모리에만 있으므로, 데몬 재시작 경계에서는 열린 세션의 앞부분을 잃거나(이미 ack된 관찰) 재시작 뒤 관찰부터 새 세션으로 나뉠 수 있다. 세션 ID에 `start_ms`가 들어가 나뉜 두 행이 서로를 덮거나 같은 구간을 두 번 세지는 않는다. 합치거나 복원하지 않는다. 다음 쓰기가 성공하면 큐를 순서대로 비운다(CV-F08, AR-F05). 수집 중지(`pause`)는 저장 실패와 관계없이 바로 따른다.
- `activity.status{}`(query) → `ActivityStatus { rev, state: "ok"|"storage_failed", pending, dropped, lastError: "disk_full"|"database"|null }`, 주제 `activity.status_changed`(같은 payload). 오류 원문은 로그에만.
- `activity.rules{}`(query) → `{ rev, rules: PrivacyRules, idleThresholdMs }`, `activity.set_rules{ rules, idleThresholdMs }`(mutation) → `{ rev }` | 오류 `activity.invalid_rules { problems: InvalidRule[] }`(하나라도 틀리면 저장 거부, 위치 반환). 저장은 `config.toml [activity]`(`toml_edit`, 주석 보존)이고 메모리 컴파일본을 바꾼 뒤 `activity.changed`. 쓰기 방식은 기존 `devbox_secrets` `SecretsService::save_name`(`crates/secrets/src/service.rs` 359행)과 같게 한다: `paths.config_dir/config.toml`이 symlink·비일반 파일이면 거절, 없으면 빈 문서에서 시작. 그 함수는 지금 잠금 없이 read-modify-write를 하므로 두 쓰기가 겹치면 한쪽 변경이 사라질 수 있다. 그래서 둘 다 `<config_dir>/.config.toml.lock`에 `rustix::fs::flock(LockExclusive)`를 잡고 읽기 → 고치기 → 같은 폴더 임시 파일 fsync → rename 한다(`crates/secrets/src/service.rs`에 잠금 몇 줄 추가. `crates/secrets`는 동결 대상이 아니지만 공유 성격이므로 S5 변경 여부를 PROGRESS §2에 적고 진행). 잠금 도우미는 각 crate에 10줄 남짓으로 둔다(core 동결). `crates/secrets/Cargo.toml`·`crates/activity/Cargo.toml`에 `rustix = { workspace = true }`(fs)를 줄 추가한다. 잠금은 async 문맥을 막지 않도록 읽기·쓰기·rename 전체를 `tokio::task::spawn_blocking` 안에서 한다(`save_name`은 지금 동기 함수이므로 호출부를 `spawn_blocking`으로 감싸는 1–2줄). 잠금 파일 `.config.toml.lock`은 devbox 설정 폴더(`~/.config/devbox`, accept는 `~/.config/devbox-accept`) 안의 빈 소유 파일이며 지우지 않는다. 사용자가 그 폴더를 dotfiles로 Git 관리하면 ignore하라는 한 줄을 Task 8에서 `README.md`(설정 파일을 설명하는 곳, 없으면 `## 개발` 아래)에 더한다.
- `activity.day{ date }`(query) → `{ rev, date, totalMs, apps: [{ app, ms }], projects: [{ projectId, ms }], titles: [{ app, title, ms }] /*상위 50*/, hours: [{ hour, ms }] /*24칸*/ }`, `activity.range{ from, to /*≤31일*/ }`(query) → `{ rev, days: [{ date, totalMs }], apps, projects }`
- `activity.export{ from, to, format: "csv"|"json" }`(mutation) → `{ path }`(`<data>/exports/activity-<from>-<to>.<ext>`, 열 `start,end,app,title,project`)
- 주제 `activity.changed { rev }`.
- devbox.db 마이그레이션 도메인 `activity` v1: `activity_sessions(id TEXT PRIMARY KEY, app TEXT NOT NULL, title TEXT NOT NULL, project_id TEXT, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL)`, `CREATE INDEX activity_sessions_start ON activity_sessions(start_ms)`.
- 저장 경계 주입: `trait SessionSink { async fn insert(&self, batch: &[Session]) -> Result<u64 /*rev*/, SinkError> }`, 실제 구현은 `Db::write`.
- 오류 `ActivityError`: `InvalidRules { problems }`, `InvalidRange`, `InvalidInput`.

**핵심 테스트:**
- 이식 25개(위 표) — 특히 `idle_retroactively_excludes_all_last_input_tail_intervals`, `explicit_pause_keeps_actual_pause_time_and_wall_clock_reversal_never_makes_negative_spans`, `input_ticks_share_the_low_32_bit_domain_across_uptime_wrap`
- `ingest_with_idle_tail_stores_end_at_last_input`(위 입력, AU-K2 데몬 경로): 저장된 세션 1개 `(900_000, 1_000_000)`, 298초 꼬리 0
- `storage_failure_keeps_later_sessions_and_reports_status`: 3세션, 두 번째 INSERT 실패 주입 → `activity.status_changed{state:"storage_failed", pending:3}` → 회복 뒤 3건 모두 저장, 중복 0, `state:"ok"`(CV-F08·AR-F05)
- `retry_queue_caps_at_10000_and_counts_drops`: 10,005건 대기 → pending 10,000, dropped 5(오래된 것부터)
- `invalid_rule_regex_is_rejected_with_position`: `excludedTitlePatterns = ["ok", "(unclosed"]` → `activity.invalid_rules` `{field:"excludedTitlePatterns", index:1, problem:"syntax"}`, config 파일 바이트 불변
- `rules_write_waits_for_the_config_lock_and_keeps_other_sections`: 시험이 `.config.toml.lock`을 잡은 채 `set_rules` 시작(대기) → 시험이 `[secrets]\nnames = ["A"]`를 덧붙이고 잠금 해제 → 결과 파일에 `[secrets]`·`[activity]` 둘 다, 주석 보존. symlink `config.toml` → 거절·대상 불변
- `crates/secrets` `save_name_holds_the_config_lock`(기존 `store_tests.rs` 방식): 잠금을 잡은 동안 `save_name`이 끝나지 않고, 해제 뒤 이전에 쓴 `[activity]` 표가 보존됨(`$NARROW cargo test -p devbox-secrets save_name -- --test-threads=2`)
- `ingest_reapplies_rules_server_side`: 규칙 `redactTitlePatterns=["secret"]` → 앱이 `secret doc`을 그대로 보냄 → 저장 제목 `[redacted] doc`
- `broken_rules_store_no_titles`: `[activity]`의 규칙을 컴파일 실패하게 손으로 씀 → 서비스 시작 → 저장 제목 빈 문자열(fail-closed)
- 데몬 통합 `ingest_is_app_only`: `ClientKind::Cli`·`Mcp`로 `activity.ingest` → `method_not_found`, `App` → 성공

---

### Task 9: Windows 활동 수집기 (`crates/win`, `app/src-tauri`)

**단계별 실행:**
- [ ] **1 · 실패 테스트:** 플랫폼 독립 부분을 먼저 쓴다: `crates/win/src/activity_buffer.rs`의 `#[cfg(test)]`, `app/src-tauri/src/core/activity.rs`의 `#[cfg(test)]`.
```rust
// app/src-tauri/src/core/activity.rs (요점)
let mut c = Collector::default();                 // consent=false, rules=None
assert!(c.accept(obs("code.exe", "secret", 1_000)).is_none());   // no consent: nothing queued
c.set_consent(true);
c.accept(obs("code.exe", "secret", 2_000));
assert_eq!(c.drain(10).events[0].title, None);    // rules missing → fail-closed
```
- [ ] **2 · 실패 확인:** `$NARROW cargo test --manifest-path app/src-tauri/Cargo.toml missing_or_broken_rules_drop_titles -- --test-threads=2`, `$NARROW cargo test -p devbox-win offline_buffer_caps_at_1000_and_counts_drops -- --test-threads=2`.
- [ ] **3 · 구현:** `crates/win/src/activity.rs`(`#[cfg(windows)]` 훅 스레드), `activity_buffer.rs`(독립), 앱 `core/activity.rs`(동의·일시중지·규칙·버퍼, 독립), `platform/activity.rs`(수집기 시작·Tauri 명령·트레이 연결), `core/settings.rs`(동의·일시중지 저장), 화면 `features/activity/{uplink.ts,Consent.tsx,useActivityStatus.ts}`. `crates/win/Cargo.toml`에 `devbox-activity = { path = "../activity", default-features = false }`, `app/src-tauri/Cargo.toml`에 `devbox-activity = { path = "../../crates/activity", default-features = false }`(둘 다 **직접 path 의존**. `{ workspace = true, default-features = false }`는 workspace 쪽 항목이 기본 기능을 켜 두면 무시되므로 쓰지 않는다. workspace 빌드에서는 cli가 `daemon`을 켜 기능이 합쳐지지만, Windows 앱은 별도 workspace라 `model`만 빌드된다)와 windows feature(`Win32_UI_Accessibility`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_System_Threading`, `Win32_System_SystemInformation`)를 줄 추가하고 `cargo metadata`로 두 lock을 만든다.

```rust
// crates/win/src/activity.rs (요점, cfg(windows))
pub fn start(sink: impl Fn(Observation) + Send + 'static) -> Collector {
    // dedicated thread: SetWinEventHook(EVENT_SYSTEM_FOREGROUND) + EVENT_OBJECT_NAMECHANGE (foreground hwnd only),
    // GetMessageW loop; 5s timer: GetLastInputInfo + GetTickCount64 →
    // last_input_ms = now_ms - devbox_activity::model::idle::duration_from_input_ticks(tick64, lii.dwTime)
}
```

- [ ] **4 · 통과:** 같은 RED 명령, `$NARROW cargo test -p devbox-win -- --test-threads=2`, `$NARROW cargo test --manifest-path app/src-tauri/Cargo.toml activity -- --test-threads=2`, 화면 `$NARROW pnpm --filter app exec vitest run src/features/activity/uplink.test.ts --maxWorkers=2`, MSVC 좁은 compile(위 "검증 명령"의 무잠금 명령). Windows 훅 동작은 Task 12 설치본 검증이며 여기서 PASS로 적지 않는다.
- [ ] **5 · 검토·커밋:** 별도 검토(동의 전 훅 0, 규칙 없을 때 제목 0, 제목이 규칙 적용 전 JS로 가지 않음, unsafe 범위) · Biome 뒤 `feat(windows): collect foreground activity with fail-closed privacy`. **묶음 V 끝:** PROGRESS 갱신·로컬 커밋.

**Files:**
- Create: `crates/win/src/{activity.rs,activity_buffer.rs}`, Modify: `crates/win/{Cargo.toml,src/lib.rs}`
- Create: `app/src-tauri/src/core/activity.rs`, `app/src-tauri/src/platform/activity.rs`
- Modify: `app/src-tauri/{Cargo.toml,src/lib.rs,src/core/mod.rs,src/platform/mod.rs,src/core/settings.rs,src/platform/tray.rs}`; 생성 `app/src-tauri/Cargo.lock`, `Cargo.lock`
- Create: `app/src/features/activity/{uplink.ts,useActivityStatus.ts,Consent.tsx}`, Test: `app/src/features/activity/{uplink.test.ts,Consent.test.tsx}`

**Interfaces → Produces:**
- `crates/win::activity::start(sink) -> CollectorHandle`(`#[cfg(windows)]`): 전면 창 변경·전면 창 제목 변경 시 `Observation{kind:"foreground"}`, 5초마다 입력 시각 확인 → idle 임계(규칙의 `idleThresholdMs`, 기본 5분) 넘으면 `idle` 1회, 입력이 다시 오면 `foreground`(현재 창), 입력 시각이 30초 이상 전진했으면 `input` 하트비트. 같은 앱·제목이 이어지면 `foreground`를 만들지 않는다. `CollectorHandle::stop()`은 훅 해제·스레드 종료.
- 앱 이름: `GetWindowThreadProcessId` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW`의 파일 이름(`code.exe`). 실패하면 `unknown`.
- `crates/win::activity_buffer::OfflineBuffer`(독립): 상한 1,000건, 넘치면 오래된 것부터 버리고 `dropped`를 센다. `drain(max) -> Batch{ events, dropped, upto }`, `ack(upto)`.
- 앱 `core/activity.rs::Collector`(독립): `consent`, `paused`, `rules: Option<CompiledRules>`(None = 못 받음·컴파일 실패). `accept(obs)` = 동의 없음·일시중지면 버림 → `privacy::apply`(None이면 제목 지움, 제외 프로세스면 버림) → 버퍼. `set_consent(false)`는 대기열을 비운다. `set_paused(true)`는 `pause` 관찰 1개를 넣고 이후 버린다.
- Tauri 명령(`platform/activity.rs`, `lib.rs`에 등록 줄 추가): `activity_state() -> { consent, paused, legacyKnowledge: bool }`, `activity_set({ consent?, paused? })`, `activity_configure({ rules: PrivacyRules|null })`(앱이 `activity.rules`를 받아 넘김. 컴파일은 네이티브), `activity_drain(max) -> { events, dropped, upto }`, `activity_ack(upto)`, `activity_tray({ storageFailed, pending })`. 이벤트 `activity://state`(트레이에서 바뀌었을 때).
- 화면 `uplink.ts`: **메인 창(`getCurrentWindow().label === "main"`)에서만** 돈다(숨긴 캡처 창·분리 창이 같은 버퍼를 중복 drain하지 않게). 연결된 동안 1초마다 `activity_drain(1000)` → `client.call("activity.ingest", { batch, events, dropped }, { clientRequestId: `activity-${runId}-${upto}` })` 성공 뒤 `activity_ack(upto)`. `runId`는 네이티브가 앱 시작마다 만드는 값이고 `batch = { runId, upto }`다. 응답이 유실돼 같은 묶음을 다시 보내면 같은 `clientRequestId`라 데몬 dedupe가 같은 결과를 돌려주고, dedupe 창이 지났어도 서버가 `runId`별 마지막 `upto` 이하 묶음을 무시해 중복 세션이 생기지 않는다(세션 ID `INSERT OR IGNORE`와 이중 방어). 실패하면 ack하지 않음(네이티브 버퍼가 1,000건까지 보관). 재연결 뒤 `activity.rules` → `activity_configure`. 브라우저(dev-gateway)에서는 아무것도 하지 않는다.
- 동의 전에는 `start`를 부르지 않는다. 동의를 철회하면 `stop` + 대기열 비움. 일시중지 중에는 보내지 않는다(트레이 메뉴·설정).
- v0.9.0 공존: `core/activity.rs::legacy_knowledge(local_app_data: &Path) -> bool`(독립 함수)이 `%LOCALAPPDATA%` 바로 아래 이름이 `com.devbox.v08.knowledge.i`로 시작하는 **일반 폴더**(symlink·junction 제외)를 훑어 그 안에 일반 파일 `active-stores.json`이 하나라도 있으면 `legacyKnowledge: true`(v0.9.0은 설치마다 `i{suffix}`가 붙는다. 접미사 없는 `com.devbox.v08.knowledge`는 v0.9.0이 만들지 않으므로 보지 않음) → 동의 화면에 "v0.9.0 Knowledge의 활동 기록이 켜져 있다면 한쪽만 켜 두세요"(01 §6.2. 읽기 전용 존재 확인만).
- 트레이(`platform/tray.rs`): 메뉴 "활동 기록 일시중지"/"활동 기록 다시 시작"(상태에 따라 글자 교체), `activity_tray` 결과로 "활동 저장 중단 · n건 대기" 항목 표시(01 §9.1-6).
- 설정(`core/settings.rs::Settings`): `activity: { consent: bool, paused: bool }`(기본 false·false, `#[serde(default)]`).

**핵심 테스트:**
- `missing_or_broken_rules_drop_titles`(앱 core, Linux): 규칙 None → 제목 None, 앱 이름·시간 유지. 컴파일 실패 규칙으로 `configure` → None과 같음
- `offline_buffer_caps_at_1000_and_counts_drops`(crates/win 독립): 1,003건 → 1,000건, dropped 3, `drain`·`ack` 뒤 남은 수
- `consent_withdrawal_clears_the_queue_and_pause_emits_one_pause`
- `excluded_process_never_reaches_the_buffer`
- 이식 시험 `input_ticks_share_the_low_32_bit_domain_across_uptime_wrap`은 Task 8에서 한 번만 센다(Task 9는 같은 함수를 부름)
- 화면 `uplink.test.ts`: ingest 실패 → `activity_ack` 미호출, 다음 성공 뒤 같은 `upto` ack. 응답 유실(첫 호출이 `connection_lost`) 뒤 재전송은 같은 `clientRequestId`. 브라우저 모드·캡처 창 label에서 drain 0회
- 데몬 통합(Task 8 `crates/cli/tests/activity.rs`에 추가) `replayed_batch_is_ignored`: 같은 `{runId, upto}` 묶음을 다른 `clientRequestId`로 두 번 → 저장 세션 수 같음. 첫 묶음 뒤 `TestDaemon::restart` → 같은 묶음 재전송 → 세션 수·ID 같음
- 앱 core `legacy_knowledge_detects_any_suffixed_store`(Linux, 임시 폴더 fixture): `com.devbox.v08.knowledge.iabc/active-stores.json` → true. `com.devbox.v08.knowledge.iabc`(파일 없음)·`com.devbox.v08.knowledge/active-stores.json`(접미사 없음)·`com.devbox.v08.knowledge.ixyz/active-stores.json`이 폴더인 경우 → false. 두 접미사 중 하나만 있어도 true. symlink 폴더는 따라가지 않음
- 화면 `Consent.test.tsx`: 동의 전 수집 버튼만, `legacyKnowledge`면 안내 문구
- Task 8 `activity.ingest` 입력에 `batch: { runId, upto } | null`을 더한다(Task 8 Interfaces와 같은 커밋에서 정하면 Task 9에서 바꾸지 않아도 된다)
- Windows 훅 동작은 Task 12 에이전트 설치본 검증

---

### Task 10: 노트 섹션 화면

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `app/src/features/notes/{wikiComplete,pasteImage,NotesPage,saveLabel}.test.ts(x)`와 E2E `app/e2e/notes.spec.ts`(작성만, 실행은 Task 12).
```ts
// app/src/features/notes/wikiComplete.test.ts (요점)
const items = rank("da", [{ path: "daily/2026-10-21.md", title: "" }, { path: "data.md", title: "Data" }, { path: "x/adam.md", title: "Adam" }]);
expect(items.map((i) => i.label)).toEqual(["data", "2026-10-21", "adam"]); // title/stem prefix, then folder, then substring
```
- [ ] **2 · 실패 확인:** `$NARROW pnpm --filter app exec vitest run src/features/notes/wikiComplete.test.ts --maxWorkers=2`.
- [ ] **3 · 구현:** 페이지·목록(트리·태그·활동 탭 자리)·편집기(`EditorHost` + Markdown + `[[` 자동완성 + 이미지 붙여넣기)·미리보기(mermaid strict)·백링크·비교·복구·읽기 전용 가져오기·저장 상태·단축키. `Shell.tsx`에 `/notes` 페이지·`/popout/note/$key` 분리 창·`<NotesCommands />` 한 줄씩, `popout.ts`의 `PopoutKind`에 `"note"`. `app/package.json`에 `mermaid` 줄 추가 + `pnpm install`. `docs/ui-terms.md`에 노트·일일 기록·빠른 캡처·활동 기록·휴지통·백링크 용어 추가.

```tsx
// app/src/features/notes/Preview.tsx (요점)
const { data } = useRpcQuery("notes.render", { path: null, text: debouncedText });
useEffect(() => {
  void import("mermaid").then(({ default: m }) => {
    m.initialize({ startOnLoad: false, securityLevel: "strict" });
    host.current?.querySelectorAll<HTMLElement>(".mermaid-block").forEach((el) => {
      void m.render(`mm-${el.dataset.idx}`, data!.mermaid[Number(el.dataset.idx)]).then(({ svg }) => { el.innerHTML = svg; });
    });
  });
}, [data]);
```

- [ ] **4 · 통과:** 같은 RED 명령과 이 과제 시험 파일들, `$NARROW pnpm --filter app typecheck`(`app/tsconfig.json`의 `include`에 `e2e`가 있어 `notes.spec.ts` 정적 타입도 함께 확인), 컴포넌트 300줄 확인.
- [ ] **5 · 검토·커밋:** 별도 검토(문구 ui-terms, 저장 상태 4가지, 원격 이미지 0) · Biome 뒤 `feat(app): add the notes section`.

**Files:**
- Create: `app/src/features/notes/{NotesPage.tsx,NotesList.tsx,NoteTree.tsx,TagList.tsx,NoteEditor.tsx,Preview.tsx,Backlinks.tsx,CompareView.tsx,RecoveryView.tsx,SaveStatus.tsx,ReadOnlyBar.tsx,NotesCommands.tsx,PopoutNote.tsx,wikiComplete.ts,pasteImage.ts,saveLabel.ts,notes.module.css}`
- Test: `app/src/features/notes/{wikiComplete.test.ts,pasteImage.test.ts,saveLabel.test.ts,NotesPage.test.tsx,CompareView.test.tsx}`, `app/e2e/notes.spec.ts`
- Modify(줄 추가): `app/src/shell/Shell.tsx`, `app/src/features/terminal/popout.ts`(`PopoutKind`), `app/package.json`, `docs/ui-terms.md`; 생성 `pnpm-lock.yaml`

**Interfaces → Produces:**
- 목록(`SectionLayout.list`): 폴더 트리(`NoteTree`, `VirtualList` 기반, 끌어서 옮기기 = `notes.rename{rewriteLinks:true}` → 진행 상태 토스트) · 태그 · 활동(일·주, Task 11이 채움)
- 본문:
  - `NoteEditor`(Task 3 `EditorHost` + `@codemirror/lang-markdown` + `[[` 자동완성 `wikiComplete`(후보 = `notes.tree` 제목·파일 이름, 정렬: 제목/stem 접두 → 같은 폴더 → 부분 일치) + 이미지 붙여넣기 `pasteImage` → `notes.attach` → 커서에 `![이름](attachments/…)` 삽입)
  - [미리보기 분할] 토글(`SplitPane`), 오른쪽 백링크 패널(접기)
  - `notes.rewrite_request` 처리: 이 창이 lease를 가진 문서면 버퍼에 edits 적용 → 저장 → `notes.rewrite_reply`
- [오늘 한 일 넣기](`InsertSummary`, Task 12 수용 5의 화면을 Task 10에 배정): `notes.insert_summary{date:null}` → 반환된 일일 기록 경로 열기. 요청 중 중복 클릭 차단·진행 표시, 실패 시 기존 노트 유지·오류 표시. 열린 문서의 수정은 기존 daemon rewrite 흐름을 사용하며 화면에서 본문을 덮어쓰지 않는다.
- Mermaid는 strict·HTML labels 꺼짐과 렌더 전 외부 리소스 문법 차단을 함께 적용한다. 감사한 flowchart/graph·sequenceDiagram만 허용하고 다른 종류는 안내 후 표시하지 않는다. 이미지 메타데이터·sequence properties/details/links·HTML/엔티티·Markdown 이미지·CSS·설정 지시문도 차단한다. 반환 SVG는 inert template에서 요소·속성·CSS를 제한해 이미지/외부 참조/탐색을 제거한 뒤 넣는다. 실제 배포 청크 분리는 Task 12에서 검증한다.
- 저장 상태(`saveLabel(doc)`): "저장됨 hh:mm"·"저장 중"·"연결 끊김 · 이 기기에 임시 저장"·"충돌 · 비교 필요"
- `CompareView`: 왼쪽 내 버퍼, 오른쪽 디스크(`documents.read`). [내 것 유지][디스크 것][직접 병합]. `DiffView`(ui) 재사용.
- `RecoveryView`: 저널과 디스크 비교. [복원][버림]
- 다른 창이 편집 중이면 `ReadOnlyBar` + [여기로 가져오기](`documents.take`)
- 단축키(`NotesCommands`): Ctrl+Alt+D(일일 기록 `notes.daily` → 열기, 앱 범위), Ctrl+N(노트 섹션에서 새 노트, 편집기 밖에서만 — keymap 규칙이 자동으로 막음), Ctrl+S(EditorHost CodeMirror keymap)
- 노트 휴지통 이동: `confirmPolicy("notes.delete") === "reversible"` → 10초 되돌리기 토스트(`notes.restore`)

**핵심 테스트:**
- `wikiComplete` 후보 정렬 단위(위)
- `pasteImage`: 붙여 넣은 PNG Blob → `notes.attach{data: base64}` 1회 → 반환 markdown을 커서 위치에 삽입, 2 MiB 초과는 호출 0·오류 표시
- `saveLabel`: 네 상태 문구
- `NotesPage.test.tsx`: 트리 선택 → 편집기 열림, 다른 창 lease → 읽기 전용 + [여기로 가져오기]
- `CompareView.test.tsx`: [디스크 것] → `resolve("disk")` 효과로 `documents.journal_drop` 호출(AR-F13 화면 연결)
- E2E `notes.spec.ts`(3개)
  - 새 노트 → 입력 → 1초 뒤 "저장됨" → 디스크 파일 내용 확인
  - 외부에서 파일 수정(테스트가 직접 씀) → 편집 중이면 비교 화면, 아니면 내용 갱신
  - 노트 이름 바꾸기 → 다른 노트의 `[[링크]]`가 새 이름으로

---

### Task 11: 통합 검색·빠른 캡처·활동 보기·노트에서 에이전트 작업·MCP

**단계별 실행:**
- [ ] **1 · 실패 테스트:** `app/src/features/search/SearchTab.test.tsx`, `app/src/features/activity/ActivityDay.test.tsx`, `app/src/features/capture/capture.test.ts`, `crates/cli/tests/mcp_notes.rs`, 앱 `app/src-tauri/src/core/shortcuts.rs` 시험, E2E `app/e2e/search.spec.ts`(작성만).
```tsx
// app/src/features/activity/ActivityDay.test.tsx (요점, AR-F12)
const day = deferred<Methods["activity.day"]["result"]>();
render(<ActivityView />, { rpc: { "activity.day": () => day.promise, "activity.status": () => ok() }, native: { consent: true, paused: true } });
await user.click(screen.getByRole("button", { name: "다시 시작" }));      // native state → collecting
day.resolve(dayResult());                                                  // late list response arrives afterwards
expect(await screen.findByRole("button", { name: "일시중지" })).toBeInTheDocument();
```
- [ ] **2 · 실패 확인:** `$NARROW pnpm --filter app exec vitest run src/features/activity/ActivityDay.test.tsx --maxWorkers=2`, `$NARROW cargo test -p devbox-cli --test mcp_notes -- --test-threads=2`.
- [ ] **3 · 구현:** 검색 탭(`/search` 경로, `client.openStream("search.query", …)`), 검색 명령(Ctrl+Shift+F, 터미널 포커스면 양보), 빠른 캡처(앱 `platform/capture.rs`가 숨긴 창을 미리 만들고 전역 단축키·트레이에서 show, 화면 `/popout/capture`), 활동 보기·설정 › 활동, 노트 선택 → 에이전트 작업(`useAgentsUi.setCreating(true, { prompt })`), MCP `mcp/notes.rs`, 알림 센터 연결(`notes.rename_incomplete`, `activity.status_changed`).
- [ ] **4 · 통과:** 같은 RED 명령과 이 과제 시험 파일들, `$NARROW cargo test --manifest-path app/src-tauri/Cargo.toml shortcuts -- --test-threads=2`, `$NARROW cargo test -p devbox-cli --test mcp -- --test-threads=2`(기존 MCP 영향), typecheck, MSVC 좁은 compile(위 "검증 명령"의 무잠금 명령).
- [ ] **5 · 검토·커밋:** 별도 검토(CV-F07·AR-F12, IME, MCP 쓰기 기본 꺼짐·vault 밖 거부) · Biome 뒤 `feat(app): add unified search, quick capture, activity views and note tools`. **묶음 W 끝:** PROGRESS 갱신·로컬 커밋.

**Files:**
- Create: `app/src/features/search/{SearchPage.tsx,SearchResults.tsx,SavedSearches.tsx,SearchCommands.tsx,useSearchStream.ts,search.module.css}`, Test: `SearchTab.test.tsx`, `useSearchStream.test.ts`
- Create: `app/src/features/capture/{CaptureWindow.tsx,capture.ts}`, Test: `capture.test.ts`
- Create: `app/src/features/activity/{ActivityView.tsx,ActivityDay.tsx,ActivityWeek.tsx,ActivityBars.tsx,RulesEditor.tsx}`, `app/src/features/settings/ActivitySettings.tsx`, Test: `ActivityDay.test.tsx`, `RulesEditor.test.tsx`, `app/src/features/settings/ActivitySettings.test.tsx`
- Create: `app/src-tauri/src/platform/capture.rs`; Modify: `app/src-tauri/src/{lib.rs,core/shortcuts.rs,core/intents.rs,core/settings.rs,platform/shortcuts.rs,platform/tray.rs,platform/window.rs}`(window는 가속키 끄기 함수를 `pub(crate)`로)
- Create: `crates/cli/src/mcp/notes.rs`; Modify: `crates/cli/src/{mcp.rs,mcp/tools.rs}`; Test: `crates/cli/tests/mcp_notes.rs`
- Modify(새 메서드 `notes.read`): `crates/notes/src/{lib,service}.rs`, `crates/cli/src/daemon/knowledge_routes.rs`, `app/src/rpc/messages.ts`(새 코드가 있으면); 생성 `app/src/rpc/gen/rpc.ts`. 시험 `crates/notes/src/vault_tests.rs::read_rejects_traversal_and_outside_symlinks`
- Modify(줄 추가): `app/src/shell/{Shell,router}.tsx`, `app/src/features/terminal/popout.ts`(`"capture"`), `app/src/features/agents/{ui.ts,NewTaskDialog.tsx}`, `app/src/features/settings/SettingsPage.tsx`(탭 "노트"·"활동"), `app/src/features/notifications/AttentionBridge.tsx`, `app/src/features/settings/ShortcutsSettings.tsx`
- Create: `app/e2e/search.spec.ts`

**Interfaces → Produces:**
- 검색 탭(`/search`, Ctrl+Shift+F. 바인딩 `enabled: () => !terminalFocused()`이라 터미널 포커스면 터미널 검색):
  - 범위 칩(노트·코드·파일 이름), 정규식·대소문자, 결과 그룹, 저장된 검색(`search.saved/save/unsave`)
  - `useSearchStream(query)`: 새 질의는 이전 스트림을 닫고 연다. 갱신(`search.changed{rev}`)은 **표시 중 결과의 rev보다 클 때만** 다시 조회하고, 다시 조회하는 동안 마지막 성공 결과와 선택(경로+줄)을 유지한다(CV-F07).
  - 결과를 누르면 노트는 노트 섹션(`/notes?path=`), 코드는 S4 전까지 외부 편집기(기존 `shell/external.ts`·`observe.resolve_location` 흐름) 또는 읽기 전용 미리보기(`CodeView readOnly`).
- 빠른 캡처:
  - 전역 단축키 Ctrl+Alt+N(개발 빌드 Ctrl+Alt+Shift+N): `ShortcutStatus`에 `capture: String`·`capture_error: Option<String>`, `core::shortcuts::default_capture(debug)`, `GlobalShortcuts.capture: Option<String>`. 팔레트·터미널과 같은 키면 등록하지 않고 오류 표시(기존 충돌 정책). 사용자가 설정 › 단축키에서 바꾼 값(`GlobalShortcuts.capture`)이 기본값보다 우선한다(Task 12 수용 6에서 prod와 겹치지 않는 키를 지정하는 데 쓴다).
  - `AppIntent::Capture`는 쓰지 않는다. 네이티브가 직접 `platform/capture.rs::show(app)`: 시작 때 만든 숨긴 창 `popout-capture-main`(`/popout/capture`, 520×220, 항상 위, 장식 최소, 가속키 끔)을 화면 가운데로 옮겨 show + focus. 창이 없으면 만들고 show. 측정: 단축키 처리 시각 → 화면 `capture-ready` 표시까지 앱 로그에 남긴다(SC2 300ms와 같은 방식).
  - 화면 `CaptureWindow`: Enter로 `notes.append{target:"inbox", format:"capture"}` 후 창 숨김, Shift+Enter 줄바꿈, Esc 숨김(내용이 있으면 `sessionStorage` 초안 보존). IME 조합 중 Enter(`isComposing`·keyCode 229)는 무시. 실패하면 창 유지·오류 표시.
  - 트레이 메뉴 "빠른 캡처"에서도 연다.
- 활동:
  - 노트 목록의 활동 탭(일·주 막대 `ActivityBars` + 앱·프로젝트 표)
  - 설정 › 활동(`ActivitySettings`): 동의·일시중지(네이티브 `activity_state/activity_set`), 규칙 편집기(`activity.rules/set_rules`, 오류 위치를 해당 입력란 아래에), 내보내기(`activity.export`), 저장 상태 배너 "저장 중단 · n건 대기"(`activity.status`)
  - 수집 제어 버튼의 의미는 `useActivityStatus()`(네이티브 상태 + `activity.status` query)만 정한다. `activity.day`·`activity.range`는 별도 query이며 버튼에 쓰지 않는다(AR-F12, 01 §9.1-5).
  - `activity.status_changed`가 `storage_failed`면 알림 센터 `error` 항목 1회 + `activity_tray`.
- 노트에서 에이전트 작업: 편집기 선택 영역(없으면 노트 전체, 64 KiB에서 자르고 표시) → [에이전트 작업 만들기] → `useAgentsUi.setCreating(true, { prompt })` → `NewTaskDialog`가 지시문 칸에 채워 연다(프로젝트 선택·신뢰는 기존 흐름).
- MCP 도구(`mcp/notes.rs`, `tools.rs`에 줄 추가):
  - `devbox_notes_search{ text }`(읽기) → `search.notes{ text, limit: 20 }`
  - `devbox_note_read{ path }`(읽기, vault 안만) → `notes.read{ path }`(query, 새 메서드: vault 상대 경로, `..`·절대 경로·vault 밖 symlink → `notes.out_of_vault`, 2 MiB)
  - `devbox_note_add{ text, daily? }`(쓰기, `[mcp] write = true`일 때만 목록·호출) → `notes.append`
- 알림 센터(`AttentionBridge.tsx`): `notes.rename_incomplete` → `{ kind: "error", title: "링크를 고치지 못한 노트 {count}개", target: { to: "/notes", search: { rename: opId } } }`

**핵심 테스트:**
- `SearchTab.test.tsx` `keeps_results_and_selection_while_refreshing`: 결과 선택 → 같은 rev의 `search.changed` 3회 → `search.query` 추가 호출 0, 선택 유지. 더 큰 rev 1회 → 다시 조회 1회, 조회 중에도 이전 결과 표시(CV-F07)
- `late_day_response_does_not_override_tracking_status`(위 입력, AR-F12. `activity.day`와 `activity.status`는 별도 query)
- `capture.test.ts`: IME 조합 중 Enter 무시, Shift+Enter 줄바꿈, Esc는 초안 보존, Enter 성공 시 `notes.append` 1회
- `RulesEditor.test.tsx`: `activity.invalid_rules{index:1}` → 두 번째 입력란 아래 오류
- `ActivitySettings.test.tsx` `storage_failure_shows_banner_tray_and_one_notice`(CV-F08 화면): `activity.status_changed{state:"storage_failed", pending:3}` 두 번 → 배너 "저장 중단 · 3건 대기", 네이티브 `activity_tray({storageFailed:true, pending:3})` 호출, 알림 센터 `error` 항목 1개(두 번째 이벤트로 늘지 않음) → `{state:"ok"}` → 배너 사라짐·`activity_tray({storageFailed:false})`
- Rust 앱 `capture_shortcut_conflicts_with_palette_are_reported`(core/shortcuts): 팔레트와 같은 키 → `capture_error`, 등록 0
- MCP `crates/cli/tests/mcp_notes.rs`: `devbox_note_read{"../x"}`·vault 밖 symlink → 오류 결과, 바깥 내용 미노출. `write=false`면 `devbox_note_add`가 목록에 없고 직접 호출도 거절
- E2E `search.spec.ts`(2개): 노트 본문 한글 부분 검색(`데몬재시작`에서 `재시작`), 코드 검색 결과 열기(읽기 전용 미리보기)

---

### Task 12: S3 완료

**단계별 실행(통합·수용 과제):**
- [ ] **1 · 수용 기준 고정:** 아래 10개 항목·SC2(캡처)·필수 CI 3개·브랜치 전체 검토·자동 리뷰 회차의 상태표를 `devbox-verification/s3-task12/`에 먼저 만들고 시작 상태를 `미실행`으로 둔다. 제품 미구현·실패를 PASS로 바꾸지 않는다.
- [ ] **2 · S5 통합(먼저 머지된 경우):** S5가 이미 main에 머지됐으면 전체 검사 **전에** `git fetch origin main` 뒤 main 변경을 통합한다(로컬 미푸시 브랜치는 rebase 가능, 등록 파일은 양쪽 보존, `rpc.ts`·lock은 `pnpm gen`·`cargo metadata`·`pnpm install`로 재생성, `Services.index` 중복 줄 정리). 통합 직접 영향 검사(`every_declared_method_is_routed`, `gen-ts --check`, typecheck)만 바로 돌린다(00 §3.6). 아직 머지 전이면 이 단계를 건너뛰고 PR 전에 다시 확인한다.
- [ ] **3 · 최종 검사:** 과제별 GREEN과 통합 뒤 처음으로 `bash scripts/check.sh`를 한 번 실행한다(내부에서 `--wait` 공통 잠금, 최대 60분. S5 트랙이 검사 중이면 기다린다. 다시 감싸지 않는다). E2E(`notes.spec.ts`·`search.spec.ts` 포함 16개 ≤ 40)는 check 안에서 돈다. 실패는 그 범위만 고쳐 다시 돌린다. 통과한 무관한 검사를 반복하지 않는다. 기한 초과 exit 75는 "검사 대기 초과"로 기록하고 재시도한다.
- [ ] **4 · 규모 통계:** `bash scripts/loc.sh 120000` 결과(Rust·TS)를 PR 본문에 쓴다. 넘으면 단순화 후보를 PROGRESS §2에 적어 사용자에게 알리고 계속 진행한다(01-design SC8, 기다리지 않음). `.github`+`scripts` 줄 수·workflow 수는 S3가 바꾸지 않았음을 기록한다. 600/300줄 초과 0을 확인한다.
- [ ] **5 · 브랜치 전체 검토:** 통합 **뒤의** 브랜치 전체를 하위 프로젝트 끝 별도 검토(과제별 검토로 대신하지 않음)하고 결과를 PROGRESS에 남긴다. 이어서 Biome CI → 완료 커밋 `chore(knowledge): record S3 verification and acceptance readiness`.
- [ ] **5b · PR 직전 재확인:** 2단계 뒤 S5가 새로 머지됐으면 같은 방법으로 통합하고, 통합 직접 영향 검사와 바뀐 범위의 최종 검증·브랜치 검토만 다시 한다(00 §3.6).

- [ ] **Step 6: 에이전트 설치본 검증(10개)** — 최종 SHA를 push하고 `gh workflow run windows.yml --ref v1/s3-knowledge`로 그 SHA의 산출물을 만든 뒤, **외부 supervisor `python3 scripts/verify-resources.py --wait --lock windows-acceptance -- <수용 driver>`가 다운로드→설치→검증→제거·정리 전체를 감싸고**, 그 안에서 `bash scripts/dogfood.sh v1/s3-knowledge --channel accept`로 설치 파일을 받는다(S2 Task 15b·15c 계약: dogfood 기본 호출은 잠금을 다시 잡지 않음).
  수용 driver는 저장소 스크립트가 아니다. S2 Task 16도 커밋하지 않은 외부 수용 helper를 검증 근거 폴더에서 썼다(PROGRESS §5 "S2 Task16": `windows-acceptance` 잠금 안 실행, pure fixture 시험·독립 재검토 뒤 사용). S3도 `scripts`가 동결이므로 같은 방식으로 `devbox-verification/s3-task12/acceptance.py`(10개 항목·소유 fixture·정리·PASS/FAIL/NOT_RUN 판정)를 만들고, 판정 로직 pure fixture 시험과 별도 검토를 통과한 뒤 위 supervisor 아래에서 실행한다. S2 helper를 고쳐 쓸 수 있으면 그 사실과 차이를 근거에 남긴다.
  에이전트가 PR 전에 00-roadmap §2.2·01-design §11의 Windows 설치본 스모크를 수행한다. 해당 브랜치 최종 SHA의 `windows.yml` 산출물에서 저장소·workflow·run·source SHA·SHA256을 확인하고, WSL에서 PowerShell 절대 경로로 설치·실행·제거한다. 아래 기능은 소유 fixture(소유 임시 vault·소유 프로젝트)로 확인하고 앱·데몬 로그, user systemd unit 상태, 진단 정보와 필요시 스크린샷을 근거로 남긴다. UI 자동화가 불가능한 항목만 미실행으로 기록한다. 설치본은 `accept` 채널만 쓰고 `windows-acceptance` 잠금 안에서 진행한다. prod 설치본·`devbox@prod`는 바꾸지 않고 설치 전후 읽기 전용 비교로 불변을 기록한다. 소유 표식 없는 `accept` 채널 자원이 이미 있으면 제거하지 않고 질문하며, UAC·재부팅·사용자 클릭이 필요하면 멈춰 보고한다. v0.9.0(`com.devbox.*`, DevboxSuite)·기존 데이터는 보존하고 이번 스모크가 만든 설치 경로·데이터·WSL unit·소켓만 정리한다. 사용자 `~/notes`는 읽지도 쓰지도 않는다. **필수 선행 단계:** accept 데몬이 처음 시작하기 전(설치 프로그램의 자동 시작 포함, 즉 설치 실행 전)에 accept 설정 `~/.config/devbox-accept/config.toml`(`Paths.config_dir`, S2 Task 15c 채널 분리)에 `[notes]\nvault = "<소유 fixture 폴더>"`를 쓴다. 파일이 이미 있으면 소유 표식이 있을 때만 그 표만 고치고(원본 바이트를 근거에 보관, 정리 때 복원), 소유 표식이 없으면 고치지 않고 멈춰 질문한다. 확인 방법: fixture vault에 무작위 이름의 마커 노트 `accept-fixture-<nonce>.md`(내용 `<nonce>`)를 미리 만들고, 설치 뒤 먼저 accept 데몬 소켓이 `$XDG_RUNTIME_DIR/devbox-accept.sock`(`Paths.socket` = `devbox-<instance>.sock`)으로 떠 있는지, prod `devbox-prod.sock`이 아닌지 확인하고, `DEVBOX_INSTANCE=accept ~/.local/share/devbox-accept/bin/devbox mcp`(accept `bin_dir`, PATH의 prod `devbox`를 쓰지 않음) stdio로 MCP 읽기 도구( `devbox_note_read{ path: "accept-fixture-<nonce>.md" }`, 쓰기 도구는 쓰지 않음)로 그 내용이 `<nonce>`인지 본다. 설정이 잘못돼 사용자 `~/notes`를 보고 있더라도 그 이름의 파일은 없으므로 `not_found`만 나오고 사용자 노트 내용은 읽히지 않는다. 이 확인이 PASS하기 전에는 수용 1–6·9·10을 시작하지 않는다. 확인할 수 없으면 vault 관련 항목은 시작하지 않고 NOT_RUN으로 멈춘다. PASS·FAIL·미실행은 PROGRESS와 PR 본문에 구분한다. 자동 시험 성공을 Windows 실기 PASS로 바꾸지 않는다. S2의 interop errno110 사전 실패가 재현되면 같은 방식으로 FAIL/미실행을 기록하고 호스트 설정을 바꾸지 않는다.
  1. 노트를 쓰고 1초 뒤 자동 저장, 앱을 닫았다 열어도 그대로
  2. VS Code에서 같은 노트를 고치면 devbox가 비교를 보여 주고 덮어쓰지 않는다(VS Code 대신 소유 PowerShell/WSL 쓰기로 대체하면 그 사실을 적는다).
  3. 노트 이름을 바꾸면 다른 노트의 링크가 따라 바뀐다.
  4. 이미지를 붙여 넣으면 첨부로 저장되고 미리보기에 보인다.
  5. Ctrl+Alt+D로 일일 기록 → [오늘 한 일 넣기]에 커밋·닫힌 에이전트 작업이 보이고, 다른 PC 블록은 그대로다.
  6. 빠른 캡처로 한 줄을 남기면 inbox에 들어간다. accept 설치본은 release 빌드라 기본 Ctrl+Alt+N이 prod와 겹친다(OS 전역). 그래서 첫 실행 전 accept 앱 설정(`core/settings.rs` `Settings.global_shortcuts.capture`, accept identifier의 local app data 아래 설정 파일)에 prod·팔레트·터미널과 겹치지 않는 소유 키(예: Ctrl+Alt+Shift+F9)를 쓰고, 그 키로 단축키 경로와 SC2(300ms)를 실제로 판정한다. 키 등록이 그래도 실패하면(다른 앱 점유) 트레이 메뉴 "빠른 캡처"로 기능만 확인하고 단축키 경로·SC2는 미실행으로 적는다.
  7. 활동 기록에 동의하면 하루 보기에서 앱·프로젝트 시간이 보이고, 일시중지 동안은 비어 있다.
  8. 마스킹 규칙을 넣으면 해당 제목이 가려진다.
  9. Ctrl+Shift+F로 노트와 코드를 한 번에 찾는다.
  10. 노트 일부를 골라 에이전트 작업을 만든다.
- [ ] **Step 7: 상태 갱신·PR**

- PR 전에는 상태 줄을 직접 편집해 `구현 완료·최종 검증 결과 기록·PR 수용 대기`로 적는다. `sed` 패턴으로 완료를 추정하지 않는다. 하위 프로젝트 끝 브랜치 전체 검토와 최종 상세 검증·Biome CI를 마치고, 결과·미실행 항목을 PROGRESS와 PR 본문에 남긴다. 검증한 SHA 뒤에 결과 기록만 담은 PROGRESS 변경은 다음 작업 브랜치(S4)의 문서 커밋으로 인계해 검증 SHA를 바꾸지 않는다(00 §3).
- 00-roadmap §3에 따라 push·PR은 한 번 진행한다. 필수 CI `Frontend (pnpm)`·`Rust (Cargo workspace)`·`Rust (Windows)`와 자동 리뷰 3회 상한 판정(CONVENTIONS §8: 1–3회 지적 처리, 회차 수정은 모아 한 번에 push, 4회째부터 차단 기준만 수정·나머지는 PROGRESS §8 후속 목록) 뒤 squash 머지한다. 회차 수·지적 수·판정·이월 항목을 PR 본문에 남긴다.
- 실제 머지를 확인한 뒤 상태 줄을 직접 `완료(S3 PR 머지)`로 고치고 PROGRESS의 S3 행·PR 표·현재 위치(트랙 1 다음: S4-1)를 갱신한다. PR 전 수용 대기를 완료로 기록하지 않는다. S3 완료 때 `scripts/sweep.sh` 조건(00 §3 자원)을 확인한다.

```bash
bash scripts/check.sh
bash scripts/loc.sh 120000
# after branch review, Biome and the completion commit:
git push -u origin v1/s3-knowledge
gh workflow run windows.yml --ref v1/s3-knowledge
# verify source SHA / artifact SHA256, then hold windows-acceptance from download to cleanup:
python3 scripts/verify-resources.py --wait --lock windows-acceptance -- python3 -I <verification-dir>/s3-task12/acceptance.py <installer> <evidence-dir>
# open the S3 PR only here; require final CI3 + bounded review before squash merge.
```
