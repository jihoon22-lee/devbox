# S4 편집·Git 상세 실행 계획

- 상태: **S4-1 상세화 별도 검토 승인**(2026-10-24). 제품 구현 S4-2–9 미착수. 진행 원장은 [PROGRESS](PROGRESS.md).
- 기반: `main@e91c9b22efcc26905cea839420a2805af669a8e0`(S3 PR #644 squash, 검증 소스 `e56c3096`와 tree 동일). 브랜치 `v1/s4-editor-git`, worktree `/home/jihoon/projects/devbox-wt/v1-s4-editor-git`.
- 범위: [07 S4](07-s4-s6.md)·[01](01-design.md) §2.3·8.2–8.6·9·9.1-1/12/15·11·12, [00](00-roadmap.md) §3.6·7. 이 문서는 S4-2–9의 실행 상세다. 제품 범위·릴리스 정책은 바꾸지 않는다.
- 실행: 최신 AGENTS/CONVENTIONS가 과거 `superpowers` 스킬 문구보다 우선한다. 하네스 구현 에이전트 → root가 배정한 별도 검토 → 과제 커밋 → 다음 과제. 트랙당 하나, 전체 두 트랙을 유지한다. S4-1 계획 별도 검토 승인 후 S4-2부터 순서대로 구현한다.

## 1. 실제 코드와 계획의 대응

아래 기존 이름은 기반 tree에서 직접 확인했다. **새 계약** 표시는 아직 없는 API이며 해당 과제에서 Rust 선언부터 만든다.

| 영역 | 실제 기반과 S4 연결 |
|---|---|
| 문서 모델 | `crates/documents/src/model.rs`: `DocKey { root, path }`, `DocPolicy { kind: "code" }`, `DiskRev { hash,size,mtimeNs,ino }`. root는 `project:<id>`이며 `rootId/realRelPath`가 아니다. `CODE_LIMIT`, binary/encoding 분류와 read-only를 그대로 쓴다 |
| 저장·이동 | `documents.claim/take/renew/release/read/save/journal/journal_drop/journal_read/rename`, `documents.changed/moved/lease_lost`. `WriteScope`는 `crates/documents/src/scope.rs`; CLI `daemon/knowledge.rs`가 프로젝트 root를 주입한다. code 이동도 기존 rename·lease·저널 rekey를 사용한다 |
| 화면 문서 | `app/src/features/documents/store.ts`의 `useDocuments`/`DocumentsStore`: `open(key,policy)`, `save`, `replace`, `resolve`, `close`, `take`, `prepareWindowClose`. `machine.ts`의 code 정책은 수동 저장이다. 저장·복구·외부변경 상태 기계를 복제하지 않는다 |
| 편집기 | `EditorHost({docKey,language,readOnly,onSave,ariaLabel,extensions})`는 문서별 `EditorState`/undo와 Ctrl+S를 이미 소유한다. `extensions`는 문서 첫 생성 때만 읽으므로 LSP/설정 변경은 Compartment로 reconfigure한다. 호스트 unmount 시 상태가 사라지는 점을 고려해 탭/분할에서 호스트 수명과 문서 상태 이전을 검증한다 |
| 언어·테마 | `features/documents/editorExtensions.ts`가 `ui/code/{languages,theme,phrases}.ts`를 쓴다. `@codemirror/language-data`·search·autocomplete는 이미 app 의존성. source에 테마 사본을 만들거나 공용 읽기 전용 CodeView를 수정하지 않는다 |
| 빠른 열기 | `search.query`는 스트림. `{text,scopes:["files"],projectIds:[id],regex:false,caseSensitive:false,limit}` → JSON-line `SearchChunk`, `SearchHit {root,path,line,rangesUtf16,...}`. 별도 전체 파일 색인을 만들지 않는다 |
| 문제 | `observe.problems(ProblemsQuery)` → rev/cursor/truncated, `observe.problems.changed`. `Problem {file,line,col,severity,message,source}`는 기존 계약. `apply_problem_batch(ProblemRun,...)`는 run 누적 전용이므로 LSP를 가짜 run으로 넣지 않는다. S4-3에서 소스·URI 단위 교체 함수를 추가한다 |
| Git | `crates/git`은 `status`, `diff`, `mutate`, `run` 함수 모음이다. GitService/RPC는 없다. 기존 `GitError::StateChanged`는 unit variant라 payload를 조용히 바꾸지 않는다. 새 source RPC 오류 타입에 현재 상태를 싣고 기존 agents caller 호환을 유지한다 |
| Git 안전 | `run.rs`는 shell 없는 argv, literal pathspec, 환경 제거·출력/시간 한도·취소 시 owned process group 정리를 갖는다. `mutate::delete_branch_at` 등 S1의 OID 비교 삭제와 암묵 checkout 보호를 재사용한다. `agents/src/actions.rs`의 `check_expect`, `model.rs`의 `ReviewExpect`는 개념 참고이며 agents 의존 역전을 만들지 않는다 |
| diff | `app/src/ui/DiffView.tsx`는 `{file:DiffFile,onSelectLines?,maxLines?}`, parser는 `ui/diff.ts`. 현재 hunk action prop은 없다. source wrapper가 hunk별 기존 DiffView와 버튼을 조합하고 agent에서는 버튼 없이 같은 wrapper를 쓴다 |
| 터미널·창 | `TerminalView({id,readOnly?,...})`, `terminal.create`의 `cwd/command`, `openPopout(kind,id)`는 `features/terminal/popout.ts`. kind에 code가 아직 없다. `Shell.tsx`가 pages/popouts를 조립한다. native quit `app/src-tauri/src/core/quit.rs::note_window`와 `shell/NativeQuitPreparation.tsx`의 note/main 한정도 함께 넓혀야 한다 |
| 미리보기 | `notes.render(RenderNote)`와 `crates/notes/src/render.rs::render_bounded`, `features/notes/{Preview,mermaidPolicy,safeSvg}`가 있다. vault 경로를 source 프로젝트 경로로 가장하지 않는다 |
| 도구 | S4 기반에는 `crates/tools`가 없다. 원래 07의 `crates/tools/src/deps.rs` → `crates/deps/src/*`로 구현 위치만 바꾸며 기존 계획의 RPC 이름 `deps.list/audit`는 유지한다. 이 경로는 S5가 소유한다. Dependency Lens는 **새 `crates/deps`(`devbox-deps`)**에서 `deps.*`를 제공한다. S5 tools 내부 이름·doctor 인터페이스에 의존하지 않는다 |

외부 API 확인: 기존 lock에 lsp-client/merge는 없으므로 S4-4/6에서 승인된 패키지를 추가하고 그때 설치된 버전·타입으로 고정한다. 2026-10-24 확인한 [CodeMirror 공식 client 소스](https://raw.githubusercontent.com/codemirror/lsp-client/main/src/client.ts)의 Transport는 `send(string)`, `subscribe(handler)`, `unsubscribe(handler)`다. [공식 export 소스](https://raw.githubusercontent.com/codemirror/lsp-client/main/src/index.ts)는 `LSPPlugin`, 진단·정의·서식·이름 바꾸기 함수를 제공한다. 이 가변 main URL은 설치 버전 증거가 아니며, 구현 때 lock 버전과 `.d.ts`를 대조한다(문서 사이트는 조회 시 403).

## 2. 소유·통합·검증 경계

- 동결 `crates/core`, `crates/protocol`, `app/src/ui`, `app/src/rpc`(gen 제외), `.github`, `scripts`는 수정하지 않는 설계다. S4 feature에 `SourceDiff`·`MergeView`를 두고 양쪽 화면이 import한다. 공용 UI 경로로 옮기는 별도 리팩터링은 하지 않는다.
- 등록 예외인 `messages.ts`, `daemon/{routes,state}.rs`, `xtask/src/main.rs`, workspace/app 의존성 목록에는 S4 줄만 추가한다. CLI 조립 `daemon/mod.rs`, `Shell.tsx`, native command 목록도 S5 변경을 보존하며 최소 연결만 한다. source 전용 `daemon/source_routes.rs`·`lsp.rs`로 본문을 분리한다. 생성 RPC·lockfile은 손으로 합치지 않는다.
- S5는 PR #645 R2 로컬 수정 단계라는 참조만 보존한다. 먼저 머지된 main을 root가 통합할 때 양쪽 등록 줄을 보존하고 `pnpm gen`, Cargo metadata, pnpm lock 재생성 뒤 영향 검사한다. `Services.index` 등 S5 조립 내용을 추측해 새 필드를 중복 추가하지 않는다. 최종 검사 전에 root가 main 통합 시점을 조율한다.
- source 선택 → 변환 도구, API 응답 → 노트는 S6-1b다. 환경 점검 UI를 S4가 다시 만들지 않는다. R14 한도/사용량·설치 제안은 S4 LSP 상태 화면에 제공하고 S5 doctor와의 통합 필요 여부를 S6-1에서 판단한다.
- 동결 예외가 실제 필요해지면 구현 전에 PROGRESS §2에 파일·최소 변경·먼저 머지할 소유 트랙을 기록하고 root에 전달한다. 예외가 자동으로 사용자 승인 요청을 뜻하지는 않는다.
- 각 과제의 첫 좁은 실패 시험(RED) → 구현 → 같은 시험·직접 영향 검사(GREEN) → 독립 검토 → Biome → 영어 Conventional Commit. RED 로그와 GREEN 결과는 Git common-dir `devbox-verification/s4-task<N>/`에 원본 명령·source SHA/dirty diff·소유 fixture·resource JSON과 함께 남긴다. 계획 과제는 제품 RED가 아니라 문서·링크·범위 검사다.
- 좁은 명령은 아래 supervisor를 앞에 붙인다. 전체 check/build/clippy/E2E는 S4-9 끝에 한 번, 공유 lock `--wait`로 실행한다. Rust 전 `source ~/.cargo/env`. 트랙1 기본 target 유지, jobs2/threads2/workers2, CPU4/nice10, 지원 cgroup6/8GiB·swap1GiB. 무거운 검사는 root와 조율한다.

```bash
# 각 명령은 저장소 루트에서 독립 실행. Rust 명령 전 source ~/.cargo/env.
env XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  python3 scripts/verify-resources.py --no-lock -- <좁은 명령>
# 예: pnpm --filter app exec vitest run src/features/source/editor/tabs.test.ts --maxWorkers=2
# 예: cargo test -p devbox-git --test source_hunks stage_and_unstage_a_single_hunk -- --test-threads=2
```

S4-1은 의존성 설치가 필요 없다. 기존 S3의 같은 Biome 2.5.14 실행 파일을 PATH로 참조해 **S4 cwd에서** `pnpm exec biome ci .`를 실행할 수 있다. Markdown은 Biome 대상이 아니므로 별도 링크/공백/범위 검사 결과를 기록한다. 완료되지 않은 Windows/E2E는 PASS로 표시하지 않는다.

## 3. S4-2 — 코드 편집기와 파일 작업

**파일**
- 생성: `app/src/features/source/editor/{CodeEditor.tsx,CodeTabs.tsx,tabs.ts,quickOpen.ts,QuickOpen.tsx,languages.ts,extensions.ts,location.ts}`, `features/source/{FileTree.tsx,PopoutCode.tsx,externalEditor.ts,session.tsx}` 및 인접 시험.
- 생성: `crates/projects/src/{files.rs,file_model.rs,files_tests.rs}`; 필요 파일 본문은 600줄 전에 나눈다.
- 수정: `crates/projects/src/lib.rs`, CLI `daemon/{source_routes,knowledge,mod,routes}.rs`(source_routes는 새 파일), `features/documents/{EditorHost.tsx,store.ts,effects.ts}`, `shell/quitPreparation.ts`, `features/terminal/popout.ts`, `shell/{Shell,NativeQuitPreparation}.tsx`, `app/src-tauri/src/{core/quit,platform/quit}.rs`와 `platform/external_editor.rs`/command 등록. 상태 수명 보완이 필요하면 문서 feature 안에서만 한다.

**계약**
- `CodeEditor({docKey,location?})`는 `useDocuments.open(key,{kind:"code"})` + EditorHost를 사용한다. 저장 ACK 후에만 저장됨 표시. Ctrl+S/undo·read-only·Recovering·Conflict·Orphaned 선택은 기존 store로 전달한다. 미리보기 탭 교체는 clean 문서만, 편집/복구 후보는 pin한다. 탭 닫기 저장/버림/초안 유지의 결과 확인 전에 탭을 제거하지 않는다.
- `tabs` 순수 reducer: key 기반 preview/pinned, 활성 탭, 좌우 그룹, reorder/move/split. 같은 key의 undo는 탭 전환·분할 이동·section 전환에 보존한다. 같은 창에서 동일 문서를 두 그룹에 표시할 경우 단일 store 텍스트와 편집 동기화를 시험하고, 서로 다른 독립 undo 소유자를 만들지 않는다.
- 확장자 → 파일명 → shebang(`#!/usr/bin/env python3`, `#!/bin/bash`, env `-S`) 순서로 언어 추론. 들여쓰기 감지·끝 공백 표시·줄 이동/찾기/바꾸기는 CodeMirror extension. 키 범위 `data-keyscope="editor"` 안 Ctrl+F/H/G와 밖 Ctrl+P를 분리한다.
- **새 RPC** `files.tree {projectId,dir,showHidden,cursor?}` → bounded page `{entries,nextCursor?}`, `files.stat {key}` → `{key,kind,rev,readOnly?}`. 프로젝트 root를 서버가 찾고 정규화한다. `.git`, `.devbox-tmp-*`, `.trash`와 제외된 agent 소유 root가 문서로 열리지 않는다. 심볼릭 링크 밖 탈출·경로 재지정은 기존 WriteScope와 같은 경계를 따른다.
- **새 파일 조작** `files.create {key}`(빈 파일, 기존 존재 거부), `files.delete {key,expect}`(always; 파일/빈 폴더, 재귀 삭제 미지원). rename은 `documents.rename`으로 통일한다. 삭제 전 dirty 문서 선택을 처리하고 확인한 stat/내용이 변하면 거절한다. 디스크 삭제 뒤 열린 문서는 기존 Orphaned 전이로 보존한다.
- 빠른 열기는 search files 스트림 + 프로젝트별 최근 파일을 dedupe·점수화. 빈 질의는 최근 파일, 새 질의는 이전 stream close, 늦은 결과 무시. 검색 결과/문제의 경로·1-based 줄/UTF-16 열을 검증한 DocKey/location으로 source에 연결한다.
- code popout ID는 직렬화된 DocKey이며 실제 경로는 데몬에서 검증. 두 번째 창은 lease read-only·[여기로 가져오기]. note/main과 동일 native 종료 준비 round에 code 창을 추가하고 준비되지 않은 새 창이면 종료 거부. 앱 종료·분리 창 X의 dirty 버퍼 flush 실패 시 창 유지. **실제 차이:** 현 `prepareWindowClose()`는 policy를 가리지 않고 Dirty/Orphaned를 저장하며 close choice를 받지 않는다. 따라서 S4-2에서 code dirty 창은 먼저 저장/버림/초안 유지/취소를 선택하게 한다. 선택 전 자동 디스크 쓰기 0회; 취소는 native round를 실패 처리하고 입력을 다시 허용한다. keep은 journalSeq 예약값이 아닌 IndexedDB/daemon 내구 저장 ACK를 확인하고, discard는 저널 drop 완료를 확인한 뒤에만 창 종료 ACK를 보낸다. 기존 note 자동 flush 및 in-flight 최신 버퍼 보호를 유지하며, 탭 close reducer가 먼저 Closed를 내는 경로도 창 종료 안전성의 근거로 단독 재사용하지 않는다. 이 최소 변경은 documents store/effects와 quitPreparation에 배정한다.
- 외부 편집기: enum `code|cursor|zed`, argv/경로/배포판 검증 후 WSL interop, 실패 시 native `open_external_editor`. 임의 shell 문자열은 전달하지 않는다. 미설치/WSL 배포판 누락은 한국어 오류; 개인 전역 editor 설정 변경 없음.

**순서·실패 검사**
1. [ ] 언어/shebang·점수·preview→edit→pin·dirty close·그룹 이동 reducer 시험을 먼저 실패시킨다. `가😀.ts`, 공백 파일명, 같은 basename 두 root를 포함한다.
2. [ ] 소유 임시 프로젝트에서 tree pagination·hidden·scope escape·create-existing·delete-stale·dirty rename 저널 rekey를 실패시킨 뒤 files RPC/등록/생성한다. `crates/cli/tests/source_files.rs`에서 실제 RPC 경로를 검증한다.
3. [ ] CodeEditor/탭/빠른 열기·파일 트리·이동·외부 실행을 연결한다. 가짜 native launcher로 argv를 검증하고 실제 개인 editor를 띄우지 않는다.
4. [ ] `EditorHost` 인접 회귀: A 편집→B 편집→A Ctrl+Z는 A만 복원; 탭 reorder/split/section 이동 후 동일. code 1초 경과 자동 저장 0회, Ctrl+S 후 파일 내용·EOL/BOM/mode 보존; lease 이동/재연결 중 dirty 보존. native code popout quit 실패/성공 시험도 직접 실행한다.
5. [ ] 좁은 Vitest source/editor + 변경한 documents/quit/popout 시험, `cargo test -p devbox-projects files`, CLI source_files, 생성 `--check`와 영향 typecheck. E2E 여정 파일은 작성하되 실행은 S4-9에 모은다.
6. [ ] 별도 검토: S3 lease/CAS/undo/quit 회귀와 동결 파일 경계. Biome → `feat(source): add code editing and file navigation`.

## 4. S4-3 — LSP 서버 수명·중계·진단

**파일**
- 생성: `crates/lsp/{Cargo.toml,src/{lib,model,servers,framing,relay,supervisor,diagnostics,uri}.rs,examples/fake_lsp.rs}`와 단위/통합 시험.
- 생성: CLI `daemon/lsp.rs`, `crates/cli/tests/lsp.rs`; 수정 CLI 조립/등록, `crates/observe/src/{problem_store,lib}.rs` + 새 `problem_lsp.rs`/시험. protocol/core 수정 없음.

**계약**
- `ServerSpec {language,command,args,install:Option<String>}`: rust-analyzer, typescript-language-server(우선; vtsls 대체 감지), pyright-langserver, gopls, bash-language-server, yaml-language-server, taplo, vscode-json-language-server. PATH 탐지만 하고 자동 설치하지 않는다. 명령별 install 제안은 구현 시 공식 설치법 확인·고정 fixture로 검증한다.
- 새 `lsp.open {projectId,language}` stream → 기존 `StreamOpened`; `lsp.servers {projectId}` → starting/ready/failed/idle_stopped·PID 메모리·미설치/설치 명령·inotify 정보, `lsp.restart`, `lsp.install` → 터미널 ID. stream close/채널 단절이 close 계약이며 별도 중복 close RPC는 필요 없다. restart topic은 `lsp.restarted {projectId,language,failed,generation}`.
- CLI가 프로젝트 root/허용 경로·terminal adapter·diagnostic sink를 주입한다. LSP crate는 projects/observe/terminal 상태를 직접 소유하지 않는다. `lsp.install`은 화면에 표시한 명령을 사용자의 [설치] 액션에서 terminal.create로 실행; 터미널을 열어 sudo 입력 가능. 숨은 installer/호스트 패키지 설치 시험 금지.
- Content-Length는 UTF-8 **바이트 길이**, 분할/합쳐진 프레임·한도·EOF/잘못된 header 오류를 처리한다. 내부 동적 JSON은 구현 세부로 제한하고 RPC 경계는 생성된 구조/JSON 문자열을 쓴다. streams의 send/credit/close를 그대로 사용; 신용·큐 한도 초과면 LSP 세션 종료(01 §5.1), 메시지 조용한 유실 금지.
- 프로젝트·언어별 프로세스 1개. 클라이언트 요청 ID(숫자/문자열)→전역 ID와 응답 역매핑, 취소·클라이언트 단절 시 표 정리. 첫 initialize 결과를 generation에 묶어 공유하고 다른 클라이언트 initialized/shutdown이 공유 프로세스를 종료하지 않는다. 서버→클라이언트 요청도 대상과 응답 ID를 결속해 중복 응답을 막는다.
- URI는 데몬의 정규 WSL file URI를 사용하고 UTF-16만 광고한다. 파일별 didOpen 참조/lease 소유자를 추적해 read-only 창이 didChange를 보내지 않게 한다. lease 이동 시 이전 version 무시·현재 텍스트로 동기화; read-only 창 닫힘이 편집자의 didClose를 발생시키지 않는다.
- 마지막 client 이후 15분 idle stop. 비정상 종료 1·5·30초 backoff, 5분 안 3회 재시작 후 추가 crash에서 failed, 수동 restart는 한도 초기화. 새 generation은 initialize cache/request table 폐기. owned child만 종료·대기한다.
- **새 내부 함수** observe의 `replace_lsp_diagnostics(project,server,uri,generation,problems)`는 같은 source+URI 기존 항목을 교체, 빈 진단은 삭제, 다른 파일·run 문제 유지. run의 attempt 교체/clear·등록 목록·project filter/cursor·truncated 상한 의미를 그대로 보존하고 source lifecycle을 별도로 관리한다. run 재시작이 LSP 항목을 지우거나 LSP shutdown이 run 항목을 지우지 않는 교차 시험을 추가한다. 동일 내용은 rev 불변, 이전 generation은 거절. source `lsp:<server>`; LSP 0-based UTF-16 → 기존 Problem의 1-based line/col로 변환하고 경로는 `observe.resolve_location`과 일치시킨다.

**순서·실패 검사**
1. [ ] `framing_round_trips_split_and_coalesced_messages`에 비ASCII·oversize·잘못된 길이·EOF fixture RED → framing 구현.
2. [ ] fake server로 `two_clients_share_one_server_with_rewritten_ids`, 같은 ID 충돌·서버 요청·취소·한 창 close·lease 전환·초기화 경쟁 RED → relay 구현.
3. [ ] 가짜 시계로 `crash_loop_stops_after_three_restarts_in_five_minutes`, `idle_server_stops_after_last_client_leaves`, 14:59 재접속·수동 재시작 RED → supervisor 구현.
4. [ ] `diagnostics_flow_into_problems`와 빈 진단 삭제·old generation·동일 rev·다른 run 유지 RED → observe sink와 CLI 등록. 설치는 fake terminal로 전달값만 검증.
5. [ ] `cargo test -p devbox-lsp`에서 위 모듈/시험 타깃, 변경 observe LSP 시험, CLI `--test lsp`와 생성 검사. 실제 언어 서버 설치/프로세스는 S4-9 소유 fixture에서만 필요 범위 실행.
6. [ ] 별도 검토: 공유 서버 수명·URI/UTF-16·lease/세션·backpressure·로그에 본문 없음. Biome → `feat(lsp): relay shared language servers and diagnostics`.

## 5. S4-4 — LSP 화면과 미리보기

**파일**
- 생성: `features/source/editor/{lsp.ts,lspWorkspace.ts,lspCommands.ts,LspStatus.tsx}`·`features/source/{Preview.tsx,preview.ts}`와 시험.
- 수정: source CodeEditor, 필요 EditorHost extension slot, `crates/projects/src/files.rs` 및 새 `preview.rs`; CLI source 등록·생성. S3 Preview/renderer는 공개 기능을 재사용하며 별도 Markdown parser를 추가하지 않는다.

**계약·순서**
1. [ ] 설치된 `@codemirror/lsp-client` 타입에 맞춰 Transport를 구현한다. `client.openStream("lsp.open",params,handlers)`의 bytes를 메시지 경계로 복원하고 send 실패는 throw한다. `subscribe/unsubscribe` handler 집합·close·재연결 generation을 시험한다. renderer stream 포맷은 S4-3과 동일한 길이/메시지 단위 계약으로 고정하며 stream chunk 하나=JSON 하나라고 가정하지 않는다.
2. [ ] disconnect→새 연결→initialize→현재 lease 문서 didOpen을 다시 보내는 실패 시험을 먼저 둔다. 오래된 응답은 현재 editor에 적용하지 않는다. document save ACK의 seq/text에 대해 didSave를 정확히 보내고 실패 저장에는 보내지 않는다.
3. [ ] F12·Shift+Alt+F·F2·완성·hover·diagnostic을 실제 EditorState에 붙인다. LSP workspace edit는 열린 문서를 `useDocuments.replace`로 바꾸고 수동 저장 정책·undo를 지킨다. 다른 파일은 먼저 open/claim 후 적용; lease 없음·외부 rev 변경·지원하지 않는 resource operation은 명시 거절한다. 일부만 조용히 적용하지 않는다. `😀a`의 UTF-16 위치 시험을 둔다.
4. [ ] `LspStatus`에 상태·메모리·inotify 안내·[설치]/[다시 시작]을 연결하고 설치 결과 TerminalView/터미널 탭을 연다. 설치 실행은 표시한 서버의 명령과 결속한다.
   - **LSP HTML sink는 파일 미리보기와 별도다.** 위 공식 `client.ts`의 `sanitizeHTML`은 선택사항(L165–171)이므로 `LSPClient` 생성 때 반드시 지정한다. hover·completion documentation·signature help의 Markdown→HTML 결과에 기존 `features/notes/safeHtml.ts`의 allowlist 정책을 재사용하는 LSP adapter(`editor/lspContent.ts`)를 적용한다. script/event handler·iframe/object·style 실행 경로를 제거하고 이미지/외부 자원 로딩을 막는다. 링크는 http/https만 남기며 javascript/data/file/command URI를 거절한다. 허용 링크도 WebView 기본 탐색 대신 명시 클릭에서 기존 `openExternal`로 보낸다. 설치 버전의 각 sink가 callback을 통과하는지 확인하고 우회 sink는 같은 살균 wrapper 또는 plain text로 표시한다.
   - 공식 `client.ts` 기본 `window/logMessage` 처리(L136–140)는 원문을 console에 쓰므로 `notificationHandlers`에서 해당 알림을 소비하고 기본 handler에 넘기지 않는다. raw protocol·hover/완성/signature 본문·서버 로그 메시지를 console/tracing에 기록하지 않는다. 필요한 진단은 서버 ID·세션 generation·오류 종류 등 본문 없는 metadata로 제한한다. `window/showMessage`도 원문 HTML을 실행하거나 로그에 복제하지 않고 텍스트로만 표시한다.
   - **별도 RED→GREEN:** fake LSP가 hover·completion documentation·signature help 각각에 `<img src="https://fixture.invalid/leak" onerror="...">`, `<script>`, javascript/command 링크를 보내는 `lspContent.test.tsx`/adapter 시험을 먼저 실패시킨다. 실제 각 UI sink에서 실행 0·네트워크 요청 0·금지 탐색 0, 일반 강조/코드와 허용 링크 클릭 유지 assertion 후 sanitizer/링크 처리를 구현한다. `window/logMessage`의 고유 본문 sentinel을 보내 console spy와 daemon 로그 fixture에 해당 값이 없음을 검증한다. files.preview 시험의 PASS로 이 검사를 대신하지 않는다.

5. [ ] source 미리보기는 dirty buffer를 반영한다. `RenderNote {path:Option<String>,text:Option<String>}`의 body 전달 모드는 이미 있으나 path는 vault 기준이다. 프로젝트 상대 이미지가 vault로 해석되지 않도록 **새 `files.preview {key,text?}`**에서 WriteScope 검증 후 `render_bounded`에 프로젝트 이미지 resolver를 주입한다. 현재 `features/notes/Preview.tsx`는 notes.render 호출과 렌더를 함께 소유한다. 같은 feature 안에 `RenderedPreview.tsx`를 추출해 `RenderedNote {html,mermaid}`·link callback을 받게 하고 노트/source 양쪽이 쓰며 기존 sanitizer·Mermaid 정책을 유지한다. SVG는 살균된 bytes의 blob `<img>`로만 표시하고 URL을 정리한다.
6. [ ] 실패 시험: 악성 SVG script/event handler·Markdown javascript/원격 이미지·Mermaid 외부 참조 차단, 프로젝트 외부 이미지 거절, dirty 미리보기 갱신과 late response 무시. 기존 notes preview·safeSvg·mermaidNetwork도 직접 영향 범위 실행한다.
7. [ ] 좁은 source LSP/preview 및 lspContent Vitest(세 HTML sink·raw log 억제 포함), projects preview·CLI RPC 시험, 생성/영향 typecheck. 가짜 LSP 진단→밑줄+문제 패널 E2E는 S4-9에 실행한다. 별도 검토 후 Biome → `feat(source): connect LSP features and safe previews`.

## 6. S4-5 — Git 전체 조회·변경

**파일**
- 생성: `crates/git/src/{service,model,rpc,hunks,branches,stash,blame,remote,watch,safety,cleanup}.rs`, `crates/git/tests/{source_hunks,source_remote,source_queries,source_watch,source_safety}.rs`.
- 수정: 기존 `lib/status/diff/mutate/run.rs`는 함수 재사용·stdin patch 지원 등 필요한 부분만. CLI `daemon/source_routes.rs`/조립/생성; projects 추가 시 경고 연결. source UI는 S4-8에서 조립한다.

**새 RPC 계약**
- 모든 source 요청은 `target: {projectId, worktreeId?}`로 서버가 등록 프로젝트/agent 작업의 허용 worktree를 해석한다. renderer 임의 절대 경로를 Git cwd로 쓰지 않는다. 기존 agents RPC·ReviewExpect와 공존한다.
- `git.status`, `git.diff {path,staged}`, `git.branches`, `git.log {path?,limit,skip}`, `git.blame {path}`, `git.worktrees`, `git.stash_list`, `git.cleanup_candidates`는 typed snapshot을 반환한다. branch/head/upstream snapshot은 push 확인과 동일한 RPC 상태를 사용한다.
- `git.stage/unstage {path,hunk?,expect}`, `git.discard {path,hunk?,expect}`(always), `git.commit {message,amend,expect}`, `git.checkout {branch,create?,expect}`, `git.branch_delete {branch,force,expect}`(always), `git.stash {action,payload,expect}`(drop은 always 별도 선언으로 분리 가능). hunk ID는 최신 daemon diff snapshot에 묶고 renderer의 임의 patch 실행을 받지 않는다. binary/rename/mode-only는 파일 단위 동작으로 표시한다.
- `git.fetch`, `git.pull {rebase,expect}`, `git.push {forceWithLease,expect}`는 작업 ID/상태 조회 + `git.operation_changed` 진행 이벤트. push/pull은 always. 사용자 취소는 owned Git 프로세스 종료 후 실제 완료 여부 조회로 정합성을 맞춘다. 인증은 기존 Git credential helper만 사용하고 출력에 인증 값이 남지 않도록 기존 오류 가림 경계를 따른다.
- `RemoteExpect {branch,headOid,upstream,upstreamOid}`. upstream에는 remote 이름·목적 ref를 포함한다. 실행 직전 branch/HEAD/upstream 설정·OID 재비교; 불일치는 source 오류 `git.state_changed`와 **현재 상태** 반환, remote 작업 0회. 기존 unit GitError는 source 오류로 변환하고 schema에 같은 오류 코드의 서로 다른 detail이 충돌하지 않도록 export 검사한다.
- push는 확인한 `headOid:refs/heads/<branch>` refspec과 명시적 `--force-with-lease=<ref>:<expectedOid>`를 사용한다. detached/unborn/no upstream은 상태로 표시하고 대상 지정 전 push/pull 비활성; 묵시적 다른 원격/브랜치로 보내지 않는다. 외부 프로그램과 완전한 원자성은 주장하지 않는다.
- discard/branch 삭제/stash drop/cleanup도 사용자에게 보인 파일 hash·index OID 또는 ref OID/대상 ID에 결속한다. 재조회 후 변경이면 거절한다. S1 compare-and-delete를 사용하고 active/dirty/locked/host-owned/unmerged worktree·사용자 branch는 정리 후보에서 보존한다.
- watch는 `.git`이 디렉터리인 repo와 gitdir 파일인 linked worktree 모두 common-dir/HEAD/index/refs/packed-refs를 감시, 300ms debounce. 변경 이벤트는 `git.changed {projectId,worktreeId?}`; 단절/resync에서 화면이 snapshot을 다시 받는다.
- 프로젝트 등록 때 `core.fsmonitor`, `core.hooksPath`, `filter.*`, `diff.external` 설정 경고를 반환/표시한다. 이 경고는 실행 승인이나 Git hook 전면 차단을 의미하지 않는다. PROGRESS §8의 S1 명시적 hook 실행 한계를 숨기지 않는다. 읽기 전용 diff의 외부 driver 차단은 유지한다.

**순서·실패 검사**
1. [ ] 소유 임시 repo에 서로 떨어진 두 hunk → 첫 hunk stage/unstage/discard 후 다른 hunk·index 보존. stale hunk/hash면 변경 0. `stage_and_unstage_a_single_hunk`, `discard_hunk_restores_only_that_hunk` RED→GREEN.
2. [ ] `amend_keeps_author`, unborn commit, branch create/checkout/delete-stale, stash push/apply/pop/drop, path log/blame, binary/rename fixture를 먼저 실패시킨 뒤 구현. CLI `source_git` 등록·always/reversible metadata를 시험한다.
3. [ ] AR-F01: bare 원격 refs snapshot → A 상태 확인 → 외부 B checkout → push(A expect) → typed state_changed·**전체 remote refs 동일**. 정상 A 양성 대조군. pull도 checkout/HEAD/upstream config 각각 변화 주입, 작업 시작/remote 호출 0.
4. [ ] `push_force_with_lease_refuses_when_remote_moved`: 별도 clone이 bare ref를 옮긴 뒤 이전 expect로 force push → 거절. 확인 이후 HEAD 이동도 거절하고 refspec에 확인한 OID만 쓰는 argv/실제 bare 시험을 둔다.
5. [ ] 외부 commit/index/packed-refs·linked worktree 변경 이벤트, 위험 Git 설정 경고와 암묵 checkout 보호, 정리 보호 fixture를 실행한다. 실제 사용 repo·원격에는 쓰지 않는다.
6. [ ] `cargo test -p devbox-git --test source_hunks` 등 새 타깃과 바뀐 함수의 S1 기존 시험, CLI source_git, 생성 검사. 별도 검토(원격 ref 증거·expect·취소·보호 정리) → Biome → `feat(git): add source operations and guarded remotes`.

## 7. S4-6 — 충돌 해결과 CV-F01

**파일**
- 생성: `crates/git/src/{conflicts,conflict_model}.rs`, `crates/git/tests/source_conflicts.rs`, `features/source/merge/{MergeView.tsx,model.ts,MergePanel.tsx}`와 시험.
- 수정: git RPC/export, CLI source 등록, agent 기준 반영 충돌 이동 연결. `@codemirror/merge` 추가(설치 타입으로 API 고정), `app/src/ui`는 동결 유지.

**계약**
- `git.conflicts {target}` → 수정/수정·삭제/수정·추가/추가 목록. `git.conflict_preview {target,path}` → `{base,ours,theirs,merged,labels,token}`; token은 `{worktreeHash,stageOids:[string|null,string|null,string|null]}`에 대상 identity를 결속한다. 삭제/없는 stage는 null, binary/oversize는 텍스트 해결 불가로 표시한다.
- `git.conflict_apply {target,path,resolution,token}`: index stage OID와 작업 파일 hash 재검사 → 같은 경우 원자적 파일 교체와 index 반영. file-scope 고정·daemon 작업 직렬화·Git index lock 경계를 명시한다. 외부 파일 변경 race는 S3의 교체 후 비교/rollback 패턴을 재사용해 stale 데이터 덮어쓰기를 피한다. index 반영 실패는 성공으로 보고하지 않고 해결안/현재 파일·index 상태를 보존해 재시도 가능하게 한다. 외부 Git 프로세스 전체와 완전 원자성은 주장하지 않는다.
- stale 오류는 `git.conflict_stale` typed detail에 현재 token/preview 또는 재조회 key를 담고, 과대 응답은 bounded preview 조회로 분리한다. 입력 해결안은 renderer state에 보존한다(서버 오류에 필요 없는 전체 본문을 echo해 프레임 예산을 늘리지 않는다).
- UI base·ours·theirs·결과 네 칸, [내 것][그쪽][둘 다]·직접 편집. merge/rebase 단계의 실제 branch/commit 이름을 보여 준다. rebase ours=onto, theirs=적용 중 commit. 오래된 응답이 새 해결안을 덮지 않는다. agents 충돌도 동일 target-aware MergePanel을 사용한다.

**순서·실패 검사**
1. [ ] 소유 repo 세 종류 충돌 fixture, apply 정상 stage0·내용 검증을 준비한다.
2. [ ] `apply_refuses_when_file_changed_after_preview`: preview → 외부 파일 수정 → 작성한 resolution apply → stale·외부 파일 그대로·**stage 변화 0**. `apply_refuses_when_index_stage_changed_after_preview`: 작업 파일 그대로 두고 stage OID만 변경 → 같은 거부. stale 검사 직후 교체 race도 테스트 seam으로 주입한다.
3. [ ] `resolution_text_is_preserved_after_stale`: local resolution을 편집하고 stale 응답/재조회 후에도 그대로, 새 preview와 나란히 비교. 기존 07의 `resolution_text_is_returned_with_stale_error` 목적을 renderer 보존 assertion으로 충족한다.
4. [ ] 삭제/수정·추가/추가·rebase label, index 실패·파일/디렉터리 대체·모드 보존 회귀를 구현과 함께 검증한다.
5. [ ] Git source_conflicts, CLI source_git 충돌 subset, merge Vitest + agent 충돌 이동 영향 검사. 별도 검토(CV-F01 디스크/index 무변화와 해결안 보존) → Biome → `feat(git): resolve conflicts with stale preview protection`.

## 8. S4-7 — Dependency Lens

**파일**
- 생성: `crates/deps/{Cargo.toml,src/{lib,model,locks,osv,service}.rs,tests/locks.rs,tests/fixtures/*}`, parser는 ecosystem별 모듈로 분리.
- 생성: `features/source/deps/{DependencyLens.tsx,model.ts}`/시험; CLI source 등록·Services 조립·workspace 의존성 추가. `crates/tools`에는 쓰지 않는다.

**계약·순서**
1. [ ] `deps.list {projectId}` → `{ecosystem,name,versions[],direct}`. pnpm-lock.yaml/package-lock.json/yarn.lock/Cargo.lock/go.sum/poetry.lock/uv.lock의 소유 fixture를 먼저 시험한다. lock schema별 지원/미지원 오류를 구분하고 파일 크기/항목 수를 제한한다.
2. [ ] direct는 lock이 제공하는 importer/root package 관계로 판정하고 부족하면 같은 프로젝트의 package.json/Cargo.toml/go.mod/pyproject.toml을 읽는다. 이름만으로 direct를 추측하지 않는다. pnpm importer 여러 개, scoped name·peer suffix, Cargo 같은 이름 여러 버전·git dependency, go.sum의 /go.mod 줄 중복, Python 정규화 사례를 포함한다.
3. [ ] `deps.audit {projectId}` → 작업 ID, 조회/완료/오류 상태와 진행 이벤트. OSV querybatch를 ecosystem/name/version으로 묶고 조회 결과는 **정규화한 의존성 집합 hash + endpoint** 기준 30분 캐시한다. 다른 프로젝트/변경 lock 결과를 잘못 재사용하지 않는다. upstream 응답은 typed model로 가공한다.
4. [ ] 네트워크 실패는 “조회 실패 · 다시 시도”, 마지막 성공 데이터는 시각/오래됨을 표시하고 취약점 없음으로 바꾸지 않는다. 중복 버전·direct/transitive·취약점 링크를 표시한다. deps.dev/자동 업데이트/취약점 자동 수정은 범위 밖.
5. [ ] OSV 고정 응답과 fake clock/HTTP adapter로 querybatch 매핑·부분 누락·오류·30분 경계·lock 변경 캐시 무효화를 RED→GREEN. 실제 OSV 네트워크는 단위/통합 시험에 사용하지 않는다.
6. [ ] `cargo test -p devbox-deps`, CLI deps subset, source deps Vitest와 생성/영향 typecheck. 별도 검토 후 Biome → `feat(source): add dependency inventory and OSV audit`.

## 9. S4-8 — 소스 섹션 화면 통합

**파일**
- 생성/완성: `features/source/{SourcePage,ChangesList,SourceDiff,BranchList,CommitBox,LogView,BlameGutter,CleanupPanel,GitStatus}.tsx`, feature CSS, 인접 시험.
- 수정: `shell/Shell.tsx`, 상태 표시줄 Git 연결, `features/search/CodePreview.tsx`의 [편집기로]와 문제 위치 이동. 기존 source/editor/merge/deps를 조립한다.

**순서·검증**
1. [ ] [파일|변경|브랜치] 선택·프로젝트 전환·분리 창·좁은 창·키보드 탭 조작을 실패 시험으로 먼저 고정한다. 프로젝트가 바뀌어도 다른 프로젝트 dirty 탭/미완료 Git 결과를 새 프로젝트에 귀속시키지 않는다.
2. [ ] 파일 트리 hidden·새 파일·rename·always 삭제·외부 editor·terminal cwd를 연결한다. 변경 목록은 staged/unstaged/conflict 그룹, 파일 선택은 SourceDiff/merge 탭. SourceDiff는 기존 DiffView 파서와 인용 범위를 그대로 사용한다.
3. [ ] 브랜치 로컬/원격·create/checkout/delete·worktree·stash·log/blame·정리 패널을 연결한다. 정리 선택의 snapshot을 확인 UI에 표시하고 stale이면 재확인. 보존 대상 선택 불가·실패 목록 유지.
4. [ ] CommitBox 72자 안내·Ctrl+Enter·amend·커밋 후 push. 커밋 완료 뒤 **새 expect를 읽어** push 확인을 받는다. commit 성공 뒤 목록 갱신 실패는 커밋을 다시 실행하지 않고 결과/재조회만 표시한다. push/pull 항상 확인, force-with-lease 명시. 상태 Git 칩 branch/ahead/behind/변경 수→source 이동.
5. [ ] 완료 이벤트·조회 지연이 최신 선택/프로젝트를 덮지 않는 Vitest, confirmation expectation 전달·stale 재확인, hunk 버튼 read-only 모드 숨김, native 외부 편집 실패 문구를 검증한다. CLI source 관련 직접 영향만 실행한다.
6. [ ] E2E 세 여정을 `app/e2e/source.spec.ts`에 추가: (a) 수정→hunk stage→커밋 (b) 충돌→stale 거부·해결안 유지→새 preview 해결→커밋 (c) 브랜치 생성·전환. 기존 편집/가짜 LSP 여정과 중복을 합쳐 전체 40개 이하를 유지한다. 실행은 S4-9.
7. [ ] 별도 검토: 모든 승인 범위 UI 도달 가능·한국어 문구·expect 확인/결과의 같은 대상·600/300줄 경계. Biome → `feat(source): integrate editing and Git workflows`.

## 10. S4-9 — 에이전트 공유·최종 검증·PR

**파일**: 실제 `features/agents`의 변경/커밋/기준 반영 화면과 source 공용 feature 컴포넌트, 관련 시험·E2E; PROGRESS와 07 S4 상태. scripts/workflow는 변경하지 않는다.

1. [ ] agent 변경/commit 탭을 `ChangesList`·`SourceDiff`·`LogView`로 연결한다. mode=`review`에서 hunk/commit mutation은 숨기고 기존 인용 `DiffRange` 동작을 보존한다. agent rebase/기준 반영 충돌의 target은 그 작업 폴더이며 source 현재 프로젝트와 혼동하지 않는다. 이를 좁은 실패 시험 → 구현 → 관련 agent/source 시험으로 검증한다.
2. [ ] 별도 과제 검토 후 root가 최신 main(S5 머지 여부 포함)을 통합한다. 두 트랙 생성/lock 재생성·등록 병합·직접 영향 검증을 기록한다. 브랜치 전체 별도 검토를 완료하고 미해결 차단을 먼저 수정한다.
3. [ ] `scripts/check.sh`의 실제 포함 항목을 읽고 전체 frontend/Rust/typegen/clippy/build를 한 번 실행한다. E2E는 별도 진입점에서 한 번(전체 검사 포함이면 중복하지 않음). shared lock --wait 최대60분/exit75 대기 초과. 통과한 무관 검사 반복 금지. `bash scripts/loc.sh 160000`, E2E≤40·파일600/컴포넌트300·scripts+.github≤1500/workflow≤3 계측.
4. [ ] S4 설치 수용은 root가 허용한 최종 source SHA Windows workflow artifact를 사용한다. repository/workflow/run/source SHA/SHA256·accept 채널·windows-acceptance 단일 잠금·prod 읽기 전용 전후 비교·소유 자원 정리를 기록한다. 기존 서비스/Docker/방화벽/WSL shutdown/사용자 설정 변경 금지. 실제 실행 불가 항목만 NOT_RUN이며 자동 fake 시험 성공으로 실기 PASS를 만들지 않는다.
5. [ ] 아래 8항목을 실제 설치본의 소유 프로젝트/로컬 bare 원격에서 assertion한다. 실외 원격 push나 개인 저장소 수정은 필요 없다. 도구 설치 항목은 실제 실행 가능한 owned 설치 경계가 없으면 NOT_RUN으로 남기고 hidden 설치를 하지 않는다.
6. [ ] 최종 Biome CI·검증 artifact·정확한 소스 근거·미실행/미입증을 PROGRESS/PR 본문에 기록. 별도 검토 후 `feat(source): share Git review views and complete S4` 과제 커밋. root 조율로 push·**S4 PR 한 번**. 필수 Frontend/Linux Rust/Windows Rust CI3와 자동리뷰 최대3회 판정 후 squash. 상한 뒤 실제 손실/유출/미승인 실행만 별도 재현/검토로 수정, 나머지 PROGRESS §8. 실제 merge SHA 확인 전 S4 완료로 바꾸지 않는다.

| 설치 수용 | 근거/판정 |
|---|---|
| 1 파일 열기·편집·저장 | 설치본 UI→daemon 디스크 내용, code 수동 저장/undo |
| 2 진단 밑줄·문제 탭 | 같은 실제 서버·파일·진단의 두 표시와 수정 후 해제 |
| 3 F12 | 정의 대상 DocKey/줄/UTF-16 위치 |
| 4 서식 | 적용 buffer·undo·Ctrl+S 뒤 디스크 |
| 5 hunk stage→commit→push | 소유 repo index/commit/로컬 bare refs와 확인 상태 |
| 6 CV-F01 | 외부 수정 후 apply 거부, 해결안·파일·stage 보존 |
| 7 언어 서버 설치 | 표시 명령=터미널 실제 실행, 실행 불가면 NOT_RUN |
| 8 Dependency Lens | 중복 버전 표시·실제 OSV 조회와 실패 구분; fixture는 별도 자동 근거 |

## 11. 추적 입력과 완료 판정

| 출처 | 구현/검증 위치 | 입력·기대 assertion |
|---|---|---|
| CV-F01, 00 §7 | S4-6 source_conflicts + merge UI, S4-8 E2E, S4-9 설치6 | preview→외부 수정→apply; 거부·작성안 보존·stage 변화0. index-only 변경도 거부 |
| AR-F01, 00 §7 | S4-5 source_remote + CLI, S4-8 확인 UI, S4-9 설치5 | A 확인→외부 B checkout→push/pull; state_changed·remote refs 변화0. A 불변 양성 대조, HEAD/upstream 변화, lease 원격 이동 포함 |
| CV-F02/03/06·AR-F13(공통 문서 회귀) | S4-2 documents 영향 검사 | 연속 conflict 버퍼 보존, dirty rename→save→reopen 옛 저널0, [디스크 것] 뒤 Recovering 아님, 내부 파일 tree 제외 |
| AU-K1 | S4-2 EditorHost/탭 | A/B 전환·분할/이동 후 Ctrl+Z는 A만 |
| AR-F09/01 §9.1-15 위치 규칙 | S4-3/4 URI/진단/workspace | `가a나`와 `😀a`에서 UTF-16 위치; 0/1-based 변환 확인 |
| R7·R14 | S4-3 supervisor/status | 마지막 client+15분 종료, 5분 3회 제한, inotify 읽기/안내만 |

S3의 기존 수용 미실행 4·6·7·8/SC2와 자동저장 observed 1384ms·strict≤1초 NOT_PROVEN은 S4가 PASS로 바꾸지 않는다. 이 문서 상세화 검토는 실행 전 계약을 검토하는 것이며 제품/Windows 성공 근거가 아니다.
