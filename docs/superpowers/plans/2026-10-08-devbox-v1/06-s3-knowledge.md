# S3 기록 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획(과제 수준) · 미착수 · 시작 조건: S2 완료([05](05-s2-runs.md) 상태 줄)
- 상세화 수준: 과제·파일·인터페이스·핵심 테스트까지 적었다. **Task 1에서 실제 S0b–S2 코드에 맞춰 단계별 코드까지 상세화한 뒤 실행한다.**

**Goal:** 개발하면서 남기는 기록을 devbox 안에서 끝낸다.
- 노트 vault(Markdown·위키링크·백링크·템플릿·이미지)
- 일일 기록과 "오늘 한 일" 요약
- 빠른 캡처
- Windows 활동 기록(개인정보 규칙 fail-closed)
- 노트·코드·파일 이름 통합 검색
- 노트에서 에이전트 작업 만들기
- MCP 노트 도구

**Architecture:**
- `crates/documents`가 문서 읽기·저장·저널·이름 바꾸기·외부 변경 감시를 소유한다.
  - 저장은 CAS(`baseRev`)와 원자적 교체로 한다.
  - 같은 문서는 lease를 가진 창 하나만 편집한다.
- 화면의 `DocumentStore`는 01-design §8.4 상태 기계를 그대로 구현한 순수 리듀서 + 효과 실행기다. S4 코드 편집기도 같은 것을 쓴다.
- `crates/notes`는 vault 규칙(frontmatter·링크·템플릿·첨부·일일 기록·살균 렌더)을 맡는다.
- `crates/search`는 `index.db`의 FTS5와 `grep-searcher`를 쓴다.
- `crates/activity`는 v0.9.0 `activity-engine` 순수 로직을 이식하고 저장을 묶어 쓴다. Windows 쪽 수집기는 `crates/win`에 둔다.

**Tech Stack:** S2 스택 + CodeMirror 6(`@codemirror/lang-markdown`, `@codemirror/autocomplete`), `pulldown-cmark`, `ammonia`, `mermaid`(화면, `securityLevel: "strict"`), `rusqlite` FTS5, `grep-searcher`·`grep-regex`·`ignore`, `nix`(`renameat2`), `idb-keyval`(화면 임시 저널).

**Spec:** [01-design.md](01-design.md) §2.2·§6.1 활동·§6.2·§8.4·§9 `documents`·`notes`·`search`·`activity`·§9.1-2·3·5·6·8·§9.3 이식 표

## Global Constraints

- S0b–S2의 Global Constraints를 따른다.
- 작업 위치: S3 전체가 브랜치 `v1/s3-knowledge`(origin/main에서) 하나와 worktree `../devbox-wt/v1-s3-knowledge` 하나다. Task 1(상세화)이 이 브랜치의 첫 커밋이다. PR은 Task 12에서 한 번 연다(00-roadmap §3).
- **S5와 동시에 진행한다**(사용자 결정 2026-10-08). 다른 세션이 `v1/s5-tools`에서 일한다. 공유 파일·잠금 파일·생성 파일·rebase 규칙은 00-roadmap §3.6을 따른다. S3에서 S5 기능(API·변환 도구)을 쓰거나 고치지 않는다. 두 쪽을 잇는 기능(API 응답을 노트에 붙이기 등)은 S6-1b다.
- 문서 쓰기·삭제는 등록한 프로젝트(에이전트 작업 폴더 포함)·vault·devbox 데이터 폴더 안에서만 한다. `realpath`로 확인하고 `O_NOFOLLOW`로 연다(01-design §12).
- 원자적 교체는 `renameat2(RENAME_EXCHANGE)`, 새 파일은 `RENAME_NOREPLACE`를 쓴다. mode·EOL·BOM을 보존하고, symlink는 realpath 대상에 쓴다(01-design §9.1-2).
- 이진·비 UTF-8·크기 초과(노트 2 MiB, 코드 5 MiB) 파일은 읽기 전용으로 연다.
- 문서 내용·제목·경로는 로그에 남기지 않는다.
- 활동 개인정보 규칙은 fail-closed다. 규칙을 받지 못했거나 정규식 컴파일에 실패하면 제목을 보내지 않는다.
- vault 기본 위치는 `~/notes`다(Q4 확정).
- 두 PC가 같은 vault를 Git으로 공유한다. 일일 기록 요약은 PC 이름 소제목 아래에만 쓴다.

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 저장 중 외부 편집기가 같은 파일을 고침 | 덮어쓰지 않고 비교 화면. 버퍼·저널 보존 | Task 2·3 |
| 여러 노트 링크를 고치는 이름 바꾸기 도중 데몬 종료 | 다음 시작 때 진행 저널로 남은 파일을 이어서 고치거나 목록을 보여 줌 | Task 5 |
| 노트 A·B를 오가며 편집 후 Ctrl+Z | 각 노트 자기 undo 기록만 되돌림(AU-K1) | Task 3 |
| 활동 저장이 디스크 부족으로 실패 | 화면·트레이에 "저장 중단", 뒤 세션이 조용히 사라지지 않음(CV-F08) | Task 8 |
| 검색 결과를 보는 중 색인이 갱신됨 | 마지막 성공 결과와 선택을 유지, rev가 바뀐 경우에만 다시 조회(CV-F07) | Task 7 |
| 충돌 화면에서 [디스크 것]을 고른 뒤 앱을 다시 엶 | 그 문서가 복구 후보로 다시 나오지 않음(AR-F13) | Task 3 |
| 기본 템플릿(빈 title)으로 만든 노트를 파일 이름으로 검색 | 1건 찾음(AR-F10) | Task 4·7 |

---

## 작업 묶음

S3 전체가 브랜치 `v1/s3-knowledge` 하나, PR 하나다(00-roadmap §3). 작업 위치는 worktree `../devbox-wt/v1-s3-knowledge`다.
묶음은 그 안의 중간 지점이다. 묶음이 끝나면 로컬 검사(`pnpm check`, 그 시점에 없으면 있는 검사만) → `PROGRESS.md` 묶음 행 갱신 커밋 → push 한다. PR·CI는 없다. 세션 인계 지점이 된다.
PR은 마지막 묶음이 끝난 뒤 한 번 열고, 그 CI가 S3 전체를 한 번 검사한다.

| 묶음 | 과제 | 끝나면 |
|---|---|---|
| 상세화 | Task 1 | 커밋(이 브랜치의 첫 커밋) |
| S | Task 2–3 | push |
| T | Task 4–6 | push |
| U | Task 7 | push |
| V | Task 8–9 | push |
| W | Task 10–11 | push |
| 마무리 | Task 12 | **S3 PR 열기** → CI·설치 파일 → 사용자 확인 → rebase 머지 |

---

### Task 1: 실제 코드에 맞춘 상세화와 이식 대상 확정

**Files:**
- Modify: 이 문서

- [ ] **Step 1:** `PROGRESS.md` §5(계획과 달라진 점)를 먼저 읽는다. S0b–S2에서 실제로 만들어진 이름(`method!`·`Router`·`Ctx::open_stream`·`Db`·`index.db` 경로·`notify.raised`·화면의 `SectionLayout`·`useBottomTabs`·`openPopout` 등)으로 Interfaces를 고친다.
- [ ] **Step 2: 이식 대상 확정.** v0.9.0 태그(`git show v0.9.0:<경로>`)에서 다음을 읽는다.
  - `crates/activity-engine/src/core/{sessionizer,privacy,idle,attribution}.rs`와 테스트
  - `crates/wsl-helper/src/document.rs`, `crates/knowledge-vault-engine/src/platform/document_publish.rs`(RENAME_EXCHANGE)
  - `crates/knowledge-vault-engine/src/core/{rename,vault}.rs`(링크 재작성 규칙)
  - `crates/markdown/src/lib.rs`(ammonia 설정)
  - 활동 idle 결함 AU-K2는 10-03 감사 K2다. 관찰이 반복된 뒤 idle로 끝나면 마지막 입력 뒤 298초까지 활동으로 집계했다. 출처는 `docs/superpowers/plans/2026-10-03-product-readiness/08-traceability.md`(v0.9.0 공개본 커밋 `72ff50c7`에 있음: `git show 72ff50c7:<경로>`)이고, v0.9.0 태그에는 수정본이 들어 있다. v0.9.0의 `sessionizer.rs`·`idle.rs` 테스트 목록을 Task 8·9에 옮겨 적는다.
- [ ] **Step 3:** Task 2–12를 `writing-plans` 형식으로 펼친다. 아래 "핵심 테스트"와 [00-roadmap §7](00-roadmap.md) 추적표에서 S3에 걸린 시험(CV-F02·F03·F06·F07·F08, AR-F10·F12·F13·D01)과 입력값을 그대로 포함한다.
- [ ] **Step 4:** 커밋.

```bash
git add docs && git commit -m "docs(plan): detail S3 against the S2 code"
```

---

### Task 2: `crates/documents` — lease·읽기·CAS 저장·저널·이름 바꾸기·감시

**Files:**
- Create: `crates/documents/{Cargo.toml,src/lib.rs,src/model.rs,src/sys.rs,src/lease.rs,src/journal.rs,src/watch.rs}`
- Modify: `crates/core/src/paths.rs`(쓰기 허용 범위 `WriteScope`)

**Interfaces:**
- `DocKey { root_id, rel_path }`(root = 프로젝트·vault). 화면은 경로 문자열 대신 이것을 쓴다.
- `DiskRev { hash: blake3 hex, size, mtime_ns, ino }`
- `DocPolicy { kind: Note|Code, max_bytes, autosave }`
- 메서드:
  - `documents.claim{ key, policy, windowId }` → `{ leaseId, rev: DiskRev?, text?, readOnly?: { reason: binary|encoding|too_large|leased_elsewhere }, journal?: { seq, baseRev, text } }`
  - `documents.release{ leaseId }`
  - `documents.take{ key, windowId }`(mutation): 다른 창의 lease를 가져온다. 이전 창에는 `documents.lease_lost` 이벤트를 보낸다.
  - `documents.save{ leaseId, baseRev?, expect?: "absent", text }`(mutation)
    - 결과: `{ rev }` 또는 오류 `documents.conflict{ diskRev, diskText }`·`documents.lease_lost`·`documents.out_of_scope`
  - `documents.journal{ leaseId, seq, baseRev, text }`(mutation): 같은 lease에서 더 큰 seq만 쓴다.
  - `documents.journal_drop{ leaseId, uptoSeq }`
  - `documents.rename{ leaseId, newRelPath }`(mutation): `RENAME_NOREPLACE`. 링크 재작성은 notes가 이 위에서 한다.
- 주제 `documents.changed { key, rev? (삭제면 없음) }`: inotify로 감시한다. 자기 저장의 echo는 rev가 같으므로 화면이 버린다.
- `sys::atomic_replace(path, bytes, preserve: Mode+Eol+Bom)`: 같은 폴더 임시 파일 → fsync → `renameat2(RENAME_EXCHANGE)` → 옛 파일 삭제. 파일이 없으면 `RENAME_NOREPLACE`.
  - 임시 파일 이름은 `.devbox-tmp-<난수>`다. `sys::is_internal(name) -> bool`이 이 접두사와 `.trash`를 참으로 돌려주고, 감시·트리·notes 색인·search·링크 재작성이 모두 이 함수로 거른다(CV-F06). 데몬 시작 때 등록 root 아래 1시간 넘은 `.devbox-tmp-*`를 지운다.
- `sys::normalize_for_disk(text, eol, bom)`: 화면은 항상 `\n`·BOM 없음으로 편집하고, 디스크 형식은 읽을 때 기록해 둔다.
- 저널은 `<data>/journals/<blake3(key)>.json`이다. 문서당 최신 하나만 둔다.

**핵심 테스트:**
- `save_with_stale_base_rev_returns_conflict_with_disk_text`
- `save_preserves_mode_crlf_and_bom`
- `save_through_symlink_writes_the_target`
- `binary_non_utf8_and_oversized_files_open_read_only`
- `write_outside_registered_roots_is_refused`(`../` 경로·바깥을 가리키는 symlink 둘 다)
- `second_window_gets_read_only_until_take`
- `journal_rejects_older_seq_and_drop_is_cas`
- `external_edit_emits_changed_with_new_rev`
- `rename_noreplace_refuses_existing_target`
- `rename_moves_the_journal_to_the_new_key_and_drops_the_old_one`: dirty 저널이 있는 문서 rename → 새 키 저널 1개, 옛 키 저널 0개(CV-F03)
- `internal_temp_files_are_invisible_and_swept_on_start`: `.devbox-tmp-x`를 만든 뒤 `documents.changed`가 오지 않고, 시각을 1시간 전으로 바꾼 뒤 데몬 시작 → 지워짐
- 이식한 v0.9.0 `document.rs`·`document_publish.rs` 테스트

---

### Task 3: 화면 `DocumentStore`와 노트 편집기 바탕

**Files:**
- Create: `app/src/features/documents/{machine,store,effects,offlineJournal}.ts`, `app/src/features/documents/EditorHost.tsx`
- Test: `app/src/features/documents/machine.test.ts`, `app/src/features/documents/EditorHost.test.tsx`

**Interfaces:**
- `machine.ts`: 01-design §8.4 표를 그대로 옮긴 순수 리듀서 `reduce(doc, event) -> { doc, effects }`
  - 상태: `Closed·Loading·Clean·Dirty·Saving·Recovering·Conflict·Orphaned·ReadOnly`
  - 문서 필드: `key, leaseId, diskRev, editSeq, savedSeq, inflight?{ seq, baseRev }, journalSeq, pendingExternal[]`
  - 효과: `claim`, `save`, `journal`, `journalDrop`, `scheduleJournal(300ms)`, `scheduleSave(1s, auto 정책만)`, `showCompare`, `reloadFromDisk`
- `store.ts`(Zustand): 열린 문서 맵, `open(key, policy)`, `edit(key, text)`, `save(key)`, `close(key, choice)`, `resolve(key, "mine"|"disk"|"merged")`
- `effects.ts`: 효과를 RPC로 실행하고 결과를 이벤트로 되돌린다.
- `offlineJournal.ts`: 연결이 끊긴 동안 저널을 IndexedDB(문서당 최신 1개)에 둔다. 다시 연결되면 같은 `baseRev`로 저장을 다시 시도한다.
- `EditorHost`: CodeMirror `EditorView` 하나. 색·언어 로드는 S2 Task 15b의 `app/src/ui/code/theme.ts`·`languages.ts`를 쓴다(새로 만들지 않음). 문서를 바꿀 때 `view.setState(문서별 EditorState)`로 전환한다(AU-K1). 문서별 상태는 `Map<keyString, EditorState>`에 둔다.

**핵심 테스트(표의 행마다 하나 이상):**
- `own_save_echo_is_ignored_while_saving`
- `edit_during_save_stays_dirty_with_new_base_rev`
- `external_change_while_dirty_goes_to_conflict`
- `external_change_while_clean_reloads_as_undoable_transaction`
- `external_delete_goes_orphaned_and_save_recreates_with_expect_absent`
- `lease_lost_goes_read_only`
- `close_dirty_requires_a_choice`
- `io_error_keeps_dirty_and_retries_same_base_after_reconnect`
- `conflict_take_disk_drops_the_journal`: Conflict → `resolve("disk")` → 효과에 `journalDrop`(현재 `journalSeq`까지)이 있고, 같은 문서를 다시 `claim`하면(가짜 응답에 저널 없음) Recovering이 아님(AR-F13)
- `recovering_discard_and_close_discard_drop_the_journal`: 두 경로 모두 `journalDrop` 효과
- `repeated_conflict_errors_never_overwrite_the_buffer_or_journal`: `documents.conflict`를 두 번 연속 받아도 버퍼·저널이 그대로, 상태 Conflict(CV-F02. 분류는 오류 코드로만)
- `EditorHost`: 노트 A 편집 → B 편집 → A로 돌아와 Ctrl+Z → A의 마지막 편집만 되돌림

---

### Task 4: `crates/notes` — vault·frontmatter·링크·백링크·템플릿·첨부·렌더

**Files:**
- Create: `crates/notes/{Cargo.toml,src/lib.rs,src/vault.rs,src/frontmatter.rs,src/links.rs,src/index.rs,src/templates.rs,src/attachments.rs,src/render.rs}`
- Modify: `crates/core/src/config.rs`(`[notes] vault = "~/notes"`, `daily_folder = "daily"`, `inbox = "inbox.md"`, `templates_folder = "templates"`)

**Interfaces:**
- 메서드:
  - `notes.tree{}` → 폴더·파일(숨김·`.git`·내부 복구 파일 제외)
  - `notes.create{ folder, title, template? }`(mutation)
  - `notes.delete{ path }`(mutation, reversible: vault의 `.trash/`로 옮기고 10초 되돌리기)
  - `notes.tags{}`
  - `notes.backlinks{ path }`
  - `notes.resolve{ target }`(위키링크 → 경로 후보)
  - `notes.render{ path | text }` → 살균 HTML
  - `notes.attach{ notePath, name, bytes }`(mutation): `attachments/<blake3 앞 12자>-<이름>`에 쓰고 Markdown 링크를 돌려준다.
  - `notes.templates{}`
- 링크 해석: `[[제목]]`·`[[폴더/제목|별칭]]`·`[[제목#제목줄]]`, Markdown 상대 링크. 대소문자 무시, 같은 이름이 여럿이면 가까운 폴더를 우선한다(v0.9.0 `vault.rs` 규칙 참고).
- 링크·태그 색인은 `index.db`의 `notes_links(src, dst)`·`notes_tags`다. 테이블은 `IndexDb::ensure_domain("notes", <버전>, …)`(S2 Task 15b)로 만든다. 시작할 때와 `documents.changed`에서 갱신한다.
- 렌더는 v0.9.0 `crates/markdown`의 `pulldown-cmark` + `ammonia` 설정을 그대로 이식한다. Mermaid 코드 블록은 `<pre class="mermaid">`로 남기고 화면이 `securityLevel: "strict"`로 그린다. 원격 이미지는 막는다.
- 템플릿 변수: `{{date}}`·`{{time}}`·`{{title}}`·`{{pc}}`

**핵심 테스트:**
- `wiki_links_resolve_with_alias_heading_and_nearest_folder`
- `backlinks_update_after_external_edit`
- `render_strips_scripts_event_handlers_and_remote_images`(이식한 markdown 테스트 포함)
- `attachments_are_content_addressed_and_stay_inside_the_vault`
- `delete_moves_to_trash_and_undo_restores`
- `blank_title_falls_back_to_the_file_name`: `title: ""`·`title: "  "`·title 없음인 `idea.md` → 색인 제목 `idea`, `[[idea]]`가 이 파일로 풀림. 원본은 바뀌지 않음(AR-F10)
- `trash_and_temp_files_are_not_indexed_or_linked`: `.trash/old.md`와 `.devbox-tmp-1`이 트리·태그·백링크·`notes.resolve` 후보에 없음(CV-F06)
- vault 첫 생성 때 `.gitignore`에 `.trash/`를 넣는다(Git 공유 vault에 휴지통이 커밋되지 않게)

---

### Task 5: 여러 노트 링크 재작성 (AR-D01)

**Files:**
- Create: `crates/notes/src/rename.rs`

**Interfaces:**
- `notes.rename{ path, newPath, rewriteLinks: bool }`(mutation) → 진행 작업 ID
  1. 대상 목록과 각 파일의 `baseRev`를 먼저 확정한다(백링크 색인 + 내용 확인).
  2. 진행 저널 `<data>/notes-rename/<opId>.json`에 `{ from, to, files: [{ path, baseRev, done }] }`를 쓴다.
  3. 노트 자체를 `documents.rename`으로 옮긴다.
  4. 파일마다 링크를 고쳐 CAS 저장한다. 끝난 파일은 `done`으로 표시한다.
  5. 다 끝나면 저널을 지운다.
- 대상 문서가 어느 창에서 열려 있고 dirty이면 그 창에 `notes.rewrite_request{ key, edits }` 이벤트를 보낸다. 창이 버퍼에 적용하고 저장한다. 창이 30초 안에 응답하지 않으면 그 파일은 "열려 있어 고치지 못함" 목록에 둔다.
- CAS 충돌 파일도 목록에 둔다. 덮어쓰지 않는다.
- 데몬이 시작할 때 남은 저널이 있으면 남은 파일을 다시 시도한다. 그래도 실패한 파일은 `notify.raised`로 "링크를 고치지 못한 노트 n개"를 알리고, 누르면 목록을 보인다.

**핵심 테스트:**
- `rename_rewrites_wiki_and_markdown_links_in_all_backlinking_notes`
- `conflicting_file_is_reported_not_overwritten`
- `crash_midway_resumes_from_the_progress_journal`(파일 2개 처리 뒤 서비스 drop → 새 서비스 시작 → 나머지 처리)
- `crash_right_after_the_move_resumes_link_rewrites`: 대상 이동 직후·첫 참조 재작성 전에 서비스 drop → 다음 시작에 모든 참조가 새 경로로 고쳐지거나 목록으로 보임(AR-D01. AR은 첫 참조 재작성 뒤 종료에서 불일치를 재현했다)
- `dirty_open_document_receives_a_rewrite_request_instead_of_disk_write`

---

### Task 6: 일일 기록과 "오늘 한 일" 요약

**Files:**
- Create: `crates/notes/src/daily.rs`, `crates/cli/src/daemon/summary.rs`(조립 함수: activity·git·agents·runtime을 읽는 쪽이라 데몬에 둔다)

**Interfaces:**
- `notes.daily{ date? }`(mutation): `daily/<YYYY-MM-DD>.md`가 없으면 템플릿으로 만들고 경로를 돌려준다.
- `notes.insert_summary{ date }`(mutation): 일일 기록의 `## <PC 이름>` 아래 `### 오늘 한 일` 블록을 만들거나 바꾼다.
  - 블록은 `<!-- devbox:summary:<pc> -->` … `<!-- /devbox:summary -->` 표시로 감싸 다른 PC의 블록과 사용자가 쓴 글을 건드리지 않는다.
  - 내용:
    - 활동 상위 앱·프로젝트 시간(Task 8)
    - 등록 프로젝트의 그날 커밋(`git log --since --author=<user.email>`)
    - 그날 닫힌 에이전트 작업(제목·결과·PR 링크)
    - 실패한 실행·예약
- CLI `devbox note add [--daily] <text>`는 inbox 또는 일일 기록 끝에 덧붙인다(MCP·스크립트용).

**핵심 테스트:**
- `summary_block_is_replaced_in_place_and_other_pc_blocks_survive`
- `user_text_outside_markers_is_preserved`
- `summary_lists_closed_agent_tasks_and_commits_of_the_day`(가짜 서비스 주입)

```bash
# 묶음 T 끝
# 묶음 T 끝: PROGRESS.md의 묶음 T 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s3-knowledge
```

---

### Task 7: `crates/search` — 노트 FTS·코드·파일 이름·저장된 검색

**Files:**
- Create: `crates/search/{Cargo.toml,src/lib.rs,src/fts.rs,src/code.rs,src/files.rs,src/saved.rs}`

**Interfaces:**
- `index.db`의 search 테이블은 `IndexDb::ensure_domain("search", <버전>, …)`(S2 Task 15b)로 만든다. 버전이 바뀌면 search 테이블만 다시 만들고 다시 색인한다(백업 없음). 다른 도메인(S5의 `api`)은 건드리지 않는다.
- `search.query{ text, scopes: [notes|code|files], projectIds?, regex?, caseSensitive?, limit }`(stream): 결과를 범위별로 나눠 보낸다(노트 FTS 먼저, 코드 grep은 진행에 따라).
  - 노트: FTS5(`unicode61` + 한글 2-gram 보조 열), 일치 부분 강조 조각
  - 코드: `ignore`(gitignore 존중) + `grep-searcher`. 파일 1 MiB 초과·이진은 건너뛴다.
  - 파일 이름: 등록 프로젝트의 `git ls-files` 캐시 + 퍼지 점수
  - 사용자 제외 규칙: `config.toml [search] exclude = ["**/dist/**"]`
- 결과 묶음마다 `rev`를 단다. 화면은 마지막 성공 결과와 선택을 유지하고, rev가 바뀐 경우에만 다시 조회한다(CV-F07).
- `search.saved{}`·`search.save{ name, query }`·`search.unsave{ id }`
- 같은 `search` crate를 S4 편집기의 프로젝트 검색도 쓴다.

**핵심 테스트:**
- `korean_substring_matches_in_note_fts`("데몬재시작" 안의 "재시작")
- `code_search_respects_gitignore_and_user_excludes`
- `search_index_rebuilds_only_its_domain_when_version_changes`(`api_*` 테이블이 그대로)
- `cancel_stops_the_code_walk`(스트림을 닫으면 탐색 중단)
- `observed_time_only_changes_do_not_bump_the_rev`: 감시기의 관찰 시각만 바뀐 갱신 3회 → 결과 묶음 rev 그대로, `search.changed` 없음(01-design §5.5 R8, CV-F07·AR-F11)
- `blank_title_note_is_found_by_file_name`: 빈 title의 `idea.md`를 이름 검색 `idea` → 1건(AR-F10)
- `trash_and_temp_files_are_not_searched`: `.trash/`·`.devbox-tmp-*` 안의 일치는 결과에 없음(CV-F06)

---

### Task 8: `crates/activity` — 이식·묶어 쓰기·규칙·저장 실패

**Files:**
- Create: `crates/activity/{Cargo.toml,src/lib.rs,src/model/{sessionizer,privacy,idle,attribution}.rs,src/store.rs,src/rules.rs,src/export.rs}`

**Interfaces:**
- `model/*`: v0.9.0 `crates/activity-engine/src/core/{sessionizer,privacy,idle,attribution}.rs`와 테스트를 이식한다. AU-K2 수정은 v0.9.0에 이미 들어 있다. 다음 테스트가 그대로 통과해야 한다: `idle_retroactively_excludes_all_last_input_tail_intervals`(마지막 입력 뒤 idle 구간 제외), `explicit_pause_keeps_actual_pause_time_and_wall_clock_reversal_never_makes_negative_spans`(일시중지·시계 역행 경계).
- `activity.ingest{ events: [{ at, app, title?, idle?, dropped? }] }`(앱 전용 mutation)
  - 서버에서 규칙을 다시 적용한다(이중 방어가 아니라, 앱 캐시가 오래됐을 때 대비).
  - 1초 또는 100건 단위로 묶어 쓴다.
- 저장 실패(디스크 부족·DB 오류):
  - 확정 세션은 메모리 재시도 큐에 둔다(상한 10,000건, 넘치면 가장 오래된 것부터 버리고 버린 수를 센다).
  - 상태 `activity.status { state: ok|storage_failed, pending, dropped, lastError }`를 발행한다.
  - 다음 쓰기가 성공하면 큐를 순서대로 비운다(CV-F08, AR-F05: 두 번째 INSERT가 실패해도 뒤 세션이 사라지지 않음).
- `activity.rules{}`·`activity.set_rules{ rules }`(mutation): 프로세스 제외·제목 마스킹·치환·정규식. 컴파일에 실패한 규칙이 하나라도 있으면 저장을 거부하고 위치를 돌려준다.
- `activity.day{ date }`·`activity.range{ from, to }`: 앱·프로젝트·제목별 시간, 시간대별 막대
- `activity.export{ from, to, format: csv|json }` → 파일 경로(데이터 폴더 `exports/`)
- 프로젝트 귀속: 창 제목·앱과 등록 프로젝트 이름·경로를 맞춘다(이식한 `attribution.rs`).

**핵심 테스트:**
- 이식한 모든 테스트 + idle 결함 재현 테스트
- `storage_failure_keeps_later_sessions_and_reports_status`(주입 가능한 저장 경계로 두 번째 쓰기 실패 → 세 번째 성공 → 세 세션 모두 저장)
- `invalid_rule_regex_is_rejected_with_position`
- `ingest_reapplies_rules_server_side`

---

### Task 9: Windows 활동 수집기 (`crates/win`)

**Files:**
- Create: `crates/win/src/activity.rs`, `crates/win/src/privacy.rs`(규칙 적용, 플랫폼 독립)
- Modify: `app/src-tauri/src/lib.rs`
- Create: `app/src-tauri/src/activity.rs`, `app/src/features/activity/Consent.tsx`(동의·일시중지 화면)

**Interfaces:**
- `#[cfg(windows)] activity::start(sink)`
  - `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` + 제목 변경(`EVENT_OBJECT_NAMECHANGE`, 전면 창만) + 5초마다 `GetLastInputInfo`로 idle 판정
  - 같은 앱·제목이 이어지면 이벤트를 만들지 않는다.
- idle 판정의 입력 시각 차이는 v0.9.0 `idle::duration_from_input_ticks`(`LASTINPUTINFO`의 하위 32비트끼리 `wrapping_sub`)를 이식해 계산한다. Windows 가동 49.7일마다 틱이 한 바퀴 돌아도 idle 하루로 잡히지 않게 하기 위해서다.
- `privacy::apply(rules: Option<&CompiledRules>, event) -> Event`: 규칙이 `None`(못 받음·컴파일 실패)이면 제목을 지운다(fail-closed).
- 앱 쪽 동작:
  - 연결이 끊긴 동안 최대 1,000건을 메모리에 보관하고, 넘치면 버린 수를 센다. 다시 연결되면 `activity.ingest`로 보내고 `dropped`를 함께 보낸다.
  - 동의 전에는 수집기를 시작하지 않는다. 일시중지(트레이 메뉴·설정)에는 보내지 않는다. 동의를 철회하면 대기열을 비운다.
  - 첫 실행 때 v0.9.0 Knowledge의 활동 수집이 켜져 있으면(`%LOCALAPPDATA%` 아래 v0.9.0 설정 파일로 확인) "한쪽만 켜 두세요"를 안내한다(01-design §6.2).
- 트레이 메뉴에 "활동 기록 일시중지/다시 시작"을 추가한다.

**핵심 테스트:**
- `missing_or_broken_rules_drop_titles`(플랫폼 독립, Linux CI)
- 이식: `input_ticks_share_the_low_32_bit_domain_across_uptime_wrap`(플랫폼 독립)
- `offline_buffer_caps_at_1000_and_counts_drops`(플랫폼 독립 버퍼 타입)
- Windows 훅 동작은 Task 12 실사용 확인

```bash
# 묶음 V 끝
# 묶음 V 끝: PROGRESS.md의 묶음 V 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s3-knowledge
```

---

### Task 10: 노트 섹션 화면

**Files:**
- Create: `app/src/features/notes/{NotesPage,NoteTree,TagList,NoteEditor,Preview,Backlinks,CompareView,RecoveryView,wikiComplete,pasteImage}.tsx|ts`
- Create: `app/src/ui/Tree.tsx`, `app/src/ui/Timeline.tsx`
- Create: `app/e2e/notes.spec.ts`

**Interfaces:**
- 목록: 폴더 트리(끌어서 옮기기 = `notes.rename`) · 태그 · 활동(일·주, Task 11)
- 본문:
  - `NoteEditor`(Task 3 `EditorHost` + Markdown 언어 + `[[` 자동완성 `wikiComplete` + 이미지 붙여넣기 `pasteImage` → `notes.attach`)
  - [미리보기 분할] 토글
  - 오른쪽 백링크 패널(접기)
- 저장 상태 표시: "저장됨 hh:mm"·"저장 중"·"연결 끊김 · 이 기기에 임시 저장"·"충돌 · 비교 필요"
- `CompareView`: 왼쪽 내 버퍼, 오른쪽 디스크. [내 것 유지][디스크 것][직접 병합]
- `RecoveryView`: 저널과 디스크 비교. [복원][버림]
- 다른 창이 편집 중이면 읽기 전용 + [여기로 가져오기](`documents.take`)
- 단축키: Ctrl+Alt+D(일일 기록), Ctrl+N(노트 섹션에서 새 노트, 편집기 밖), Ctrl+S(지금 저장)

**핵심 테스트:**
- `wikiComplete` 후보 정렬 단위
- `pasteImage`가 붙여 넣은 이미지를 첨부로 올리고 링크를 넣는지 단위
- E2E `notes.spec.ts`(3개)
  - 새 노트 → 입력 → 1초 뒤 "저장됨" → 디스크 파일 내용 확인
  - 외부에서 파일 수정(테스트가 직접 씀) → 편집 중이면 비교 화면, 아니면 내용 갱신
  - 노트 이름 바꾸기 → 다른 노트의 `[[링크]]`가 새 이름으로

---

### Task 11: 통합 검색·빠른 캡처·활동 보기·노트에서 에이전트 작업·MCP

**Files:**
- Create: `app/src/features/search/SearchTab.tsx`, `app/src/features/capture/{CaptureWindow,capture}.tsx|ts`, `app/src/features/activity/{ActivityDay,ActivityWeek,RulesEditor}.tsx`
- Modify: `app/src-tauri/src/shortcuts.rs`(빠른 캡처 전역 단축키), `app/src-tauri/src/tray.rs`, `crates/cli/src/mcp.rs`
- Create: `app/e2e/search.spec.ts`

**Interfaces:**
- 검색 탭(Ctrl+Shift+F, 터미널 포커스면 터미널 검색):
  - 범위 칩(노트·코드·파일 이름), 정규식·대소문자, 결과 그룹, 저장된 검색
  - 결과를 누르면 노트는 노트 섹션, 코드는 S4 전까지 외부 편집기·읽기 전용 미리보기로 연다.
- 빠른 캡처:
  - 전역 단축키 Ctrl+Alt+N(개발 빌드 Ctrl+Alt+Shift+N) → 작은 창(`/popout/capture`, 520×220, 항상 위)
  - Enter로 inbox 끝에 `- hh:mm 내용`을 덧붙이고 닫는다. Shift+Enter는 줄바꿈, Esc는 닫기(내용이 있으면 초안 보존).
  - 트레이 메뉴 "빠른 캡처"에서도 연다.
  - SC2 기준(300ms)을 같은 방식으로 잰다.
- 활동:
  - 노트 목록의 활동 탭(일·주 막대 + 앱·프로젝트 표)
  - 설정 › 활동(동의·일시중지·규칙 편집기·내보내기·저장 상태 배너 "저장 중단 · n건 대기")
- 노트에서 에이전트 작업: 선택 영역 또는 노트 전체 → [에이전트 작업 만들기] → S1 `NewTaskDialog`에 지시문으로 채워 연다.
- MCP 도구 추가:
  - `devbox_notes_search{ text }`(읽기)
  - `devbox_note_read{ path }`(읽기, vault 안만)
  - `devbox_note_add{ text, daily? }`(쓰기, `[mcp] write = true`일 때만)

**핵심 테스트:**
- `SearchTab`이 갱신 중에도 이전 결과·선택을 유지하는지 단위(CV-F07)
- `late_day_response_does_not_override_tracking_status`: 활동 보기에서 `activity.day` 응답을 늦게 풀고 그사이 `activity.status`가 "수집 중"으로 바뀜 → 버튼은 "일시중지"를 유지(AR-F12. `activity.day`와 `activity.status`는 별도 query)
- 캡처 창 키 처리 단위(IME 조합 중 Enter 무시)
- MCP `devbox_note_read`가 vault 밖 경로를 거부
- E2E `search.spec.ts`(2개): 노트 본문 한글 부분 검색, 코드 검색 결과 열기

```bash
# 묶음 W 끝
# 묶음 W 끝: PROGRESS.md의 묶음 W 행과 현재 위치를 고쳐 커밋한 뒤 push한다. PR·CI는 없다(00-roadmap §3)
pnpm check && git push origin v1/s3-knowledge
```

---

### Task 12: S3 완료

- [ ] **Step 1: 규모 통계:** `bash scripts/loc.sh 120000` 결과를 PR 본문에 쓴다. 넘으면 S4 전에 단순화 후보를 사용자와 정한다(01-design SC8).
- [ ] **Step 2: 실사용 확인(사용자, 10개)** — 설치 파일은 `bash scripts/dogfood.sh v1/s3-knowledge`로 받는다.
  1. 노트를 쓰고 1초 뒤 자동 저장, 앱을 닫았다 열어도 그대로
  2. VS Code에서 같은 노트를 고치면 devbox가 비교를 보여 주고 덮어쓰지 않는다.
  3. 노트 이름을 바꾸면 다른 노트의 링크가 따라 바뀐다.
  4. 이미지를 붙여 넣으면 첨부로 저장되고 미리보기에 보인다.
  5. Ctrl+Alt+D로 일일 기록 → [오늘 한 일 넣기]에 커밋·닫힌 에이전트 작업이 보이고, 다른 PC 블록은 그대로다.
  6. 빠른 캡처(전역 단축키)로 한 줄을 남기면 inbox에 들어간다.
  7. 활동 기록에 동의하면 하루 보기에서 앱·프로젝트 시간이 보이고, 일시중지 동안은 비어 있다.
  8. 마스킹 규칙을 넣으면 해당 제목이 가려진다.
  9. Ctrl+Shift+F로 노트와 코드를 한 번에 찾는다.
  10. 노트 일부를 골라 에이전트 작업을 만든다.
- [ ] **Step 3: 상태 갱신·PR**

```bash
sed -i 's/^- 상태: 계획(과제 수준) · 미착수 · 시작 조건: S2 완료.*/- 상태: 완료(S3 PR 머지, 실사용 확인 10\/10)/' docs/superpowers/plans/2026-10-08-devbox-v1/06-s3-knowledge.md
# PROGRESS.md: S3 행·PR 표의 S3 행·현재 위치(다음: S4-1)를 고친다
git add docs && git commit -m "docs(plan): mark S3 complete"
git push origin v1/s3-knowledge && gh pr create --base main --head v1/s3-knowledge --title "feat: notes, activity and search (S3)" --body "S3 기록 전체(Task 1–12), 규모 통계, 실사용 확인 10개 결과.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```
