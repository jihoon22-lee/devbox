# Everything+ 고급 필터와 저장된 검색 구현 기록

## Overview

GitHub issue `#349`(Everything+ 고급 필터)와 `#350`(saved query CRUD 및
`saved-queries/v1` snapshot)을 하나의 사용자 흐름으로 묶은 cohesive PR 후보를
구현했다. 오래된 혼합 worktree에서는 의도한 파일만 transfer commit으로 격리했고,
Developer Toolbox #340–#343의 검토 완료 head `1272b5f` 위에 만든 깨끗한 전용 브랜치
`feat/everything-plus/advanced-filters-saved-queries-final`로 cherry-pick했다. 충돌 뒤 생긴
AppLink 중복 variant/argv arm과 기존 consumer 생성자의 새 filter 필드 누락은 이 clean
worktree에서 직접 제거했다. #453이 squash merge되면 해당 stacked base만 최신 main으로
교체하고 이 기능 커밋만 유지한다.

## Context and decisions

- 확장자·수정 시각·크기·등록 검색 루트·내용 상태 필터는 renderer가 결과를 받은 뒤
  임의로 거르는 대신 SQLite FTS projection에 bound parameter로 적용했다.
- 외부 snapshot에는 사용자가 명시적으로 저장한 query/filter 정의만 발행한다. 현재
  결과 행, raw path, 문서 원문, 환경변수와 credential은 저장하지 않는다.
- source는 호출자가 제출한 path가 아니라 `roots.id`다. root id는 monotonic sequence로
  할당해 삭제 후 재사용하지 않고, 기존 파일 행은 migration/repair 때 가장 깊은 등록
  root에 맞춰 보정한다. orphan row와 경계 밖 stale row는 일반 검색에서도 숨기며, nested
  root 삭제 시 남은 parent/child의 가장 깊은 ownership으로만 재할당한다.
- 검색/인덱싱의 기존 bounded I/O, cooperative cancellation, partial/truncated
  metadata, frontend stale-generation guard를 유지했다. 검색·metadata·cold/hot
  AppLink 요청은 각각 generation을 확인해 늦은 응답/요청을 폐기하며, 검색 결과를
  여는 동작은 실행 직전에 final file identity를 재확인한다.
- Snapshot producer는 `crates/integration::write_atomic`만 사용한다. DB lock은 snapshot
  파일 I/O 전에 반납하고, DB write/prepare/publication 중 실패하면 이전 definition set을
  bounded SQLite transaction으로 보상 복구한다. cross-file transaction을 가장하지
  않으면서도 앱 DB와 마지막 정상 snapshot의 불일치 창을 작게 유지한다.
- 현재 Launcher query payload parser는 `payload.filter`를 동일한 bounded `QueryFilter`로
  소비·재검증해 `query-filter-v1` AppLink로 전달한다. filter 없는 legacy `--query`는
  그대로 유지한다. 현재 payload version/filter의 unknown field나 잘못된 범위는 source를
  fail-closed하며, AppLink build도 오류를 반환해 invalid filter를 text-only 요청으로
  조용히 강등하지 않는다. 설치된 구버전 수신기가 모르는 optional flag를 무시하는 protocol
  하위 호환성과 현재 발신자가 손상된 filter를 버리는 동작은 구분했다.

## Changes made

### 1. Native filter model and bounded database projection

- `apps/everything-plus/src-tauri/src/core/models.rs`
  - `SearchFilter`와 normalization/validation을 추가했다.
  - 확장자 64개·확장자별 16 bytes, 0 이상 날짜/size 범위, 양의 root id, 고정 content
    status만 허용하고 `partial`을 `truncated` 별칭으로 보존한다.
  - `FileEntry`, `ContentResult`, `RootInfo`에 source/content 상태와 extractor
    metadata를 추가했다.
  - `SavedQuery` DTO와 normalization unit fixture를 추가했다.
- `apps/everything-plus/src-tauri/src/core/db.rs`
  - `saved_queries` table 및 CRUD helper를 추가했다(최대 2,048개 read/write 경계).
  - `search_with_filter`/`search_content_with_filter`에 fixed SQL predicate와
    `rusqlite::Value` bound values를 추가하고 filename 2,000/content 200 cap을
    유지했다.
  - 기존 v0.4.x의 missing/zero `files.root_id`를 등록 root 중 가장 깊은 항목으로
    보정한다. filesystem I/O는 migration에서 수행하지 않는다. root id와
    content-status index를 추가하고, ownership 없는 row는 root가 0개인 경우에도 filename/
    content 검색 projection에서 fail-closed한다.
  - filter 조합, partial 상태, saved definition round-trip, root backfill/orphan/nested
    root/drive-root fixture를 추가했다. root 제거는 기존 `root_id`가 잘못된 행도 전체
    deepest ownership repair로 재계산하고, 파일이 없는 `file_content` orphan을 purge해
    status/FTS 파생 상태가 stale로 남지 않게 한다.
  - root 추가·삭제와 전체 deepest-ownership repair를 `BEGIN IMMEDIATE` transaction으로
    묶었다. repair/commit 실패 시 root 목록, monotonic next-id, file ownership과 content
    cleanup을 함께 rollback하며 trigger fault fixture로 양방향을 확인한다.
- `apps/everything-plus/src-tauri/src/commands/search.rs`
  - Tauri 경계에서 optional filter를 normalize하고 고정 오류만 반환한다.
  - 빈 filter는 기존 command/argument 경로를 유지해 하위 호환성을 보존했다.
- `apps/everything-plus/src-tauri/src/commands/indexing.rs`
  - full/re-index batch가 실제 `RootInfo.id`를 파일 행에 기록하게 했다. 기존
    batch size, extraction timeout, cooperative cancel/race check는 유지했다. nested
    content root 제거로 남은 ancestor가 content policy를 새로 소유하는 경우에는 해당
    ancestor만 bounded re-index하도록 예약해 content가 stale로 비어 있지 않게 했다.

### 2. Saved query CRUD and integration producer

- `apps/everything-plus/src-tauri/src/commands/saved_queries.rs`
  - name/query/filter 정의만 저장하는 list/save(update)/delete command를 추가했다.
  - label 128 bytes, query 512 bytes, filter JSON 8 KiB, 총 2,048개 경계를 둔다.
  - 빈 값/control character와 Bearer/Basic/provider token, authorization/password/
    secret/api_key/private-key marker를 persistence 전에 거부한다.
  - `snapshot:everything-plus/saved-queries/v1` multi-view envelope를 만들고
    `payload.text/filter`와 고정 display metadata만 entry로 내보낸다.
  - CRUD 호출과 startup publication을 producer mutex로 serialize하고, DB lock을 파일
    publication 밖에서 유지하며 실패 시 이전 definitions를 transaction으로 복구한다.
    `integration::write_atomic`의 temp-file/atomic-replace 및 link/reparse guard를
    사용한다.
  - request는 deny-unknown-field이며 저장/복구/읽기에서 positive created timestamp와
    `updated >= created`를 검증한다. SQLite `length(CAST(... AS BLOB))` projection을 먼저
    읽어 손상된 대형 name/query/filter를 Rust `String`으로 할당하기 전에 거부한다.
  - snapshot fixture가 query/filter 외 result/path/content가 없음을 확인한다.
- `apps/everything-plus/src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`
  - command registration 및 startup snapshot publication을 연결했다. snapshot
    실패는 고정 로그만 남기고 검색 앱은 계속 시작한다.
- `apps/everything-plus/src-tauri/Cargo.toml`, `Cargo.lock`
  - 공용 `integration` crate dependency를 추가했다.

### 3. Result actions and safety boundary

- `apps/everything-plus/src-tauri/src/core/open_targets.rs`
  - `open_in` 대상 path의 indexed membership와 final object를 `filesystem_identity`로
    확인해 등록 root 밖 row, final/ancestor symlink/reparse point 및 directory replacement를
    거부한다.
- `apps/everything-plus/src-tauri/src/commands/actions.rs`
  - 기본 open/reveal도 absolute path, traversal, exact file identity를 실행 직전에
    확인한다. ancestor link도 `filesystem::ensure_no_links`로 점검한다.
  - opener/OS error detail과 rejected path를 UI/log로 전파하지 않으며, relative,
    missing, directory, final symlink fixture를 추가했다.

### 4. Frontend filter/saved-query flow

- `apps/everything-plus/src/types.ts`
  - native metadata, `SearchFilter`, `SavedQuery`, save request, root id 타입을
    추가했다.
- `apps/everything-plus/src/api.ts`
  - optional filter를 native invoke에 전달하고 browser mock에서도 같은 filter
    projection을 흉내 낸다.
  - saved query list/save/delete API와 bounded mock state를 추가했다.
- `apps/everything-plus/src/App.tsx`, `apps/everything-plus/src/App.css`
  - extension/size/mtime/source/content-status panel, clear/filter count, saved
    query load/save(update)/delete UI를 추가했다.
  - 결과 표에 content state를 표시하고 기존 keyboard/context-menu 흐름을 유지했다.
  - query `seq`/cancelled/unmount guard를 filter 변경에도 적용해 늦은 응답이 현재
    결과를 덮지 못하게 했다. metadata와 cold/hot inbound request도 별도 generation으로
    보호하고, regex 오류 역시 이전 검색 응답에서 번지지 않게 했다. saved definition의
    UTF-8 name/query bound도 버튼 전에 확인하며, Launcher inbound Query의 filter는 같은
    normalize 경계를 거쳐 native 검색에 적용한다. UI 입력도 같은 normalize 결과를
    확인해 64개/길이·범위 역전·음수/unsafe number를 native 호출 전에 거부하고, nullable
    scalar option은 허용하되 extension 배열의 명시적 `null`은 malformed request로 거부한다.
- `apps/everything-plus/src/App.test.tsx`
  - native filter 전달과 saved definition load/no-result persistence fixture를
    추가하고 기존 app-link/context-menu mock 계약을 보존했다.

### 5. Catalog and project documentation

- `apps/catalog.json`
  - stacked base의 `catalogRevision: 10`에서 revision 11로 올리고 Everything+ producer capability
    `snapshot:everything-plus/saved-queries/v1`를 선언했다.
- `apps/everything-plus/README.md`
  - filter semantics, status/partial behavior, saved-query limits/privacy, exact
    snapshot shape, atomic writer, stale generation과 path safety를 상세히 기록했다.
- `docs/architecture.md`
  - Everything+의 native filter projection과 integration snapshot producer 흐름을
    architecture map에 반영했다.
- `docs/roadmap.md`
  - #349/#350 cohesive 범위와 현재 Launcher consumer의 strict filter 적용, release smoke를
    기록했다.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
  - 기존 P3-08 계획을 보존한 채 실제 구현 범위, acceptance fixture, privacy,
    bounded/cancellation/path 계약, root repair, snapshot compensation 및 Launcher
    compatibility를 덧붙였다.

## Key code contracts

```rust
// Everything+ native filter values are normalized before SQL/persistence.
let filter = request
    .filter
    .clone()
    .unwrap_or_default()
    .normalized()
    .map_err(|_| SAVED_QUERY_ERROR.to_string())?;
```

```rust
// A snapshot carries a definition, never a result set or result path.
payload: LauncherQueryPayload {
    text: saved.query,
    filter: saved.filter.to_applink(),
},
```

```typescript
// Every search response is accepted only for the current query/filter generation.
if (!cancelled && seq.current === current) setResults(next);
```

```typescript
// A late cold/hot request cannot replace a newer pending request.
if (!disposed && requestSeq === openRequestSeq.current && request) {
  handleOpenRequestRef.current(request);
}
```

## Verification

Executed checks:

```text
cargo test -p applink -j1
63 passed

cargo test -p filesystem -j1
17 passed

cargo test --manifest-path apps/everything-plus/src-tauri/Cargo.toml --lib -j1
121 passed

cargo test --manifest-path apps/devbox-launcher/src-tauri/Cargo.toml --lib -j1
17 passed

cargo test -p launch -j1
24 passed

cargo test -p catalog -j1
11 integration tests passed

cargo clippy -p applink -p launch -p catalog -p devbox-launcher -p everything-plus
  --all-targets -- -D warnings
exit code: 0

pnpm --dir apps/everything-plus test
2 files / 23 tests passed

pnpm --dir apps/everything-plus build
TypeScript and Vite production build passed

python3 .github/scripts/check-dependencies.py check
dependency policy and notices passed

bash .github/scripts/check-catalog.sh
exit code: 0

git diff --check
exit code: 0
```

The focused suites ran with one Rust worker. The temporary frontend dependency links were
removed immediately after the successful run, leaving the worktree clean. Latest-main full
workspace Rust/frontend gates and GitHub Actions Windows compile remain mandatory after #453 is
squash-merged and this stacked branch is rebased.

## Remaining risks and follow-up

1. Native SQLite search is bounded by query/result caps but has no interrupt handle;
   frontend stale-generation suppression prevents stale UI state while a call finishes.
   Existing index/extractor cooperative cancellation remains the authoritative long-I/O
   boundary.
2. Windows W3 validation (actual Tauri opener, reparse behavior, ACL/permission failure,
   filter save/restart/Launcher replay) and the full repository CI gates still need to run on
   the parent PR.
3. SQLite and snapshot writes are serialized and each snapshot replacement is atomic. A
   process crash can still occur between DB mutation and publication; startup publication
   repairs the external snapshot, while the command-level failure path restores the prior DB
   definitions. No partial JSON is exposed.
