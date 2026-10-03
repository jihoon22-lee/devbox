# Knowledge·Agent 구체 개선 계획 — 구현 전 검토

> **For agentic workers:** [00 실행 규칙](00-roadmap.md)·[01 제품 계약](01-product-contract.md)·[06 수용 매트릭스](06-acceptance-release.md)를 함께 적용한다. 선행/파일 배정 충돌 시 00의 통합 표가 우선한다.

**Goal:** Knowledge 문서·복구·검색·활동의 정확성과 실패 복구를 완성한다.

**Architecture:** 기존 제품/native 소유권·revision·typed IPC를 유지하고 실제 UI 전이를 검증한다. 신규 제안 파일/테스트는 아래 과제에서 생성한다.

**Tech Stack:** 현 Rust·React 19·TypeScript·pnpm 9·CSS·Windows WebView2.

**Spec:** 01. **Global Constraints / Review Focus:** 01의 전체 제약과 5개 검토 관점을 상속한다.

**과제 실행 순서:** 각 하위 과제에서 (1) 명시한 assertion의 RED 작성, (2) 실제 실패 원인 확인, (3) 아래 계약에 맞춘 최소 구현, (4) 동일 좁은 검사 GREEN, (5) 과제 커밋. 명령은 실행 계획이며 현재 PASS 기록이 아니다. 직접 pnpm/cargo 명령은 반드시 `python3 .github/scripts/verify-resources.py --` 아래에서 실행한다. 통합 완료 후 최종 검증은 00을 따른다. 작업마다 전체 검증·CI를 반복하지 않는다.


2026-10-03 / 저장소 `/home/jihoon/projects/devbox` / 통합 과제 R01·R12·R13, 연계 R06·R15.

이 문서는 계획이다. 제품·테스트·Git·CI를 변경하지 않았고 검증을 실행하지 않았다. 기존 감사 K1–K7는 `/home/jihoon/projects/devbox-audit-2026-10-03/report.md`의 판정을 그대로 사용한다. 추가로 코드를 읽은 결과와 개선 제안은 다음처럼 구분한다. Agent의 독립적인 신규 고영향 결함은 이번 읽기에서 확정하지 못했다. 이를 Agent 정상 보증이나 Windows 수용 PASS로 표현하지 않는다.

## 분류와 작업 배치

| 구분 | 항목 | 과제/작업 |
|---|---|---|
| 이미 확정 | K1 다른 노트 Undo→현재 파일 autosave, K5 삭제된 원본의 저널을 UI에서 사용할 수 없음 | R01 |
| 이미 확정 | K3 regex의 FTS 전처리가 정상 후보를 제외 | R12 |
| 이미 확정 | K2 idle 시간 과대집계, K4 handoff 재생성 불능/현재 digest 오용, K6 종료 안내와 실제 owner 불일치 | R13 |
| 이미 확정, 타 작업 소유 | K7 최소 창 편집 불가 | R06; R01 소비자 수용만 공유 |
| 새 코드확정 N03 | Timeline 이전/다음이 동일 날짜를 다시 조회 | R13 |
| 새 코드확정 N04 | idle threshold 저장 오류를 native와 UI가 모두 숨김 | R13 |
| 개선 제안 U1 | 종료의 `버리고 종료`와 저널 보존·삭제 의미를 명시적으로 분리 | R01 |
| 개선 제안 U2 | 새 프로세스에서 vault가 offline인 경우에도 신뢰 가능한 로컬 저널을 확인·복사할 복구 화면 | R01 |
| 개선 제안 U3 | 저장 검색을 native/launcher 계약과 같은 source·mode·regex 의미로 실행 | R12 |
| 개선 제안 U4 | watcher/owner 변화, reference 만료, root 조작 실패를 능동적으로 갱신하는 Search UX | R12 |
| 개선 제안 U5 | 현재 수집 상태·동의·privacy 적용 범위·background owner를 한 흐름에서 확인 | R13 |
| 실기 검증 공백 | 설치본 최초 준비/재연결/crash/tray stop/update/단일 writer/portable 대조 | R15; R13/R12에서 fixture/assertion 제공 |

N03 증거: `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.tsx:757`의 `shift()`는 day/week/month만 날짜를 변경한다. Timeline도 같은 이전/다음 control을 렌더하므로 날짜 값은 그대로다. 새 Date 객체 때문에 재조회만 생긴다. 코드확정이며 실기 실행은 하지 않았다.

N04 증거: `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/components/ActivitySettings.tsx:208`는 먼저 표시값을 바꾸고 `void setIdleThreshold()`를 호출하며 catch/ack가 없다. `/home/jihoon/projects/devbox/crates/activity-engine/src/commands/tracking.rs:313`는 DB `set_setting()` 후 항상 Ok; `/home/jihoon/projects/devbox/crates/activity-engine/src/core/db.rs:249`는 execute 오류를 버린다. 읽기 전용/쓰기 실패 DB에서 성공처럼 보이고 재실행 후 기존값으로 돌아가는 경로가 코드로 확정된다. 화면 거짓 성공은 P2, 새로운 실제 DB 실패 재현은 미실행이다.

U3는 새 결함으로 세지 않는다. `/home/jihoon/projects/devbox/packages/knowledge-features/src/search/App.tsx:1120`는 저장 검색이 검색어와 필터만 저장한다고 명시하고 DTO도 같다. 다만 현재 regex/content/notes 상태에서 저장·불러오면 의미가 현재 UI 상태에 의존하여 혼동되므로 제한과 복원 규칙을 구체화한다.

## 선행관계와 완료 조건

- R01을 먼저 수행한다. P1 파일 내용 오염을 막는 최우선 과제다.
- R12는 R01 변경 통합 뒤, R13은 통합 표에 따라 R01/R06 뒤 진행한다. R12와 Knowledge.tsx·generated binding·lifecycle harness를 함께 수정할 때는 해당 파일 소유를 직렬화하고, 서로 필요한 API 변경이 생기면 R12 변경 통합 뒤로 배치한다. 공용 feature/generated binding·Knowledge lifecycle·Windows harness를 겹쳐 변경하지 않는다. 독립 분석·검토는 병행할 수 있고 필요한 선행 변경이 통합 브랜치에 반영되면 후속 구현을 시작한다. 작업별 main 머지·CI 대기를 만들지 않는다.
- R06은 K7 CSS·responsive 구조 소유자다. R01은 R06과 `MarkdownEditor` document identity prop 계약만 합의한다. R01에 전체 shell/반응형 작업을 복제하지 않는다.
- 공통 shell S1과 최초 활성화/route 계약 수정이 설치본 UI 수용의 선행조건이다. native 권한을 넓혀 임시로 우회하지 않는다. R15의 최종 설치본 수용은 R01/R12/R13 및 bootstrap 관련 선행 작업 통합 후 수행한다.
- 결함별 최소 재현→수정→직접 영향 테스트→과제 커밋 순서로 진행한다. 모든 작업과 문서의 통합 완료 뒤 최종 검증·Windows/WSL·Biome를 모으고 단일 PR의 CI 통과 후 main에 머지한다. 작업별 전체 검증과 CI는 실행하지 않는다. 이 계획의 명령은 실행 예정이며 현재 PASS 근거가 아니다.
- 기본 편집/복구/검색/동의/연결을 사용자가 손으로 테스트하도록 넘기지 않는다. 담당 구현 에이전트가 로컬 Windows/독립 Windows VM fixture와 자동 UI/native assertion을 실행하고 근거 artifact를 남긴다. 환경 확보 실패는 명시적 미실행으로 기록하고 최종 격리 수용에서 보충하며 출시 전에는 반드시 해결한다. 무관한 작업의 통합을 막거나 사용자의 수동 확인으로 대체하지 않는다.

## 작업 R01 — 노트 문서 수명과 복구 보존

### 목표와 고정 계약

1. `documentGeneration`은 성공한 open/reopen/source 교체마다 증가하고 일반 본문 편집에서는 증가하지 않는다. editor의 history·selection·비동기 작업 수명은 이 generation에 묶는다. 같은 path 재열기도 이전 source의 undo를 섞지 않는다. 기존 `sourceVersion`은 edit에도 증가하므로 그대로 React key로 쓰지 않는다.
2. 기본 선택은 문서 전환 때 새 CodeMirror state/history를 생성하는 방식이다. 개별 노트 undo 보존은 이번 작업에 추가하지 않는다. 외부 본문 sync는 일반 사용자 edit/history와 구분하되 `addToHistory:false` 하나만으로 해결됐다고 판단하지 않는다. documentGeneration 검증을 image paste 등 기존 async path guard에 결합한다.
3. `write(path,content,expectedRevision)`는 같은 document generation에서 캡처한 세 값만 전달한다. 과거 editor callback, A의 지연 paste/save 완료가 B를 수정할 수 없다. 저장 conflict와 ordered writer 계약을 유지한다.
4. 복구본 열람은 원본 파일 open 성공에 의존하지 않는다. 본문 미리보기·명시적 복사 가능; 복사는 사용자 클릭 시만 clipboard에 기록한다. 삭제된 노트 재생성은 missing revision CAS 후 명시적 확인으로 실행한다. 그 사이 원본 생성/수정은 conflict, 덮어쓰기 금지. `다른 이름으로 복원`은 기존 native create-new/no-clobber 경로를 재사용한다.
5. journal은 성공한 publish 확인 또는 명시적 영구 삭제 뒤만 제거한다. 실패·취소·offline·충돌·프로세스 종료는 복구본을 유지한다. 적용/복사만으로 journal 삭제를 완료하지 않는다. 이미 복원한 내용을 재표시할 때도 중복 mutation을 만들지 않는다.
6. 종료 정책 제안 U1: 현재 `버리고 종료`를 `저장하지 않고 종료(복구본 유지)`로 명확히 바꾸고, 영구 폐기는 별도 확인으로 제공한다. 영구 폐기를 고르면 autosave/journal debounce를 먼저 정지·drain, 진행 writer 완료 후 현재 generation의 저널만 삭제하고 닫는다. 삭제 실패 시 닫지 않는다. 다른 vault/노트의 저널은 보존한다. 이는 기존 discard가 의도적으로 안전 복구를 남길 가능성을 인정한 정책 선택이며 현재 동작을 데이터 손실 결함으로 단정하지 않는다.
7. U2는 최초 offline 읽기만 확장한다. native가 검증·저장한 active vault binding의 canonical recovery scope를 재사용해 로컬 저널을 읽는다. 이 scope는 source 파일에 쓸 권한이 아니다. renderer가 root/path를 임의 지정하여 다른 vault 본문을 가져오는 API를 만들지 않는다. 기존 journal과 일치하는 신뢰 가능한 scope가 없으면 자동 결합을 하지 않고 복구 확인 필요 상태로 남긴다. 원본 접근 회복 전 write/recreate는 disabled, 로컬 본문 보기·복사만 가능하다. 기존 foreign-vault count와 별도 삭제 확인을 유지한다.

### 정확한 변경 예정 파일

수정:
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/noteDocument.ts` — 열린 문서 generation와 복구 상태/restore transition.
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/MarkdownEditor.tsx` — generation 교체 시 EditorState/history/selection 재설정, stale callback guard.
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/App.tsx` — generation 전달, 복구 적용·종료 journal drain 연결.
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/journal.ts`, `autosave.ts`, `lifecycle.ts` — keep/discard 명시적 종료 계약과 generation-safe drain.
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/RecoveryControls.tsx`, `RecoveryBanner.tsx` — missing/offline 미리보기·복사·명시 복원·실패 안내.
- `/home/jihoon/projects/devbox/apps/devbox-knowledge/src/QuitGuard.tsx`, `Startup.tsx`, `RecoveryBoundary.tsx` — keep/discard 문구와 첫 실행 offline 복구 진입.
- `/home/jihoon/projects/devbox/crates/knowledge-vault-engine/src/core/document.rs`, `commands/docs.rs`, `core/journal.rs`, `commands/journal.rs`, `api.rs`, `component.rs` — 기존 missing CAS 재사용, 로컬 recovery scope 읽기와 restore 응답 계약.
- `/home/jihoon/projects/devbox/apps/devbox-knowledge/src-tauri/src/ipc/notes.rs`, `core/vault_binding.rs` — source 권한과 recovery read-only scope 분리. `/crates/knowledge-stores`는 새로운 active-binding 영속 필드가 필요할 때만 검증된 manifest schema 변경; 필수로 별도 DB를 만들지 않는다.
- 기존 `MarkdownEditor.test.tsx`, `noteDocument.test.ts`, `App.document.test.tsx`, `App.journal.test.tsx`, `journal.test.ts`, `autosave.test.ts`(동일 notes 하위), `/apps/devbox-knowledge/src/QuitGuard.test.tsx`, `Startup.test.tsx`, `RecoveryBoundary.test.tsx`, native `core/journal_tests.rs`, `commands/journal_tests.rs`.
- `/home/jihoon/projects/devbox/.github/scripts/windows-knowledge-lifecycle.mjs` — 현재 합성 vault lifecycle에 실제 UI undo/save와 삭제/재열기 복구 연결.
신규:
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/notes/components/RecoveryControls.test.tsx` — 원본 없이 저널 사용하는 UI contract 집중 테스트.
- `/home/jihoon/projects/devbox/.github/scripts/windows-knowledge-document-recovery.mjs`, `windows-knowledge-document-recovery.test.mjs` — 기존 hosted fixture에서 호출하는 자동 수용 module/fixture contract.
Generated DTO는 native exporter로 생성; generated TS를 손으로 수정하지 않는다. `/home/jihoon/projects/devbox/.github/scripts/check-generated-bindings.sh`와 각 제품 `--test typescript` export는 PR 끝의 기존 검증에 묶으며 별도 전체 테스트를 반복하지 않는다. Native 단순 read만 필요한 경우 신규 restore 명령을 만들지 않고 기존 read/write/create 계약을 조합한다.

### 먼저 실패해야 하는 회귀 assertion

- A edit→B open→Ctrl+Z→1500ms 경과: B 본문에 A sentinel 없음; `writeFile`의 B 인수에 A 본문 없음; Undo를 마친 뒤 B에서 실제 edit하면 B path/B revision/B edited content만 write. `expect(writes.find(w=>w.path==='B.md')?.content).not.toContain('A_SENTINEL')`.
- 같은 B path의 native source 교체→Undo: 이전 source의 내용을 복원/자동 저장하지 않음. 정상 같은 문서의 type→Undo→Redo는 정상이고 history 재설정 때문에 매 keystroke Undo가 불능이 되지 않음.
- A paste/image native 요청 지연→B 전환→A 응답: B의 편집 callback·본문·journal unchanged. B 전환 전 A writer 성공은 A에만 기록.
- native `content=null` + journal 있음: 본문 read-only preview가 보여야 함; Copy 가능; 원본은 자동 생성되지 않음. 명시 recreate 성공만 clear journal. recreate 사이 누군가 파일 생성하면 conflict, 새 파일 원문/저널 모두 유지.
- 원본 읽기 실패·disk changed·write 실패·영구 discard 실패: journal byte/content가 보존됨, 성공 표시와 quit 승인 없음. 현 generation 변경 중 이전 복구 응답이 도착하면 새 문서에 적용하지 않음.
- fresh process + source offline + persisted validated scope: 로컬 preview/copy 가능, write/recreate 불가능; foreign-vault journal 본문 노출 없음; 다시 연결한 뒤 CAS 재검증.
- keep-and-close 후 재시작: 복구본 유지. permanent discard 후 재시작: 해당 복구본 없음. 완료 뒤 늦은 timer가 삭제한 복구본을 다시 만들지 않음. 다른 노트 저널 byte-identical.

### 좁은 검증 명령(각 과제에서 필요한 묶음만 선택)

```bash
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/knowledge-features exec vitest run src/notes/components/MarkdownEditor.test.tsx src/notes/noteDocument.test.ts src/notes/App.document.test.tsx
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/knowledge-features exec vitest run src/notes/components/RecoveryControls.test.tsx src/notes/App.journal.test.tsx src/notes/journal.test.ts src/notes/autosave.test.ts
python3 .github/scripts/verify-resources.py -- pnpm --filter devbox-knowledge exec vitest run src/QuitGuard.test.tsx src/Startup.test.tsx src/RecoveryBoundary.test.tsx
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-knowledge-vault-engine --lib journal
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-knowledge-vault-engine --lib document
node --test .github/scripts/windows-knowledge-document-recovery.test.mjs
```

### Windows 수용 기준

로컬 Windows의 소유 disposable fixture 또는 독립 VM에서 UI tree 클릭→실제 CodeMirror 키 입력→다른 노트 열기→Undo/Redo→native writer/disk read로 확인한다. 호출 mock 결과만으로 실제 저장 PASS를 대체하지 않는다. autosave OFF/ON, same-path reopen, 외부 변경 conflict, crash 후 삭제된 원본 저널 재생성/다른 이름 복원/복사, UI retry, vault 전환을 자동 수행한다. 실제 disk B에 A sentinel이 없어야 하고 원본/복구본 hash와 실패 시 보존 결과를 artifact에 남긴다. R06가 제공하는 720×480/900px 구조에서 edit·restore·quit 버튼과 keyboard focus를 검증한다. WSL offline는 기존 owned hosted WSL fixture를 이용하고 로컬 기존 배포판을 멈추지 않는다.

## 작업 R12 — 검색 의미, offline 결과와 색인 수명

### 목표와 고정 계약

1. K3: regex expression을 기호 제거 literal FTS로 바꾸지 않는다. native query에 `nameCandidates`라는 명시적 후보 선택 의미를 추가해 등록된 허용 scope에서 filename/title metadata를 deterministic order로 열거한다. literal/content query는 기존 FTS 경로 유지. regex grammar/500ms worker deadline은 기존 JavaScript 계약 유지.
2. 이번 기본 계약은 후보 최대 2,000/기존 job 1,500ms·4MiB 상한이다. 2,001번째 후보 존재/timeout이면 `partial=true`와 `candidateLimit/timeout` 원인을 반환하고 UI에 `일부 후보에서 검색`으로 명시한다. 한정되지 않은 전체 이름을 검색한 것처럼 0건을 표시하지 않는다. notes도 regex에서 기존 projection limit100에 숨게 두지 않고 같은 명시 상한/partial 계약을 사용한다. 임의 disk 전수 순회나 bounded worker 제거 금지. 안정적 prefix 추출을 통한 최적화는 이 작업 필수가 아니다.
3. native 후보 요청→worker filtering→snapshot replacement를 한 query generation으로 묶는다. 이전 source/owner/vault/project의 후보·worker 응답이 새 화면에 섞이지 않는다. 변경된 후보 배열에는 새 match computation을 사용한다. native/reference job cancel과 worker terminate를 모두 처리하고 오래된 blocking OS call permit을 실제 반환 전 재사용하지 않는다.
4. U3: 현재 저장 DTO 및 launcher의 literal files/name 의미를 유지한다. Save는 `files+name+literal`에서만 허용하고 제한 이유를 보여준다. Load는 source/files, mode/name, regex/off와 저장 filter/query를 원자적으로 적용한다. notes/content/regex를 저장하고 싶은 요청은 별도 v2 설계로 남긴다. 이번에 하위 protocol/schema 전체를 넓히지 않는다.
5. U4: Search가 보일 때 root/index status를 bounded interval(2초 권장) 또는 native invalidation으로 갱신; 색인 중 기존 500ms 사용. route hidden이면 UI polling/job을 cancel하되 Agent의 watcher/index owner를 끄지 않는다. 복귀·Agent reconnect·project epoch 변경 시 meta refresh+현재 query 재평가. 현재 `status.indexing`이 false면 polling이 끝나는 로직(App.tsx:268)만으로 offline/reconnect 상태 갱신을 보증하지 않는다.
6. `cancel_requested`는 취소 요청 접수와 worker 종료를 구분한다. 마지막 성공 index/부분 commit을 보존하며 offline·불완전 scan으로 삭제를 추정하지 않는 기존 engine 계약을 유지한다. Root remove는 per-root busy+오류 catch+ack 뒤 chip 제거; 실패하면 chip/현재 index를 유지. 현재 root 삭제(App.tsx:1135)의 `void removeRoot(...).then(loadMeta)`에는 실패 표시가 없다.
7. reference는 180초 TTL/정확한 source generation/object lease 권한을 유지한다. stale/expired open은 path fallback 금지, 명시 재조회→사용자 재선택. 응답 실패와 실제 검색 0건/unsupported/partial/offline 상태를 구분한다. privacy `indexContent=false`와 sensitive skipping 정책이 있는 root는 regex 이름 후보 기능 때문에 content extract를 시작하지 않는다.

### 정확한 변경 예정 파일

수정:
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/search/App.tsx`, `api.ts`, `types.ts`, `lib/regex.ts`, `lib/regex.worker.ts` — 후보 query, recipe UI 제한/원자 load, activity-aware polling/실패 안내.
- `/home/jihoon/projects/devbox/apps/devbox-knowledge/src/Knowledge.tsx` — Search `active`/reconnect invalidation 전달. R13의 Activity visibility 변경과 충돌하도록 동시에 개발하지 않는다.
- `/home/jihoon/projects/devbox/apps/devbox-knowledge/src-tauri/src/ipc/search.rs`, `search.rs`, `core/source_search.rs` — strict typed candidate mode, truthful partial reason, existing cancel/reference lease 유지.
- `/home/jihoon/projects/devbox/crates/content-index-engine/src/core/db.rs`, `component.rs`, `api.rs`, `commands/search.rs` — filters/deepest root/scope를 유지하는 bounded filename enumeration, installed Agent에도 같은 query 계약.
- `/home/jihoon/projects/devbox/crates/knowledge-vault-engine/src/core/db.rs`, `component.rs` — bounded note-title/name candidate projection. 구현 중 파일 위치가 실제 projection 소유자인지 확인하고 기존 module 안에 추가한다.
- 기존 `/packages/knowledge-features/src/search/App.test.tsx`, `api.source.test.ts`, `lib/regex.test.ts`, native source_search/db 테스트.
- `/home/jihoon/projects/devbox/.github/scripts/windows-knowledge-lifecycle.mjs`, `windows-agent-collectors.mjs` — installed/portable 조회·background file event 결과 연결.
신규:
- `/home/jihoon/projects/devbox/.github/scripts/windows-knowledge-search-lifecycle.mjs`, `windows-knowledge-search-lifecycle.test.mjs` — UI부터 candidate/regex/root/status/open 수용 module.
기존 offline/cancel invariant가 통과하는데 engine을 재작성하지 않는다. watcher lifecycle 파일 변경은 신규 실패 회귀가 확인될 때만 추가한다.

### 먼저 실패해야 하는 회귀 assertion

- 실제 candidate chain 합성 이름 `foo.txt`, `bar.md`, `a---b.log`, `한글.md`: `foo|bar`, `a.*b`, `.*`, `한글` 모두 포함할 정상 후보를 포함. worker에 미리 정답 후보를 넣은 테스트만으로 끝내지 않음.
- 후보2,001개에서 only-match가2,001번째: complete/0건으로 표시하면 실패. `partial=true`, cause candidate limit, UI 일부 검색 안내가 반드시 존재. 후보2,000 이하의 complete 데이터에서는 worker predicate와 결과집합 일치.
- query A→B/source A→B와 늦은 native/worker 응답: B만 표시; A job cancel 1회, 새 worker old response ignored. regex timeout은 제한 오류, native literal fallback 금지.
- 저장 recipe load를 notes/content/regex 현재 상태에서 수행: 단 한 canonical search 호출이 files/name/literal+saved filter로 시작. 저장 버튼은 incompatible 상태에서 disabled+설명.
- 검색 상태 false로 정착 후 root offline→active visible poll→offline badge; source 복귀→기존 삭제를 추정하지 않고 complete scan 뒤 converge. removed-root rows/reference는 사용할 수 없음.
- Root remove 실패: chip 유지, role alert, busy 해제; 성공 후 meta 갱신. cancel 중 재시작 버튼은 worker 종료 전 중복 작업을 만들지 않음. 기존 last-good rows/content가 유지.
- fake clock+180초 경과 reference open: `search_stale`, no raw-path opener; refresh 후 새 reference 선택으로만 open 성공.
- hidden Search는 UI poll을 하지 않음, background Agent fixture 파일 생성은 계속 색인됨. indexContent=false root에서 본문 sentinel은 content query에 없음.

### 좁은 검증 명령

```bash
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/knowledge-features exec vitest run src/search/App.test.tsx src/search/api.source.test.ts src/search/lib/regex.test.ts
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-content-index-engine --lib core::db
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-content-index-engine --lib commands::indexing
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-knowledge --lib core::source_search
node --test .github/scripts/windows-knowledge-search-lifecycle.test.mjs
```

core::db의 해당 추가 테스트 이름 필터로 더 좁혀 우선 실행하고 PR 끝에서 위 영향 module을 묶는다. 기존 invariant `offline_root_preserves_index_until_a_complete_reconnect_snapshot`, `cancelled_replacement_scan_keeps_last_good_rows_and_content`는 영향 회귀에 포함한다.

### Windows 수용 기준

실제 UI로 root 추가/indexContent opt-in/regex 입력/source 선택/취소/재시작/root 제거를 수행하고 native index+UI 결과를 비교한다. 기존 root offline·WSL unavailable fixture의 last-good 보존과 reconnect complete convergence를 확인한다. source참조 만료·파일 교체 후 open이 raw path fallback 없이 거절되는지 확인한다. 설치본은 Knowledge를 닫은 사이 Agent가 처리한 fixture event의 indexed time이 재열기 이전인지 확인하고, portable은 창 닫기 이후 owner가 남지 않는 대조를 R15에 제출한다. 실제 root 상태 badge 갱신도 측정하여 native command-only 시험으로 UI 갱신을 대체하지 않는다.

## 작업 R13 — 활동 정확성·handoff·privacy/owner 설명

### 목표와 고정 계약

1. K2: 일반 `finish()`의 단조 시간 종료와 `closeAtLastInput()`의 idle 소급 종료를 분리한다. 반복 observe가 last-input 이후 end를 늘렸어도 idle 판정 때 마지막 입력 이후 구간을 제외한다. end≥start, 음수시간 없음; active session이 threshold 동안 window/title 변경으로 나뉜 경우 이미 닫힌 idle-tail session도 수집 tick의 보류/정산 범위에서 처리한다. 한 함수의 max 제거만으로 해결했다고 판단하지 않는다. suspend/resume·32bit input tick wrap/벽시계 보정과 pause 종료를 서로 분리한다. 기존 idle 정책은 last input부터 전부 제외하는 것이다.
2. K4: `regenerate(handoffId)`는 native history에서 period/startDate/endDate/timezone/filter를 읽어 digest request를 재구성한다. UI 현재 digest/date/설정 tab state를 입력으로 쓰지 않는다. 기록이 이미 보존한 summary.filter를 사용하고 원문/경로/secret/전체 digest를 history에 추가 저장하지 않는다. historical timezone의 day boundaries는 native timezone 검증·재구성으로 만든다; renderer 로컬 Date로 임의 재해석 금지.
3. 하나의 regenerate 요청은 digest single-flight/cancel generation을 소유하고 기존 cancel 경계를 존중한다. 새 draft ID와 regeneratedFrom 링크 생성, 기존 handoff는 변경하지 않는다. `expired`가 재생성 불가인지 일반 새 집계로 재생성 가능한지 정책을 명시한다: 기본 제안은 원본 역사 요약이 유효하게 보존된 기간이면 같은 범위를 새로 집계 가능, source unavailable이면 partial source 설명과 검토 후 전달; 원본 삭제/검증 실패이면 명시 오류. 성공=미리보기/검토 대기이며 자동 노트 저장 아님.
4. N03: Timeline도 day 이동. 월 이동은 현재31일→다음월 overflow로 건너뛰는지 추가 회귀로 검토하고 계약을 `다음 월의 가능한 동일 일 또는 월말`로 명시한다. 이 월말 문제는 본 계획에서 새 확정 결함으로 세지 않는다. today/midnight/timezone 동작은 주입 가능한 clock/date helper로 검증한다.
5. N04: threshold 저장은 `try_set_setting()` 결과를 전달한다. native는 idle parser가 지원하는 범위 밖의 값을 reject하고 previous value 보존, poison/write fail에도 panic/거짓 Ok 금지. UI에 입력 draft와 last acknowledged threshold를 분리하고 explicit save/pending/오류/재시도 제공. 성공 뒤 current native value로 갱신하며 실패·늦은 응답이 최신 state를 덮지 않음.
6. K6/U5: native lifecycle status가 installedAgent/portableLocal/unknown owner와 consent/tracking 상태를 제공한다. renderer build/OS 추정이나 연결 `unavailable`을 portable로 치환하지 않는다. 설치본 quit은 동의한 background work가 계속된다는 정확한 설명; portable quit은 local collector stop 설명; unknown은 중지를 보증하지 않고 상태 확인 오류/안전 안내. LifecycleSettings/QuitGuard/Activity/tray가 같은 용어와 실제 상태를 사용한다.
7. privacy U5는 UI active와 native collection consent를 분리한다. 숨긴 탭 UI poll을 중지해도 수집을 꺼버리지 않는다. privacy rule save 실패는 현재 승인된 rule을 유지하고 적용 성공을 표시하지 않는다. 동의하지 않은 새 설치는 0수집; pause의 의미는 미래 수집 중단이며 기존 데이터 삭제가 아님; 기존 삭제/export 기능의 별도 범위를 화면에서 설명한다. privacy rule이 새 수집/기존 기록 조회 각각 어디 적용되는지 코드기반 contract table을 작성하고 원래 적용 범위를 검증 없이 확장하지 않는다.

### 정확한 변경 예정 파일

수정:
- `/home/jihoon/projects/devbox/crates/activity-engine/src/core/sessionizer.rs`, `core/idle.rs`, `commands/tracking.rs`, `core/db.rs` — idle 정산과 acknowledged threshold 저장.
- `/home/jihoon/projects/devbox/crates/activity-engine/src/commands/handoff.rs`, `core/draft_history.rs`, `core/digest.rs`, `api.rs`, `component.rs` — bounded history→regenerate input, native timezone boundaries, typed 요청/응답.
- `/home/jihoon/projects/devbox/apps/devbox-knowledge/src-tauri/src/ipc/activity.rs`, `activity_projection.rs` — 신규 regenerate typed call의 설치본 prepare→offer→finish 및 portable delivery bridge 연결. Agent는 기존 `native_draft_delivery` envelope에서 새 prepare subtype을 처리하며 owner/session admission을 유지한다; `apps/devbox-agent/src/routes.rs`의 route/권한 자체를 넓히지 않는다.
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.tsx`, `components/ActivitySettings.tsx`, `api.ts`, `lib/activityPresentation.ts` — digest 독립 재생성, Timeline shift, threshold draft/ack, active poll control.
- `/home/jihoon/projects/devbox/apps/devbox-knowledge/src/Knowledge.tsx`, `QuitGuard.tsx`, `LifecycleSettings.tsx`, `src-tauri/src/lifecycle.rs`, `ipc/commands.rs`, `collector_owner.rs` — native owner DTO를 통한 실제 lifecycle 설명. 공용 shell agent_status/reconnect transport는 R15 소유라 별도 구현을 복제하지 않음.
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.contextMenu.test.tsx`, `App.test.ts`, `/apps/devbox-knowledge/src/QuitGuard.test.tsx`, `LifecycleSettings.test.tsx`.
- `/home/jihoon/projects/devbox/.github/scripts/windows-agent-collectors.mjs` — 기존 `physicalForegroundSessionCapture='사용자 확인 대기'`를 자동 실제 foreground/session 검증 결과로 바꿈; consent/pause/owner evidence 추가. 이 항목은 native 실기를 맡는 R15와 한 쪽만 변경하도록 사전 합의.
신규:
- `/home/jihoon/projects/devbox/packages/knowledge-features/src/activity/App.lifecycle.test.tsx`, `components/ActivitySettings.test.tsx` — historical regeneration/Timeline/ack failure 집중 회귀.
- `/home/jihoon/projects/devbox/.github/scripts/windows-knowledge-activity.mjs`, `windows-knowledge-activity.test.mjs` — hosted consent/foreground/idle/privacy/quit UI 수용 module.
- 필요 시 `/home/jihoon/projects/devbox/crates/activity-engine/src/core/tracking_clock.rs`(신규): sampling 정산을 기존 sessionizer/idle 안에 간결하게 둘 수 없을 때만 테스트 가능한 pure coordinator 추출; 새 crate 금지.

### 먼저 실패해야 하는 회귀 assertion

- 원본 probe schedule(lastInput=1,000,000, observe 계속, threshold300,000): `closed.end_ts == 1_000_000`, 기존 `1_298_000`은 실패. idle-tail app/title change 구간 합계가0, resume 뒤 새세션 정상, pause는 실제 pause time으로 종료.
- lastInput<sessionStart/end, suspend 큰 gap, tick wrap, threshold 변경 중 수집: 음수시간·중복 삽입 없음; closed intervals 겹침 없음; 정상 활동 tick 합계 보존.
- Settings로 이동 후 displayed digest=null: regenerate button enabled(유효 entry), native 호출은 selected historical ID만 보냄. UI 현재 date/filter 변경과 무관하게 history period/timezone/filter가 native digest input에 일치. 기존 handoff unchanged/new ID/regeneratedFrom linkage. 중도 tab/date 전환과 cancel 후 늦은 응답 무시, busy 해제.
- Timeline previous/next: date±1일, native request dayStart/dayEnd 모두 이동; 월말/연말/DST boundary와 월 shift 계약 회귀.
- native read-only DB: setter Err+old value 유지; UI role alert/ack unchanged. 저장 성공 후 reopen/get 값 일치. 빠른 입력·두 응답 역전 시 최신 ack만 반영. unsupported threshold native reject.
- owner installed+tracking true: QuitGuard가 중지라고 표시하지 않음. portable는 실제 collector shutdown과 설명 일치. unavailable installed still installed; unknown never promises stopped. quit cancel은 owner/session/dirty 그대로.
- privacy 변경 save failure: 이전 native effective rules·UI ack 유지. 수집 pause 후 fixture 새 title session 없음; 이미 저장된 합성 기록 조회/export 설명과 실제 포함 범위 일치. no consent 첫 설치는 DB session0.

### 좁은 검증 명령

```bash
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-activity-engine --lib core::sessionizer
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-activity-engine --lib core::idle
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-activity-engine --lib commands::tracking
python3 .github/scripts/verify-resources.py -- cargo test --locked -p devbox-activity-engine --lib commands::handoff
python3 .github/scripts/verify-resources.py -- pnpm --filter @devbox/knowledge-features exec vitest run src/activity/App.lifecycle.test.tsx src/activity/components/ActivitySettings.test.tsx src/activity/App.test.ts
python3 .github/scripts/verify-resources.py -- pnpm --filter devbox-knowledge exec vitest run src/QuitGuard.test.tsx src/LifecycleSettings.test.tsx
node --test .github/scripts/windows-knowledge-activity.test.mjs
```

### Windows 수용 기준

로컬 소유 fixture 또는 독립 VM의 interactive desktop의 **소유한 합성 창**을 foreground에 올린 뒤 HWND/PID/title identity를 검증하고, native Windows 입력 시각을 기록한다. 순수 CDP F24 acknowledgment만으로 GetLastInputInfo 변경을 보증하지 않는다. 허용된 inert SendInput→GetLastInputInfo 관측 후 1분 threshold 이상의 무입력 구간과 resume를 자동 관측하여 DB session endpoint/idle excluded duration을 비교한다. default5분 정책은 deterministic unit에서 확인하고 같은 의미의 장시간 OS 대기는 중복 추가하지 않으며 OS 수용을 모의시각 PASS로 대체하지 않는다. 30초 이하 간격으로 진행근거를 남겨 hang과 wait를 구분한다.

이전에 기록된 실제 사용자 창 제목/앱을 sampling fixture로 쓰지 않는다. privacy rule에 소유 synthetic title을 포함/제외하고 persistence·digest·handoff source summary가 계약대로 처리되는지 자동 확인한다. Activity 설정 실제 save/handoff regeneration/Timeline navigation UI를 native DB/receipt와 대조한다. 설치본 UI close 뒤 같은 agent PID+creation time 유지와 collector/index 동작, portable UI close 뒤 local owner 종료를 R15에서 교차확인한다.

## R15로 넘길 Agent 실사용 수용 계약(독립 Agent 결함 확정 아님)

변경 주 소유자는 부모 R15다. 계획의 테스트는 기존 production에 이미 존재하는 재연결/no-replay/stop/update/ownership 계약을 실제 OS 경계까지 검증한다. 실패가 확정되면 해당 생산 파일에 최소 수정과 회귀를 추가하며, 코드 읽기만으로 예방적인 Agent 재작성은 하지 않는다.

| 시나리오 | 자동 동작 및 필수 assertion | 기존 코드/fixture 연결 |
|---|---|---|
| 첫 설치/활성화 | 완전한 UI setup 절차로 store manifest 생성; 준비 전 denied는 pending 안내, 준비 후 한 owner; no consent는 activity0, 이름 index는 consent와 독립 | `apps/devbox-agent/src/collectors.rs`, `runtime.rs`, `crates/product-shell-tauri/src/lib.rs`; shared bootstrap 작업 선행 |
| cold reconnect | 네 제품 동시 연결→bootstrap launch count1; starting→connected 상태; ready receipt 확인; UI button 경유 manual reconnect | `crates/agent-client/src/lib.rs`, `crates/suite-runtime/src/platform/agent_transport.rs`, `.github/scripts/windows-agent-reconnect.mjs` |
| 제출 중 crash | identity 검증한 해당 fixture agent만 종료; submitted mutation 자동 replay0; pending UI 오류1; next read 또는 명시 reconnect만 새 owner; old generation reply ignored; durable receipt와 content 보존 | `.github/scripts/windows-agent-runtime.mjs`의 crash를 Knowledge request에 확장; 기존 client no-replay test 유지 |
| tray pause/resume | 실제 tray menu action 또는 검증된 메뉴 handler 경유 pause; native tracking=false, 이후 synthetic session0; resume는 새세션; 검색 owner는 정책대로 유지 | `apps/devbox-agent/src/tray.rs`, `collectors.rs`; command-only stop과 실제 메뉴 경로 근거 구분 |
| tray 전체 종료 | shutdown notice→UI unavailable/stopped; collector/index/webhook/runtime 합류와 exact peer exit; 모든 기존 UI polling을 최소2 backoff cycle 넘게 계속해도 launch0; manual reconnect는 허용 | `agent-client` intentional-stop latch, `apps/devbox-agent/src/server.rs`, `routes.rs`; user-confirm pending 없이 자동 identity/pipe 관측 |
| update | exact verified generation의 peer stop; missing agent 종료 요청은 launch0; old peer/pipe 사라지기 전 새 writer admission 금지; update committed 후 새 build identity; protocol mismatch도 same verified identity 종료 | `shutdown_if_running`, transport generation/identity, suite writer lease; parent delivery/update fixture 선행 |
| background ownership | Knowledge close 후 same PID+creation agent가 fixture file event를 UI 재열기 전 반영; consented activity가 계속, dirty note quit은 문서 보호; pause/전체 종료가 각각 다름 | `.github/scripts/windows-agent-collectors.mjs`, Knowledge QuitGuard/lifecycle, R12/R13 |
| portable 대조 | agent unsupported, product-local engine만 소유; UI close 후 collection/index 종료; 재실행 store/journal 보존; installed unavailable을 portable fallback으로 우회하지 않음 | `collector_owner.rs`, native transport; separate disposable product fixture |
| second installation/retirement | install/session/generation mismatch 요청 거절; 다른 설치의 data root/peer/pipe/process 불변; shutdown은 retained peer만 대상으로 함; lease 재검증 실패는 data write0 | component_scope/agent_protocol/wire 및 installer fixture; 사용자 설치/프로세스는 fixture에서 제외 |

R15 파일 제안: 기존 `/home/jihoon/projects/devbox/.github/scripts/windows-agent-runtime.mjs`, `windows-agent-runtime.test.mjs`, `windows-agent-reconnect.mjs`, `windows-agent-reconnect.test.mjs`, `windows-agent-collectors.mjs`, `windows-installer-acceptance.ps1`를 확장한다. tray 실제 동작·idle 입력 module이 필요하면 신규 `windows-agent-lifecycle.mjs`/`.test.mjs`를 추가하고 `.github/workflows/product-foundation.yml` 또는 기존 installer acceptance **정확히 한 실행 경로**에 등록한다. 중복 job 새로 만들지 않는다. 어떤 runner가 interactive foreground를 제공하는지 사전 입증하고 없으면 구체적 runner gap으로 기록한다.

좁은 순수 회귀 예정 명령:

```bash
source ~/.cargo/env
python3 .github/scripts/verify-resources.py -- cargo test --locked -p agent-client --lib disconnect_fails_the_submitted_call_once
python3 .github/scripts/verify-resources.py -- cargo test --locked -p agent-client --lib graceful_shutdown
python3 .github/scripts/verify-resources.py -- cargo test --locked -p agent-client --lib update_shutdown
node --test .github/scripts/windows-agent-reconnect.test.mjs .github/scripts/windows-agent-runtime.test.mjs
```

Windows 명령은 Windows에서 기존 hosted entrypoint `node .github/scripts/windows-knowledge-lifecycle.mjs`와 installer acceptance job이 각 신규 module을 호출하도록 한다. source commit/artifact hash/install namespace/agent PID+creation+path/build generation, request receipt, native DB assertion, UI screenshot와 failed-stage diagnostics를 artifact에 보존한다. 실패한 native 동작을 다른 route 하드코딩으로 통과시키지 않는다. compilation·unit pass와 UI/OS pass를 따로 집계한다. installed packaged 수용은 exact commit의 기존 build artifact로만 수행하고 임의 새 build/기존 stable로 바꾸지 않는다.

## 추가 검토 범위와 제외

검토한 현행 연결: editor history/onChange→NoteDocument→autosave/journal/quit, recovery missing snapshot/native journal scope, Activity load/settings/history DTO/idle setter, Search candidate/FTS/API/partial/referenceTTL/root status/index cancel·offline 보존, installed collector owner/native lifecycle, Agent connect/stop/update/transport identity의 생산 경로와 현행 Windows module. 이번 계획 단계에서는 테스트·native 실행을 추가하지 않았다. 모든 전체 파일을 철저히 감사했다는 주장이나 미실행 Windows PASS는 하지 않는다.

U1–U5는 의도/범위가 필요한 구체 개선 제안이고, 이미 확정 K1–K7/N03/N04와 같은 결함 개수로 합산하지 않는다. 새 권한 확장, 개인정보 raw history 저장, regex 무한순회, 모호한 silent fallback, Agent 전면 재작성은 계획에 없다. 추가 UX 개선을 PR 끝의 미정 후속 항목로 남기지 않고 각 정책 계약/회귀/Windows 수용을 해당 작업 완료 범위에 넣었다.

## Windows UI 모듈 등록

R01의 `windows-knowledge-document-recovery.mjs`는 DOC-01/02/03, R12의 `windows-knowledge-search-lifecycle.mjs`는 SEARCH-01/02, R13의 `windows-knowledge-activity.mjs`는 ACTIVITY-01/02/03을 반환한다. 절대 기준 디렉터리는 `/home/jihoon/projects/devbox/.github/scripts/`다. 각 모듈은 04에 정의한 `run(context): Promise<ScenarioResult[]>` export와 02의 UiDriver/evidence 계약을 사용한다. 기존 lifecycle/collector runner에 export adapter를 추가하되 현재 direct invoke 경계 검사를 UI 결과로 재명명하지 않는다. 작업 subset matrix 등록 및 `.test.mjs`의 ID/소유 fixture/cleanup 검사를 각 담당 작업에 포함한다.
