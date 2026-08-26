# Devbox Manager Batch Operations

## Overview

Issue #274의 P1-09 범위로 여러 devbox 앱을 선택해 휴대용 또는 설치 패키지 방식으로 한 번에
설치·업데이트하고, 각 앱의 결과를 독립적으로 확인한 뒤 실패한 항목만 다시 실행할 수 있게 했다.
기존 단일 설치의 asset 검증·원자 다운로드·portable version layout을 재사용하되 release manifest와
HTTP client는 batch당 한 번만 준비하고 모든 항목을 순차 처리한다.

```text
catalog + release manifest + installed state
                    │
                    ▼
       actionable target checkbox selection
                    │
       portable / installer mode 선택
                    │
       setup mode ──┴──► 다중 마법사 확인
                    │
                    ▼
 bounded request validation (1..32, unique, known target)
                    │
                    ▼
 manifest 1회 + HTTP client 1개
                    │
                    ▼
 app A ─► success/failure ─┐
 app B ─► success/failure ─┼─► ordered public results
 app N ─► success/failure ─┘
                    │
                    ├─ success ─► 유지·선택 해제
                    └─ failure ─► exact mode 선택 유지·retry
```

전체 batch를 하나의 전역 transaction으로 묶지 않는다. 앱 하나가 commit 단위이며 성공한 앱은
유지하고 실패한 앱은 가능한 상태 복구를 수행한 뒤 다음 항목을 계속한다. 이 정책은 대용량 앱을
다시 내려받거나 이미 성공한 installer를 임의로 되돌리는 것보다 사용자가 결과를 이해하고 실패만
재시도하기 쉽다.

## Context

기존 Manager는 앱 행마다 `install(appId, mode)`를 호출했다. 여러 앱 업데이트가 쌓이면 사용자는
각 행의 버튼을 반복해 눌러야 했고 다음 문제가 있었다.

- 여러 앱을 한 번에 선택하거나 전체 결과를 비교할 수 없었다.
- 단일 install command가 매번 GitHub latest release와 `release-manifest.json`을 다시 조회했다.
- 한 요청 실패 뒤 다른 앱을 계속 처리하거나 실패 항목만 기억하는 batch contract가 없었다.
- portable의 `current.json`을 먼저 바꾼 뒤 registry write가 실패하면 두 상태가 어긋날 수 있었다.
- installer spawn 뒤 registry write가 실패하면 이미 외부 process가 시작된 뒤 실패가 보고될 수 있었다.
- lower-level request/filesystem 오류를 그대로 batch 결과로 모으면 URL이나 로컬 path가 UI에 남을 수
  있었다.

## Scope

### Included

- catalog/manifest/installed state 기반 설치·업데이트 가능 앱 checkbox
- 현재 actionable target 전체 선택과 개별 선택
- portable·installer batch mode
- installer mode의 앱별 설치 마법사 실행 확인
- 최대 32개, non-empty, unique app ID와 허용 mode validation
- manager-visible/non-self-managed catalog target 재검증
- backend strict SemVer upgrade gate와 stale-selection downgrade no-op
- batch당 release manifest 1회 조회와 HTTP client 1개
- 입력 순서의 순차 실행과 앱별 ordered result
- 한 앱 실패 뒤 나머지 계속
- 성공 항목 유지·선택 해제, 실패 exact-mode 항목만 retry
- lower-level error 비노출 public result
- portable registry failure의 이전/빈 current 복구
- installer registry 선기록과 spawn failure registry 복구
- batch 진행 중 중복·row lifecycle·tab/refresh action 차단
- Rust request/DTO/registry/current fixture와 React partial-failure/retry fixture
- README, architecture, roadmap, opportunity, UX/native-first 계획 동기화

### Excluded

- 전체 batch의 all-or-nothing rollback
- 여러 다운로드의 병렬 실행
- download resume와 bandwidth scheduler
- installer 완료·exit code·실제 설치 경로 검증
- installer uninstaller 실행과 batch remove
- install executable/root/source manifest 표시 (#275)
- custom install root와 safe removal (#308, #309)
- Data Inspector, support bundle와 Related Tools (#354, #355, #365)
- Manager self-update
- 새 sidecar, runtime download dependency와 외부 package manager 연동
- Windows packaged partial-failure smoke의 개별 실행

## Changes Made

### 1. Bounded Batch Contract

`apps/devbox-manager/src-tauri/src/core/batch.rs`가 public request/result와 mutation 전 validation을
소유한다.

```rust
pub struct BatchInstallRequest {
    pub app_id: String,
    pub mode: String,
}

pub struct BatchInstallResult {
    pub app_id: String,
    pub mode: String,
    pub ok: bool,
    pub message: String,
}
```

| Boundary | Rule |
|---|---|
| item count | 1..=32 |
| app ID | 1..=64 ASCII lowercase/digit/hyphen, leading/trailing hyphen 없음 |
| duplicate | 같은 app ID가 mode와 무관하게 두 번 나오면 전체 request 거부 |
| mode | `portable` 또는 `installer` |
| catalog | manager-visible이며 self-managed가 아닌 exact ID |
| version | available strict SemVer가 installed보다 클 때만 mutation |
| result identity | 검증된 catalog app ID와 요청 mode |
| failure detail | 고정된 retry/shared-preparation 문구, 내부 오류 원문 없음 |

structural 오류와 unknown catalog target은 어떤 network/filesystem 작업보다 먼저 전체 request를
거부한다. 정상 catalog request에서 manifest나 data root라는 공통 준비가 실패하면 각 요청을 같은
안전한 실패 결과로 반환해 UI의 실패 전용 retry가 유지된다.

### 2. Manifest-hoisted Sequential Execution

`install_many` command는 다음 순서를 사용한다.

1. request shape와 catalog ownership 검증
2. latest release와 `release-manifest.json` 한 번 조회
3. Manager data root 한 번 resolve
4. `reqwest::Client` 하나 생성
5. 각 request를 입력 순서대로 `install_with_manifest`에 전달
6. 성공은 실제 success message, 실패는 원문을 버린 fixed retry result로 변환
7. 마지막 항목까지 결과 수집

기존 단일 `install`도 같은 `install_with_manifest`를 호출하므로 asset 선택, URL allowlist,
Content-Length/size/SHA-256, `.partial` streaming과 registry 경계가 batch와 갈라지지 않는다. batch는
병렬 download를 만들지 않아 memory, disk와 registry write가 동시에 증가하지 않는다.

각 request 직전에는 batch 시작 시 읽은 registry version과 manifest version을 `semver 1.0.28`로
비교한다. 미설치는 진행하고 strict upgrade만 mutation한다. 동일 version, 더 최신 installed version,
stable installed보다 낮은 prerelease available은 download 없는 성공 no-op로 반환한다. 파싱할 수 없는
version은 lexical ordering으로 추측하지 않고 해당 항목의 고정 실패 결과가 된다.

### 3. Per-app Commit and Recovery

portable은 download가 검증된 뒤 다음 순서로 commit한다.

1. 새 version directory의 executable 준비
2. 기존 `current.json`과 전체 registry snapshot 읽기
3. 이전 version을 가리키는 새 current atomic write
4. 대상 app entry만 교체한 registry atomic write
5. registry 성공 뒤 runtime metadata best-effort sync

4가 실패하면 3을 되돌린다. 이전 current가 있으면 그 원문 구조를 다시 atomic write하고, 최초
설치였다면 새 current를 제거한다. 다운로드한 version artifact는 active registry/current가 아니므로
성공으로 표시하지 않고 다음 retry 또는 기존 partial/version cleanup 정책의 대상이 된다. 복구까지
실패하면 내부 single-install 경계는 수동 확인 오류를 반환하지만 batch public result에는 absolute path나
OS 원문을 넣지 않는다.

installer는 검증된 setup executable을 준비한 뒤 다음 순서를 사용한다.

1. 원래 registry snapshot 보관
2. installer-mode entry를 atomic write
3. 검증된 setup executable spawn
4. spawn 실패 시 원래 registry 복구
5. 성공 뒤 runtime metadata best-effort sync

이 순서로 registry 기록 실패 뒤 installer가 실행되는 partial state를 막는다. spawn 성공은 설치
마법사가 시작됐다는 뜻이며 설치 완료를 뜻하지 않는다. 완료/실제 경로 확인은 별도 lifecycle 기능이
필요하므로 UI와 result message가 이 차이를 명시한다.

### 4. Multi-selection and Result UI

`App.tsx`는 manifest에 있고 현재 설치 버전과 다른 catalog row만 batch candidate로 만든다. row
selection/context-menu selection과 batch checkbox state는 분리한다. 전체 선택은 candidate만 포함하고
up-to-date·manifest missing row는 disabled다.

batch 실행 중에는 immediate ref와 React state를 함께 사용한다.

- immediate ref: 같은 render frame의 double-click과 single/batch action race 거부
- state: buttons, checkbox, tab, refresh와 모든 row lifecycle action disabled 표현
- sequential backend: frontend가 각 앱마다 별도 invoke하지 않음
- refresh: 모든 result 수신 뒤 한 번만 수행

결과 panel은 display name, mode, 성공/실패와 bounded message를 ordered list로 표시한다. 실패가 있으면
`실패 항목만 재시도 (N)` 버튼이 나타나고, 이전 result의 exact `{appId, mode}`만 다음 request에
사용한다. 성공 항목은 retry에 포함되지 않는다.

### 5. API and Styling

- `src/types.ts`: `InstallMode`, `BatchInstallRequest`, `BatchInstallResult` 추가
- `src/api.ts`: Tauri/mock이 같은 `installMany(requests)` 계약 사용
- `src/App.css`: batch action bar, bounded result scroll, success/failure border, responsive result grid
- 새 runtime package, Cargo crate, storage key와 schema 없음

## Security and Failure Boundaries

- frontend는 asset URL, version directory, registry/executable path를 선택하지 않는다.
- backend가 catalog ownership과 release manifest asset을 다시 결정한다.
- backend SemVer gate가 stale UI의 equality-only candidate를 downgrade로 바꾸지 못하게 한다.
- invalid/duplicate/oversized request는 shared state를 바꾸기 전에 실패한다.
- download는 기존 request/final redirect allowlist와 size/digest 검증을 그대로 사용한다.
- public failure result는 caught error의 `Display`/`Debug`를 serialize하지 않는다.
- 성공한 앱을 실패한 다른 앱 때문에 rollback하거나 재다운로드하지 않는다.
- failed portable commit은 active current를 복구한 경우에만 일반 retry failure가 된다.
- installer batch는 explicit confirm 뒤에만 호출되며 success 의미는 process spawn이다.
- batch는 remove, uninstaller, custom root, arbitrary path와 credential input을 받지 않는다.

## Files Changed

- `apps/devbox-manager/src-tauri/src/core/batch.rs`
- `apps/devbox-manager/src-tauri/Cargo.toml`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `apps/devbox-manager/src-tauri/src/core/mod.rs`
- `apps/devbox-manager/src-tauri/src/commands/manager.rs`
- `apps/devbox-manager/src-tauri/src/lib.rs`
- `apps/devbox-manager/src/types.ts`
- `apps/devbox-manager/src/api.ts`
- `apps/devbox-manager/src/App.tsx`
- `apps/devbox-manager/src/App.css`
- `apps/devbox-manager/src/App.test.tsx`
- `apps/devbox-manager/README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/product-opportunities.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

## Verification

### Rust

```text
cargo test -p devbox-manager --jobs 1
51 passed; 0 failed

cargo check -p devbox-manager --jobs 1
passed
```

fixture coverage:

- unique bounded portable/installer request acceptance
- empty, 33-item, duplicate, traversal-like app ID와 unknown mode rejection
- lower-level path-like error가 public result에 없는지 확인
- 미설치/strict upgrade 허용, equal/newer/prerelease downgrade no-op와 invalid SemVer 거부
- registry update가 unrelated app entry를 그대로 보존하고 target만 교체
- failed portable registry commit 뒤 이전 current 복구와 최초 current 제거
- 기존 manifest, asset, URL, digest, layout, managed-install, runtime-metadata 회귀

### Frontend

```text
pnpm --filter devbox-manager exec vitest run --maxWorkers=1
Test Files  1 passed (1)
Tests       10 passed (10)

pnpm --filter devbox-manager build
TypeScript compile: passed
Vite: 40 modules transformed, production build passed
```

새 fixture는 두 앱 batch에서 첫 앱 failure와 두 번째 app success를 동시에 반환한다. UI가 두 request를
한 번만 보냈는지, single install을 호출하지 않았는지, success checkbox를 해제하고 failed checkbox만
유지하는지, retry가 정확히 실패한 app/mode 하나만 보내는지 확인한다. setup fixture는 confirm 거부 시
호출이 없고 승인한 exact app만 installer mode로 보내는지 검증한다. 기존 context-menu 8개 fixture도
같은 단일 worker run에서 통과했다.

### Dependency Review

| Item | Decision |
|---|---|
| Purpose | backend가 numeric/prerelease SemVer ordering으로 batch downgrade 차단 |
| Alternative | frontend-only/equality 비교는 stale invoke를 막지 못하고 수제 parser는 ordering 오류 위험 |
| Source/version | dtolnay/semver, crates.io `1.0.28` exact direct dependency |
| License | MIT OR Apache-2.0, generated notices에 기존 수록 |
| Install/bundle | workspace lock에 이미 존재; frontend/Tauri resource의 신규 package payload 없음 |
| Offline/update | 정적 Rust dependency, runtime network 0; Cargo advisory/license CI와 Dependabot가 추적 |

### Bundle

| Asset | #273 main | #274 | Delta |
|---|---:|---:|---:|
| JS | 214,628 B | 218,297 B | +3,669 B |
| JS gzip (`gzip -n`) | 66,825 B | 67,954 B | +1,129 B |
| CSS | 4,691 B | 5,721 B | +1,030 B |
| CSS gzip (`gzip -n`) | 1,573 B | 1,834 B | +261 B |

### PR-wide Gates

- `cargo fmt --all --check`: passed
- `cargo clippy -p devbox-manager --all-targets --jobs 1 -- -D warnings`: passed
- workspace `cargo test --workspace --jobs 1`: passed
- workspace `cargo check --workspace --jobs 1`: passed
- `NODE_OPTIONS=--max-old-space-size=768 pnpm -r --workspace-concurrency=1 build`: passed
- `pnpm install --frozen-lockfile --prefer-offline`: passed
- `pnpm audit --audit-level moderate`: no known vulnerabilities
- dependency notice generation/check and dependency regression tests: passed
- build-manifest notice tests: passed
- `cargo deny --locked check`: advisories, bans, licenses, sources passed
- catalog consistency: passed
- GitHub Actions Linux/frontend/dependency/catalog/Windows gates: PR에서 확인 예정

## Known Boundaries and Next Steps

- W1: packaged Windows에서 두 portable 항목 중 첫 digest/download failure 뒤 두 번째 success
- W1: 실패 결과만 retry해 첫 항목 성공, 기존 성공 항목의 재다운로드 부재 확인
- W1: portable registry write failure fixture에서 previous current/executable 유지 확인
- W1: setup 2개 확인 뒤 검증된 installer 창이 각각 열리고 결과가 "실행"으로 표시되는지 확인
- W1: batch 진행 중 row/context/tab/refresh와 double click이 disabled인지 시각 evidence
- process/OS crash를 가로지르는 batch journal은 제공하지 않는다.
- #275가 executable/root/source manifest의 read-only install path 표시를 별도 PR로 구현한다.
- custom root/safe removal, Data Inspector/support bundle/Related Tools는 P2/P3 issue 경계를 유지한다.
- Devbox Manager 0.4.0 version bump는 Wave 9 release preparation에서 별도로 수행한다.
