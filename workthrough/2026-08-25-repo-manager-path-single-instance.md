# Repo Manager Path Single-Instance Repository Selection

## Overview

P1-04-R [#243](https://github.com/jihoon22-lee/devbox/issues/243)의 Repo Manager
inbound Path 수신을 구현했다. catalog revision 4에서 Repo Manager가 `path` capability를
선언하고, cold-start argv와 이미 실행 중인 instance 재호출을 동일한 one-shot
`PendingOpen` 경로로 frontend에 전달한다.

수신 경로는 backend에서 절대 경로, traversal, 존재 여부와 Git repository 여부를 검증한다.
현재 scan 목록과 canonical identity가 같으면 해당 repository card를 선택하고 focus한다. 목록에
없는 유효한 repository는 자동 등록하거나 Git 명령을 실행하지 않고 비지속 “등록 초안”으로만
표시한다. 사용자가 `이 경로 탐색`을 명시적으로 누를 때에만 기존 read-only scan을 실행한다.

## Context

Repo Manager는 이미 다른 devbox 앱을 catalog 기반으로 여는 송신 기능과 repository 경로
검증기를 갖고 있었지만 inbound capability는 없었다. `--path`로 실행하면 별도 process가 열릴
수 있었고, 이미 표시된 repository 선택이나 목록 밖 경로를 안전하게 검토하는 흐름도 없었다.

이 기능은 다음 경계를 동시에 만족해야 했다.

1. cold/hot 요청은 같은 pending pull 경로로 한 번만 적용한다.
2. hot relaunch는 기존 창을 show/unminimize/focus한다.
3. inbound raw path를 frontend 신뢰만으로 파일시스템 또는 Git command에 넘기지 않는다.
4. 기존 목록 miss를 조용히 무시하지 않되, 자동 등록·지속 저장·Git mutation도 하지 않는다.
5. invalid path와 parser 오류는 raw 입력을 로그나 오류에 반향하지 않는다.
6. Git history, worktree cleanup, arbitrary path write는 별도 후속 issue로 남긴다.

## Changes Made

### 1. Catalog revision and repository contracts

- `apps/catalog.json`
  - `catalogRevision`을 3에서 4로 증가시켰다.
  - `repo-manager.accepts`를 `path`로 선언했다.
  - 다른 12개 앱의 capability는 변경하지 않았다.
- `crates/catalog/tests/catalog.rs`
  - repository revision 4를 고정했다.
  - path target 순서에 `repo-manager`가 추가됐는지 검증한다.
  - query target 계약은 `everything-plus`, `knowledge-base` 그대로 유지한다.
- `apps/devbox-manager/src-tauri/src/core/catalog.rs`
  - Manager build-time adapter가 revision 4와 Repo Manager Path capability를 검증한다.
  - Knowledge와 Everything+의 선행 capability assertion도 유지한다.

Catalog fixture의 revision 5/6 값은 runtime freshness와 fake-sixteenth 시나리오를 위한 독립
fixture이므로 변경하지 않았다.

### 2. Backend single-instance delivery

- `apps/repo-manager/src-tauri/Cargo.toml`
  - 기존 lock graph에 있던 `tauri-plugin-single-instance = "2"`를 직접 연결했다.
- `apps/repo-manager/src-tauri/src/applink.rs`
  - `Mutex<Option<OpenRequest>>` 기반 `PendingOpen`을 추가했다.
  - `take_pending_open`은 반환과 동시에 slot을 비운다.
  - 소비 전 요청이 연속 도착하면 newest request가 이전 request를 교체한다.
- `apps/repo-manager/src-tauri/src/lib.rs`
  - single-instance plugin을 opener와 setup보다 먼저 등록했다.
  - hot argv는 parse → pending set → `devbox://open` emit → window restore/focus 순서다.
  - cold argv도 setup에서 같은 parser와 pending state에 넣는다.
  - parser 실패는 raw argv를 포함하지 않는 `applink: invalid request`만 기록한다.

```rust
// apps/repo-manager/src-tauri/src/lib.rs
.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
    match devbox_applink::parse_argv(&args) {
        Ok(Some(request)) => {
            app.state::<applink::PendingOpen>().set(request.clone());
            let _ = app.emit("devbox://open", request);
        }
        Ok(None) => {}
        Err(_) => eprintln!("applink: invalid request"),
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}))
```

### 3. Validated repository metadata boundary

- `apps/repo-manager/src-tauri/src/commands.rs`
  - 기존 outbound `open_in` 경로 검증을 `validated_repository`로 재사용 가능하게 정리했다.
  - `repository_entry`가 canonical filesystem 존재와 `.git`을 확인하고 `RepoEntry`만 만든다.
  - 새 `prepare_inbound_repository` command는 metadata만 반환한다.
  - 이 command는 DB/config 쓰기, repository 등록, scan, `git` subprocess를 수행하지 않는다.
  - relative path, `.`/`..` segment, missing path, non-repository를 고정 오류로 거부한다.
  - scan과 inbound metadata가 같은 canonical identity를 만드는지 fixture로 검증한다.

```rust
// apps/repo-manager/src-tauri/src/commands.rs
#[tauri::command]
pub fn prepare_inbound_repository(path: String) -> Result<RepoEntry, String> {
    validated_repository(&path).map_err(str::to_string)
}
```

유효한 path 자체는 사용자가 요청한 repository를 식별하기 위해 card 또는 draft에 표시한다.
반대로 검증에 실패한 raw path는 오류 문자열, log, persisted state에 포함하지 않는다.

### 4. Listener-first frontend routing

- `apps/repo-manager/src/api.ts`
  - applink wire shape과 일치하는 `OpenTarget`, `OpenRequest`를 추가했다.
  - `takePendingOpen`, `onOpenRequest`, `prepareInboundRepository` wrapper를 추가했다.
- `apps/repo-manager/src/lib/applink.ts`
  - Path target만 허용한다.
  - empty, 32,767자 초과, NUL path를 generic 오류로 거부한다.
  - Windows canonical key는 filesystem의 case-insensitive 의미에 맞게 대소문자 없이 비교하고,
    WSL/Linux key는 case를 보존한다.
- `apps/repo-manager/src/App.tsx`
  - 초기 repository 목록이 준비되면 Git status/worktree hydration을 기다리지 않고 event
    listener를 먼저 등록한 뒤 cold pending을 pull한다.
  - hot event payload는 trigger로만 사용하며 authoritative request는 pending slot에서 다시 take한다.
  - backend가 반환한 canonical key와 현재 list가 일치하면 card를 선택·scroll·focus한다.
  - 목록 miss는 `registrationDraft` UI state로만 유지한다.
  - draft 확인 전에는 scan, Git status, worktree command, persistence를 실행하지 않는다.
  - `이 경로 탐색`을 누르면 기존 read-only scan을 실행하고 결과 card를 선택한다.
  - scan 실패 시 이전 draft의 pending selection을 제거해 후속 scan에 잘못 재사용하지 않는다.
  - request sequence와 pending selection 취소로 늦은 과거 validation/scan 의도가 최신 hot
    request를 덮어쓰지 못하게 했다.
- `apps/repo-manager/src/App.css`
  - 선택 card와 등록 초안의 구분 가능한 상태를 추가했다.

```typescript
// apps/repo-manager/src/App.tsx
const inbound = await prepareInboundRepository(action.path);
if (sequence !== openSequenceRef.current) return;
const match = reposRef.current.find((repo) =>
  sameRepositoryKey(repo.canonicalKey, inbound.canonicalKey),
);
if (match) {
  setRegistrationDraft(null);
  setSelectedRepoKey(match.canonicalKey);
} else {
  setSelectedRepoKey(null);
  setRegistrationDraft(inbound);
}
```

### 5. Tests

- `apps/repo-manager/src-tauri/src/applink.rs`
  - one-shot take와 newest-wins를 검증한다.
- `apps/repo-manager/src-tauri/src/commands.rs`
  - absolute existing Git directory만 수락하는지 검증한다.
  - relative/traversal/non-repo를 generic 오류로 거부하는지 검증한다.
  - secret-shaped invalid suffix가 오류에 반향되지 않는지 검증한다.
  - inbound metadata와 scan identity가 일치하는지 검증한다.
- `apps/repo-manager/src/lib/applink.test.ts`
  - bounded Path, invalid Path, unsupported target, Windows/WSL key 비교를 검증한다.
- `apps/repo-manager/src/App.applink.test.tsx`
  - listener-before-take cold selection과 card focus를 검증한다.
  - stale event payload 대신 pending hot Path를 적용하는지 검증한다.
  - 늦은 이전 validation이 최신 hot selection을 덮지 않는지 검증한다.
  - 목록 밖 repository가 자동 mutation 없이 draft로 남는지 검증한다.
  - 명시적 확인 뒤에만 scan하고 결과를 선택하는지 검증한다.
  - invalid Path의 generic recoverable error와 listener 실패 cold fallback을 검증한다.

### 6. Documentation and dependency notices

- `apps/repo-manager/README.md`
  - Path receive, 선택/draft UX와 privacy/mutation 경계를 기록했다.
- `docs/architecture.md`
  - catalog revision 4, single-instance pending, backend validation과 non-persistent draft를 기록했다.
- `Cargo.lock`, `THIRD_PARTY_NOTICES.md`
  - Repo Manager의 기존 locked single-instance dependency direct edge와 provenance hash만 갱신했다.
  - 새 transitive dependency나 runtime download는 없다.

## Verification Results

로컬 자원 사용을 제한하기 위해 package 단위, Cargo `-j 1`, frontend 단일 worker로 실행했다.
의존성 tree는 main worktree에서 임시 symlink하고 exit trap으로 즉시 제거했다.

### Rust

```text
cargo test -p repo-manager --lib -j 1
15 passed; 0 failed

cargo test -p catalog --test catalog -j 1
11 passed; 0 failed

cargo test -p devbox-manager --lib -j 1
37 passed; 0 failed

cargo check -p repo-manager -j 1
passed

CARGO_BUILD_JOBS=1 cargo clippy -p repo-manager --all-targets -- -D warnings
passed

cargo fmt --all
passed
```

### Frontend

```text
pnpm exec tsc --noEmit
passed

pnpm exec vitest run --maxWorkers=1
3 files, 16 tests passed

pnpm exec vitest run src/App.applink.test.tsx --maxWorkers=1
7 tests passed after the final listener/selection ordering adjustment

NODE_OPTIONS=--max-old-space-size=1024 pnpm build
37 modules transformed; production build passed
```

### Repository gates

```text
bash .github/scripts/check-catalog.sh
passed

python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match lockfiles

python3 .github/scripts/test-check-dependencies.py
passed

git diff --check
passed
```

## Follow-up Boundaries

- Windows packaged cold/hot Path, second-instance restore/focus와 실제 card focus evidence는 P1
  병합 후 W1 checkpoint에서 남긴다.
- Git history/diff/stage/commit/fetch/pull/push는 P2 Repo Manager issue다.
- safe worktree/branch cleanup은 선택 P3 issue이며 이 PR은 remove 동작을 추가하지 않는다.
- protocol v2 Handoff와 전 앱 context menu는 별도 기능 단위 PR이다.
- 등록 초안의 지속 저장 모델은 현재 계획에 없으며, 필요하면 별도 product decision이 선행돼야 한다.
