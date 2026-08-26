# Knowledge Rename Preview and Transaction

## Overview

Issue #273의 P1-09 Knowledge 범위로 파일·폴더 이름 변경을 즉시 실행하던 흐름을
`preview → 전체 승인 → one-shot apply`로 교체했다. 이름 변경 뒤 깨질 위키링크만 canonical 새
target으로 바꾸고, alias나 계속 유일하게 resolve되는 title link는 그대로 둔다. apply 직전에는
filesystem snapshot을 다시 검증하며, 파일 rewrite·source rename·SQLite 인덱스 중 하나라도 실패하면
이미 반영한 filesystem 변경을 되돌린다.

```text
사용자 rename 요청
    │
    ├─ dirty editor ────────────────X─► 먼저 저장
    │
    ▼
canonical source/destination validation
    │
    ├─ root inventory ≤ 10,000
    ├─ all Markdown + source subtree ≤ 64 MiB
    └─ SHA-256 snapshot
    │
    ▼
future link-key simulation
    │
    ├─ existing key still unique ──────► no rewrite
    ├─ canonical new key unique ───────► target-only rewrite, alias preserved
    └─ canonical new key ambiguous ─X──► no plan
    │
    ▼
relative path + bounded link syntax diff
    │
    └─ opaque one-shot plan ID ─► fixed full approval / discard
                                  │
                                  ▼
                    snapshot revalidation
                                  │
             stale/destination conflict ─X─► no mutation
                                  │
                                  ▼
       per-file atomic rewrite → source rename → SQLite transaction
                                  │
                         failure ─┴─► reverse rollback
```

Windows packaged locked-file와 rollback 시각 smoke는 계획된 W1 P1 묶음 checkpoint에서 수행한다.
이 작업은 WSL에서 검증 가능한 pure/core transaction, Tauri command 경계, React 승인 UI, 회귀 테스트와
production build를 완료한다.

## Context

기존 `rename_file` command는 destination이 없으면 바로 `std::fs::rename`을 실행하고 이동한 파일을
SQLite FTS에 재색인했다. 다음 위험은 해결하지 못했다.

- path/filename key를 사용한 inbound `[[wikilink]]`가 이동 뒤 missing이 되어도 알 수 없었다.
- link source 여러 개와 이동을 사용자가 적용 전에 확인하거나 하나의 단위로 승인할 수 없었다.
- link rewrite를 나중에 덧붙이면 일부 source만 바뀐 뒤 rename이나 DB가 실패할 가능성이 있었다.
- preview와 apply 사이 외부 editor/watcher 변경을 감지할 snapshot contract가 없었다.
- folder 안의 note, folder 밖 inbound source, folder 내부 cross-link를 같은 계획으로 다뤄야 했다.

SQLite는 계속 보조 인덱스이고 filesystem이 source of truth다. 따라서 DB transaction만으로 여러
파일의 전역 원자성을 주장하지 않고, bounded preflight와 파일별 atomic replace, 실패 시 명시적
rollback을 조합하는 all-or-rollback 계약을 선택했다.

## Scope

### Included

- 파일·폴더 source와 새 root-relative destination 미리보기
- destination 존재, symlink escape, 자기 subtree 이동의 사전 거부
- 이동 전/후 note key map simulation
- 현재 유일하게 이동 note를 resolve하며 이동 뒤 깨지는 wikilink만 rewrite
- explicit `|alias`와 target 앞뒤 whitespace의 byte-for-byte 보존
- 이동 경로와 영향 link syntax의 before/after diff
- checkbox 없는 전체 변경 승인과 취소
- app-managed 단일 one-shot plan slot
- apply 직전 root/inventory/content SHA-256 재검증
- 파일별 overwrite-capable atomic write, filesystem rename, SQLite FTS/link transaction
- partial write, rename, DB index/commit 실패의 filesystem rollback
- 성공 뒤 activity snapshot, tree/tag/link metadata와 현재 editor disk content 갱신
- 파일·폴더, stable link, alias, collision, stale plan과 injected DB failure fixture
- README, architecture, roadmap, product opportunity와 native-first 상세 계획 동기화

### Excluded

- OS primitive 하나로 여러 파일을 동시에 교체하는 global atomic transaction
- apply 도중 process/OS 강제 종료 뒤 재개하는 persistent recovery journal
- 일반 Markdown link rewrite, heading fragment, transclusion과 fuzzy alias
- missing/ambiguous/invalid wikilink의 임의 복구
- case-only rename을 위한 Windows 전용 two-hop rename
- quick capture, Inbox, attachment/image, daily/weekly template와 opt-in Git
- 외부 knowledge tool, sidecar, network service와 runtime download
- Windows packaged UI/locked-file smoke의 개별 실행

## Changes Made

### 1. Bounded Filesystem Plan and Future Link Resolution

`apps/knowledge-base/src-tauri/src/core/rename.rs`에 preview와 apply가 공유하는 filesystem 계약을
추가했다. preview는 다음 데이터를 한 번 순회해 구성한다.

| Boundary | Limit / rule |
|---|---|
| Root entries | 최대 10,000 file/directory path |
| Snapshot contents | 모든 Markdown + source subtree 합계 64 MiB |
| Rewritten Markdown files | 최대 200개 |
| Rewritten links | 최대 5,000개 |
| Link syntax in public preview | link당 최대 1,024 UTF-8 bytes |
| Destination | root 내부, existing entry/symlink 없음 |
| Folder destination | source 자기 subtree가 아님 |
| File extension kind | Markdown 여부(`.md`/비 Markdown) 유지 |

fingerprint는 path 길이와 bytes, directory flag, 읽어야 하는 파일의 내용 길이와 bytes를 SHA-256에
순서대로 넣는다. root 밖 non-Markdown 파일 내용은 link resolution이나 source 이동과 무관하므로
경로 inventory만 포함하고, 이동 subtree 안의 binary를 포함한 모든 파일 내용과 root 전체 Markdown은
정확히 포함한다. 새 entry의 생성/삭제, Markdown 수정, 이동 source 내부 파일 수정은 stale plan을
만든다.

현재 key map은 각 note의 root-relative path, filename, frontmatter title key를 사용한다. future map은
이동 subtree의 path만 destination으로 remap한 뒤 같은 key 규칙을 다시 적용한다. link rewrite 조건은
다음 세 단계를 모두 통과해야 한다.

1. 현재 target key가 exactly one note로 resolve한다.
2. 그 note가 이동 대상과 같거나 이동 folder의 descendant다.
3. 기존 key가 미래의 새 path를 유일하게 resolve하지 않는다.

rewrite가 필요하면 새 path without `.md`의 canonical key도 future map에서 정확히 새 note 하나로
resolve해야 한다. 다른 note의 title/filename/path key와 충돌하면 임의 target을 고르지 않고 plan
생성을 중단한다. source의 `[[ target | alias ]]`에서 trim된 target byte range만 바꾸므로 pipe 뒤 alias,
target 주위 whitespace와 나머지 문서는 그대로 유지된다.

### 2. Opaque One-shot Plan Vault

`RenamePlanStore`는 app process 안에 현재 plan 하나만 보관한다. plan에는 root canonical identity,
fingerprint, rewrite 원문/결과, 재색인할 UTF-8 문서와 생성할 parent 목록이 있으므로 의도적으로
`Debug`와 `Serialize`를 구현하지 않았다.

```rust
pub struct RenamePlan {
    id: String,
    root: PathBuf,
    fingerprint: [u8; 32],
    rewrites: Vec<FileRewrite>,
    index_documents: Vec<IndexDocument>,
    // ... relative operation metadata
}
```

frontend에는 monotonic opaque string ID와 root-relative path, bounded diff만 반환한다. 새 preview는
이전 plan을 먼저 지우고, apply는 ID 일치 여부와 무관하게 slot에서 plan을 꺼내 한 번만 시도한다.
취소도 일치하는 plan을 즉시 폐기한다. 재시도는 항상 새 snapshot부터 만든다.

### 3. Conflict-checked Apply and Rollback

`apps/knowledge-base/src-tauri/src/commands/rename.rs`가 세 IPC command를 제공한다.

- `preview_rename(from, to)`: 이전 plan 폐기, bounded plan 작성, public diff 반환
- `apply_rename(plan_id)`: one-shot take, DB mutex 안에서 root resolve와 transaction apply
- `discard_rename_preview(plan_id)`: 아직 보관 중인 exact plan 폐기

apply는 현재 canonical root identity, source kind, destination absence와 전체 fingerprint를 다시
계산한다. 하나라도 다르면 파일이나 DB를 바꾸기 전에 고정 conflict 오류를 반환한다. 검증 뒤 실제
순서는 다음과 같다.

1. SQLite transaction 시작
2. preview 때 없던 destination parent를 component 단위로 만들고 목록 기록
3. 영향 Markdown을 `devbox_filesystem::atomic_write`로 old/current path에 교체
4. source를 destination으로 rename
5. old prefix의 FTS/link rows 제거
6. 이동한 readable UTF-8 파일과 외부 rewrite source를 final path/content로 재색인
7. SQLite commit
8. 성공 뒤 privacy-safe Knowledge activity snapshot 갱신

3~7 단계 실패 시 SQLite transaction을 drop/rollback한 뒤 source rename을 먼저 되돌리고, 실제 쓴
rewrite를 역순으로 원문 복구하며, 생성했던 destination parent를 빈 경우에만 역순 제거한다. rename
복구 자체가 실패하면 이동 subtree rewrite의 현재 destination path에서 원문 복구를 시도한다.
rollback 어느 단계든 실패하면 성공 또는 일반 apply 오류로 숨기지 않고 Knowledge folder 수동 확인
메시지를 반환한다.

watcher도 같은 `AppState.db` mutex를 사용한다. command가 filesystem과 DB transaction을 수행하는
동안 watcher가 중간 인덱스를 쓰지 못하고, lock 해제 뒤 전달되는 rename/write event가 최종 상태로
수렴한다.

### 4. Fixed Full-approval Diff UI

`packages/diff-view/src/index.tsx`에 기존 두 소비자의 기본 동작을 바꾸지 않는 optional props를
추가했다.

```tsx
<ChangeSetPreview
  items={renamePreview.items}
  selectable={false}
  disabled={renameBusy}
  onApprove={() => void commitRename()}
  onCancel={cancelRename}
/>
```

- `selectable` 기본값은 `true`라 Code Pad recovery와 Run Manager import의 checkbox/부분 선택이
  그대로 유지된다.
- Knowledge는 `false`를 전달해 toggle-all, checkbox, reject를 숨기고 모든 item을 한 번에 승인한다.
- `onReject`는 optional이 되어 discard와 cancel 의미가 다른 기존 소비자만 제공한다.
- `disabled`는 apply 중 중복 승인·취소·선택을 막는다.

Knowledge `App.tsx`는 dirty editor가 있으면 prompt나 preview 호출 전 저장 안내를 표시한다. preview
modal은 이동 item과 각 link source item을 보여주며 apply 전에는 mutation command를 호출하지 않는다.
성공 뒤 selected/tree path를 folder rename까지 prefix remap하고 현재 selected file을 disk에서 다시
읽어 self-link나 external inbound rewrite를 editor에 반영한다. apply 실패는 one-shot modal을 닫고 새
preview가 필요함을 backend 오류로 알린다.

### 5. API, State and Styling

- `apps/knowledge-base/src/api.ts`: camelCase preview/applied DTO와 세 command wrapper를 추가하고
  즉시 `rename_file` wrapper를 제거했다.
- `apps/knowledge-base/src-tauri/src/commands/docs.rs`: `AppState`에 plan store를 추가하고 기존
  rename/index helper를 제거했다.
- `apps/knowledge-base/src-tauri/src/lib.rs`: 새 command 등록과 plan store 초기화를 연결했다.
- `apps/knowledge-base/src/App.css`: modal, responsive two-column diff, before/after 상태와 disabled
  control을 기존 token으로 표현했다.
- `apps/knowledge-base/package.json`, `pnpm-lock.yaml`: 기존 workspace `@devbox/diff-view`를 세 번째
  소비자로 연결했다.

### 6. Dependency Decision

Knowledge가 fingerprint를 직접 계산하도록 `sha2 = 0.11.0`을 direct Rust dependency로 선언했다.
같은 resolved crate는 이미 workspace `Cargo.lock`과 `THIRD_PARTY_NOTICES.md`에 있었고 license는
MIT OR Apache-2.0이다. 새 crate version이나 transitive dependency는 resolve하지 않았다.

`@devbox/diff-view`도 이미 Code Pad와 Run Manager가 쓰던 private workspace package라 registry
download, 새 runtime license 또는 network dependency가 없다. 기능은 설치 후 완전히 offline으로
동작하며 external executable, daemon, polling worker를 추가하지 않는다.

동일 Node/Vite toolchain의 #272 main 대비 production bundle은 다음과 같다.

| Asset | Main | Feature | Delta |
|---|---:|---:|---:|
| Largest JS exact | 1,411,279 B | 1,415,036 B | +3,757 B |
| Largest JS gzip | 416,435 B | 417,734 B | +1,299 B |
| CSS exact | 9,361 B | 11,224 B | +1,863 B |
| CSS gzip | 2,466 B | 2,884 B | +418 B |

plan은 하나만 보관하고 snapshot input은 64 MiB에서 fail-closed한다. 로컬 빌드와 테스트는 Cargo
job 1개, Vitest worker 1개, Node heap 768 MiB로 순차 실행했다. 별도 background resource는 없다.

## Files Changed

### Backend and dependency metadata

- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `apps/knowledge-base/src-tauri/Cargo.toml`
- `apps/knowledge-base/src-tauri/src/core/mod.rs`
- `apps/knowledge-base/src-tauri/src/core/rename.rs`
- `apps/knowledge-base/src-tauri/src/commands/mod.rs`
- `apps/knowledge-base/src-tauri/src/commands/docs.rs`
- `apps/knowledge-base/src-tauri/src/commands/rename.rs`
- `apps/knowledge-base/src-tauri/src/lib.rs`

### Frontend and tests

- `packages/diff-view/src/index.tsx`
- `apps/knowledge-base/package.json`
- `pnpm-lock.yaml`
- `apps/knowledge-base/src/api.ts`
- `apps/knowledge-base/src/App.tsx`
- `apps/knowledge-base/src/App.css`
- `apps/knowledge-base/src/App.test.tsx`
- `apps/knowledge-base/src/App.applink.test.tsx`
- `apps/knowledge-base/src/App.wikilinks.test.tsx`

### Product and architecture documentation

- `apps/knowledge-base/README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/product-opportunities.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `workthrough/2026-08-26-knowledge-rename-preview-transaction.md`

## Verification Results

### Rust focused verification

```text
cargo test -p knowledge-base core::rename --jobs 1
7 passed; 0 failed
```

fixture coverage:

- file rename에서 stable title link는 유지하고 path link target만 변경
- explicit alias와 target whitespace 보존
- folder 내부 cross-link와 folder 외부 inbound link 동시 변경
- 이동 문서와 외부 rewrite source의 FTS/link 재색인
- 미래 canonical key가 다른 note title과 충돌할 때 mutation 전 중단
- destination 존재·잘못된 parent·Markdown 종류 변경의 mutation 전 중단
- preview 뒤 source 변경 시 exact snapshot conflict와 zero mutation
- SQLite insert failure injection 뒤 link 원문, source rename, 새 parent directory rollback

### Frontend verification

```text
pnpm --filter knowledge-base exec vitest run --maxWorkers=1
Test Files  7 passed (7)
Tests       30 passed (30)
```

새 frontend fixture는 preview API가 먼저 호출되고 apply는 승인 전 호출되지 않는지, fixed 목록에
checkbox가 없는지, 이동/link before/after가 모두 보이는지, 전체 승인 뒤 exact plan ID를 적용하는지,
취소가 backend plan을 폐기하는지 검증한다. 보강된 App fixture는 native transaction 성공 뒤 현재
note 재읽기만 실패하면 raw 오류나 stale 본문을 표시하지 않고 metadata 갱신을 계속하는지도 검증한다.
기존 mode, context menu, applink, editor, preview,
wikilink/backlink suite도 같은 단일 worker 실행에서 통과했다.

```text
pnpm --filter knowledge-base build
TypeScript compile: passed
Vite: 2,153 modules transformed, production build passed
```

### PR-wide completion gates

PR 직전 집중 검토에서 다음 기능 범위 gate를 통과했다.

- full `cargo test -p knowledge-base --jobs 1` — 48 passed
- `cargo check -p knowledge-base --jobs 1` — passed
- `cargo clippy -p knowledge-base --all-targets --jobs 1 -- -D warnings` — passed
- `cargo fmt --all --check` — passed
- `pnpm install --frozen-lockfile`와 `pnpm audit --audit-level moderate` — passed, 취약점 0
- generated notices check와 dependency/build-manifest regression tests — passed
- `cargo deny --locked check` — advisories/bans/licenses/sources passed
- catalog consistency — passed
- `cargo test --workspace --jobs 1` — passed
- `cargo check --workspace --jobs 1` — passed
- `NODE_OPTIONS=--max-old-space-size=768 pnpm -r --workspace-concurrency=1 build` — 17개
  frontend workspace project passed

로컬 completion gate는 모두 단일 Rust job·단일 frontend workspace concurrency로 실행해 자원 사용을
제한했다. GitHub Actions의 Linux/frontend/dependency/catalog/security gate는 PR CI에서 확인한다.

## Known Boundaries and Next Steps

- W1: packaged Windows에서 파일·폴더 rename, missing parent 생성과 tree/editor remap smoke
- W1: 다른 process가 source/link file을 잠근 경우 zero partial success와 복구 오류 표현 확인
- W1: preview 뒤 외부 편집/destination 생성 conflict와 injected/realistic DB failure evidence
- W1: watcher event가 transaction 뒤 old path를 제거하고 final path metadata로 수렴하는지 확인
- Knowledge quick capture/Inbox와 attachment/image는 P2의 독립 기능이다.
- daily/weekly template는 P3, opt-in Git은 후속 후보 경계를 유지한다.
- 다음 P1-09 feature는 계획 순서의 Devbox Manager batch로 별도 branch/PR에서 수행한다.
- Knowledge 0.4.0 version bump는 Wave 9 release preparation에서 별도로 수행한다.
