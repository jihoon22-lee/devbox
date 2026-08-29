# Run Manager Launcher Snapshot (#474)

## Overview

Run Manager의 기존 flat status snapshot은 `run-manager/v1/summary.json`으로 정확히
유지하고, Launcher용 job/service action은 같은 producer/version 디렉터리의 named
`jobs-services.json` sidecar로 별도 발행한다. 기존 Workbench/Life Log와 구버전 Launcher는
summary만 읽고, 새 Launcher는 sidecar를 우선 읽은 뒤 sidecar가 없을 때만 summary의
active-service fallback을 사용한다.

이 기록은 v0.5.0 tag 이후의 #474 post-release source correction이다. 공개 v0.5.0 binary에는
포함되지 않는다.

## Context

기존 producer는 active service와 오늘의 실행 통계만 flat `data`에 기록했다. Launcher에는
이미 `jobs-services` source와 legacy flat fallback reader가 있었지만, Run Manager가 실제
job/service action entry를 발행하지 않아 검색·실행 결과가 비어 있었다. `data.views`가
추가된 envelope을 독립 배포된 기존 status consumer가 선택하지 않도록 `summary.json`을
그대로 보존하고 named sidecar를 별도 capability로 추가했다. sidecar는 기존 discovery의
`summary.json` 스캔 대상이 아니므로 Life Log의 newest-version 선택도 영향을 받지 않는다.

## Changes Made

### Run Manager producer

- `apps/run-manager/src-tauri/src/integration.rs`
  - 기존 flat status를 `run-manager/v1/summary.json`에 먼저 원자 교체한다.
  - `run-manager/v1/jobs-services.json`에 envelope schema v1과 `jobs-services` view 하나를
    원자 교체한다.
  - 앱 시작 시 첫 snapshot을 즉시 발행하고 이후 60초마다 갱신한다.
  - storage의 `id`, `kind`, `name` 전용 read-only projection을 `LIMIT 2,049`로 읽어
    bound 초과를 fail-closed한다. command, cwd, environment column은 projection하지 않는다.
  - 모든 job/service를 ID로 정렬하고 최대 2,048개까지 포함한다.
  - action entry는 `id`, bounded `label`/`detail`, `targetApp`, `targetKind`,
    `payloadVersion`, opaque `{id}` payload만 갖는다.
  - command, cwd, environment, path, credential, log 원문은 직렬화하지 않는다. absolute/UNC/
    file URI와 명백한 relative path 형태의 이름은 fallback label로 대체하되 `Build/API` 같은
    일반 이름은 보존한다.
  - invalid/credential-like ID, 중복 ID, 범위 초과 데이터는 sidecar 갱신을 거부한다.
  - sidecar가 overflow/failure면 v1 status는 갱신되고 sidecar last-good 파일은 보존된다.
  - jobs/services, status shape, privacy, path fallback, bounds/duplicate, last-good 회귀
    테스트를 추가했다.

### Integration contract

- `crates/integration/src/lib.rs`
  - `<producer>/v<version>/<kind>.json` named-view path를 위한 kebab-case kind 검증을
    추가했다.
  - named read/write가 producer/version identity, root/producer/version 및 target
    symlink/reparse 방어, envelope depth/size 검증, atomic write를 기존 contract와 공유한다.
  - named file은 `data.views`가 정확히 하나이고 key가 filename kind와 같을 때만 허용한다.
  - reserved `summary` kind와 traversal/underscore/dot 이름을 거부하며, summary discovery는
    named sidecar를 무시한다.

### Launcher and catalog

- `apps/devbox-launcher/src-tauri/src/core/launcher.rs`
  - SourceSpec에 source별 snapshot version과 optional named sidecar를 추가했다.
  - Run Manager는 `jobs-services.json` sidecar를 primary로 읽는다.
  - sidecar가 missing일 때만 `summary.json` flat status를 fallback으로 읽으며, corrupt/
    permission/symlink sidecar는 fallback하지 않고 fail-closed한다.
  - logical diagnostic/result source와 target app은 계속 `run-manager`다.
- `apps/catalog.json`, `crates/catalog/tests/catalog.rs`,
  `apps/devbox-manager/src-tauri/src/core/catalog.rs`
  - `snapshot:run-manager/status/v1`과 `snapshot:run-manager/jobs-services/v1` capability를
    catalog revision 12에 등록했다.
- `apps/run-manager/README.md`, `apps/devbox-launcher/README.md`, `docs/architecture.md`,
  `docs/roadmap.md`, `docs/superpowers/specs/2026-08-17-app-interop-design.md`,
  `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
  - sidecar protocol, independent-app compatibility, privacy/bounds, #474 및 post-release
    binary 범위를 문서화했다.
- Workbench와 Life Log 소스는 변경하지 않았다. 둘은 기존 v1 flat summary를 그대로 읽는다.

## Code Examples

Named sidecar path:

```text
%LOCALAPPDATA%/devbox/integration/run-manager/v1/jobs-services.json
```

Sidecar `data.views.jobs-services.entries`의 action shape:

```json
{
  "id": "job-123",
  "label": "Build",
  "detail": "Run Manager · job",
  "targetApp": "run-manager",
  "targetKind": "task",
  "payloadVersion": 1,
  "payload": { "id": "job-123" }
}
```

`summary.json`의 `data`는 기존 flat `activeServices`, `runs`, `lastRunAtMs` shape만 가진다.

## Verification Results

- `CARGO_BUILD_JOBS=1 cargo test -p run-manager --lib` — 216 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p devbox-launcher --lib` — 21 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p catalog --test catalog` — 11 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p devbox-manager core::catalog::tests --lib` — 2 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p life-log --lib` — 101 passed; 기존 flat consumer 회귀 없음.
- `CARGO_BUILD_JOBS=1 cargo test -p workbench --lib` — 117 passed; 기존 flat consumer 회귀 없음.
- `CARGO_BUILD_JOBS=1 cargo test -p integration --lib` — 17 passed.
- `CARGO_BUILD_JOBS=1 cargo check -p run-manager -p devbox-launcher` — passed without warnings.
- `cargo fmt --all -- --check` 및 `git diff --check` — passed.
- `.github/scripts/check-catalog.sh` — passed (15 app contracts/release catalog).

## Risks and Follow-up

- `pnpm build`는 이 Rust/catalog/docs 전용 변경에서 실행하지 않았다.
- Linux focused Rust tests/checks와 catalog validation으로 producer/consumer 계약을 확인한다.
- Windows packaged acceptance는 현재 환경에서 수행하지 않으며, snapshot 경로·Launcher cold
  action·Run Manager task 재검증은 별도 release checkpoint로 남긴다.
- 이 source correction은 v0.5.0 tag 이후의 #474 보강이며 공개 v0.5.0 binary에는 포함되지 않는다.
- worktree는 의도적으로 dirty 상태로 두며 commit/push/PR은 수행하지 않는다.
