# Devbox Manager local-quality inspection

**Date:** 2026-08-31
**Issue:** #491 (W10 PR B)
**App:** `apps/devbox-manager`
**Status:** implemented; local/required CI and v0.6.0 #493 hosted Windows package gate passed;
user-specific accessibility observations remain environment-dependent

## Goal and boundary

Devbox Manager의 `로컬 품질` 탭은 사용자가 누른 명시적 새로고침 한 번에만 현재 로컬
설치 상태와 integration snapshot 상태를 읽기 전용으로 확인한다. 결과는 `schemaVersion: 1`,
`mode: "local-only"`인 bounded DTO로만 현재 프로세스 메모리에 남는다. 이 기능은 telemetry,
remote network, local-quality persistence, 자동 refresh, 설치·수정·삭제를 제공하지 않는다.

브라우저 fallback은 화면 흐름을 위한 bounded fixture일 뿐 native filesystem, registry 또는
integration discovery가 성공했다는 증거가 아니다.

## User flow

1. `로컬 품질` 탭을 열면 inspection은 시작하지 않는다.
2. 사용자가 `상태 새로고침`을 눌렀을 때만 `inspect_local_quality`를 호출한다.
3. 새 결과가 성공하면 현재 snapshot을 교체하고, 실패하면 고정된 안전 문구만 표시하면서
   마지막 정상 snapshot을 유지한다.
4. 다음 새로고침은 항상 새 관찰을 수행한다. 이전 결과를 자동으로 재검증하거나 저장하지 않는다.

## Native contract

### Envelope

`inspect_local_quality`는 입력 없이 다음 필드만 반환한다.

| Field | Contract |
| --- | --- |
| `schemaVersion` | 정확히 `1` |
| `observedAtMs` | 양의 JavaScript safe integer |
| `mode` | 정확히 `local-only` |
| `status` | `healthy` 또는 `attention`; 하위 상태와 일관돼야 함 |
| `installation` | catalog/validated registry projection |
| `integration` | bounded `discover_report` projection |

`healthy`는 catalog와 registry가 모두 ready이고 설치 목록 truncation이 없으며, integration
root가 ready이고 격리된 issue와 모든 collection/view truncation이 없을 때만 사용한다. 그 밖의
상태는 `attention`이다.

### Installation projection

Manager는 현재 catalog에서 `manager_visible && !self_managed`인 앱 ID와 revision만 읽고,
검증된 registry에서는 app ID/version/mode와 revision만 읽는다. executable, install root,
manifest, locator 또는 source path는 DTO에 들어가지 않는다.

Registry가 없거나 검증에 실패하면 `registryState`는 `unavailable`, registry revision과
`installedAppCount`는 `null`이며 표시되는 모든 앱은 `unknown`이다. registry를 읽을 수 없다는
이유만으로 `not-installed`를 주장하지 않는다. registry가 ready일 때만 record가 있는 앱을
`installed`, 없는 앱을 `not-installed`로 표시하며, 설치된 앱의 version은 bounded SemVer이고
mode는 `portable` 또는 `installer`다.

### Integration projection

native는 `crates/integration::discover_report()`를 호출하되 public DTO로 투영하면서 각
snapshot의 `path`와 `generatedAt`, 각 issue의 raw error text 및 root raw error를 제거한다.
snapshot에는 producer/schema/producer version/freshness와 bounded view summary만 남고,
issue에는 producer/schema와 다음 고정 enum만 남는다.

```text
invalid | unreadable | unsafe | limit-exceeded
```

root error도 같은 enum으로 분류한다. 원본 경로·오류·환경변수는 화면, 로그, telemetry로
반향하지 않는다.

### Resource limits

| Collection/value | Limit |
| --- | ---: |
| Installation app rows | 64 |
| Integration snapshots | 64 |
| Integration issues | 64 |
| Views per snapshot | 16 |
| Serialized local-quality DTO | 256 KiB |

원본 total count는 count 필드로 보존하고, visible collection을 cap한 경우 해당 `truncated`
flag를 true로 표시한다. native는 JSON serialize 후 256 KiB를 초과하면 고정 오류로 실패한다.

## Frontend trust boundary

`src/api.ts`는 native 또는 browser fixture를 React로 넘기기 전에 exact-key와 관계를 모두
검증한다.

- envelope, installation, app, integration, snapshot, view, issue는 각각 허용된 key만 가진다.
- app/producer/view ID는 bounded kebab-case이고 중복이 없다. version은 bounded SemVer,
  schema/revision/count/freshness는 safe integer 범위다.
- `installed`만 version/mode를 가지며 `not-installed`와 `unknown`은 둘 다 `null`이다.
- catalog unavailable은 empty rows/count와 registry unavailable을 함께 요구한다. registry
  unavailable은 revision/count가 `null`이고 모든 app state가 `unknown`이어야 한다. registry
  ready에서는 `installedAppCount`가 실제 installed row 수와 같고 `unknown` row가 없다.
- visible length, total count, truncation flag는 각 cap과 일치해야 한다. root state와 root issue,
  snapshot/view key uniqueness, root unavailable 시 empty result, `status`와 `attention` 조건도
  서로 일치해야 한다.

검증에 실패한 응답은 `로컬 품질 응답이 올바르지 않습니다.`라는 고정 오류로 닫힌다.

## Async and accessibility behavior

- inspection region은 작업 중 `aria-busy="true"`를 노출하고 완료 후 false가 된다.
- 공용 `readBusy` 경계로 다른 Manager read/mutation과 중복 inspection을 막는다.
- request ID와 mounted guard가 늦은 응답, 이전 요청의 응답, unmounted component 뒤의 응답을
  모두 폐기한다. cleanup은 pending generation을 무효화한다.
- native 오류가 뒤늦게 도착해도 마지막 정상 snapshot은 지우지 않으며, raw native error는
  렌더링하지 않는다.
- empty state와 오류는 status/alert semantics를 사용하고, 상태는 명시적 버튼의 결과로만
  갱신된다.

## Acceptance and exclusions

현재 구현의 local evidence는 다음 문서에 기록한다.

- Devbox Manager frontend: 71/71 tests passed across two test files.
- Workspace frontend: 1,488/1,488 tests and all 15 production builds passed.
- local-quality Rust core: 8/8 tests passed.
- Manager production bundle: JavaScript raw 312,992 bytes, gzip 93,326 bytes; budgets are
  345,000 and 105,000 bytes.
- All 15 frontend bundle budgets and the accessibility contract passed.
- Rust workspace `check`, `clippy --all-targets -D warnings`, `fmt --check`, and `test` passed.
- Catalog, dependency/notices, checker fixtures, pnpm audit, and cargo-deny policy passed. The
  pre-existing duplicate/yanked warnings remain release follow-up input rather than a new failure.
- `git diff --check`: passed.

The feature PR's required GitHub Actions CI and the later v0.6.0 #493 hosted package gate passed.
Physical Windows screen reader and high-contrast behavior still depends on the installed user's
environment; this source evidence does not claim every such configuration was observed.

This PR makes no RC, tag, release, or app version bump. It only adds the existing workspace
`integration` crate to the Manager dependency graph, so `Cargo.toml` and `Cargo.lock` record that
wiring without introducing a new third-party dependency.

## Implementation map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/core/local_quality.rs` | Bounded projection, fixed enums, status rules, privacy and eight Rust tests |
| `src-tauri/src/commands/local_quality.rs` | Explicit command, local observation, serialized-size guard |
| `src-tauri/src/commands/manager.rs` | Catalog and validated registry path-free observations |
| `src/api.ts` and `src/types.ts` | Browser fixture, DTO types, exact-key/relationship validation |
| `src/App.tsx` | Explicit refresh UI, `aria-busy`, last-good and stale/unmount guards |
| `src/App.test.tsx` and `src/api.test.ts` | UI/API contract and accessibility regression coverage |
