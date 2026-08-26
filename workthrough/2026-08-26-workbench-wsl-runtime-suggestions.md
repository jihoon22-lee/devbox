# Workbench WSL Runtime Suggestions

## Overview

Issue #281의 P1-09 단일 기능으로 Workbench가 #410의 `wsl-desktop/runtime/v1` snapshot을
읽어 container published host port를 현재 profile editor에 제안하도록 구현했다. 기준 commit은
`bc1bb4d44c892f55fe2f5d8533acc3ad01d640a7`이다. consumer는 producer 파일이나 WSL/Docker
process를 소유하지 않으며, preview와 accept 모두 저장소를 변경하지 않는다.

## Contract

- source는 공용 integration root의 `wsl-desktop/v1/summary.json`과 `runtime` view 하나다.
- 60초 producer cadence에서 계산 freshness 120초 이하는 `fresh`, 120초 초과 900초 이하는
  `stale`, 900초 초과는 `expired`다. missing과 corrupt도 서로 다른 상태다.
- stale port 반영은 port 개수와 draft-only 효과를 고지한 확인 뒤에만 진행하고 expired,
  missing, corrupt는 반영하지 않는다.
- preview 결과를 authority로 재사용하지 않는다. Accept 직전에 snapshot을 다시 읽고 선택 port가
  최신 candidate에 모두 남아 있으며 status가 허용되는지 확인한다.
- complete view를 먼저 검증한다. distro 64개, container 256개/distro·512개 전체, mapping
  32개/container·1,024개 전체와 producer의 string/identity/state/protocol 계약을 넘으면 전체를
  corrupt로 처리한다.
- `ProjectProfile.expectedPorts`는 TCP health/start check이므로 published TCP mapping만 후보가
  된다. UDP/SCTP mapping은 유효한 snapshot data지만 목적 DTO가 protocol을 표현하지 못해 생략한다.
- port는 숫자순, source는 distro/container/state/target/protocol 순으로 정렬하고 동일 source를
  dedupe한다. 기존 draft port 순서는 보존하고 선택된 새 port만 오름차순 append한다.
- frontend에는 고정 source label, producer version, freshness, 검증된 distro/container/state와
  published/target/protocol만 보낸다. snapshot 절대 경로, container ID, raw Docker output,
  image, command, environment와 오류 원문은 보내지 않는다.

## Native Consumer

`apps/workbench/src-tauri/src/core/runtime_suggestions.rs`에 read-only consumer와 순수 검증 경계를
추가했다. `discover_report_in`이 path/link/file/envelope/freshness를 확인하고
`read_snapshot_in`이 atomic snapshot을 읽은 뒤, `deny_unknown_fields` DTO로 runtime v1의 future/raw
field를 fail-closed한다. producer/schema/view mismatch, root 오류, malformed JSON과 payload 오류는
raw detail 없는 `corrupt` 상태가 된다. 진짜 snapshot 부재는 `missing`이다.

검증을 통과한 mapping은 `BTreeMap<u16, BTreeSet<RuntimePortSourceKey>>`에 넣어 host port와
source ordering을 자료구조로 고정한다. Tauri `wsl_runtime_suggestions` command는 profile ID나
store state를 받지 않고 integration reader만 호출하므로 suggestion read 자체가 profile CRUD,
resource start 또는 외부 command로 확장되지 않는다.

Rust fixture는 다음 경계를 재현한다.

- fresh/stale/expired/missing 분류
- malformed producer와 unknown raw field의 corrupt 분류/비반향
- published port sort 및 중복 source aggregation
- invalid container/mapping 전체-view 거부
- destination이 표현하지 못하는 UDP omission

## Draft-only UI

`apps/workbench/src/lib/runtimeSuggestions.ts`의 merge는 기존 raw port text를 먼저 검증한다. invalid
중간 입력이면 원문을 그대로 남기고 fixed error를 반환한다. 유효하면 기존 순서를 유지하고 선택된
1–65535 port만 dedupe/sort해 128개 상한 안에서 append한다.

editor의 제안 panel은 source/version/age/status, host port와 각 source를 표시한다. 이미 등록된
port는 checked/disabled로 구분하고 사용자가 고른 후보만 explicit accept 대상이 된다. request
sequence는 editor switch/close/App unmount에서 pending preview·accept를 무효화한다. accept는 최신
snapshot을 다시 읽어 candidate 소실을 발견하면 selection을 교집합으로 줄이고 draft를 유지한다.
stale confirm을 거부하거나 status가 expired/missing/corrupt로 바뀌어도 draft와 store는 변하지 않는다.

App fixture는 fresh explicit accept 뒤 Save 전 `updateProfile` 미호출, stale confirm, expired block,
missing/corrupt label, accept-time candidate disappearance, editor close 뒤 late response 무시를 확인한다.
pure merge fixture는 기존 order, duplicate, invalid input preservation, unsafe port와 128개 상한을 고정한다.

## Verification

Focused native checkpoint:

```text
$ source ~/.cargo/env && cargo test -p workbench --lib -j 4
running 50 tests
test result: ok. 50 passed; 0 failed

$ source ~/.cargo/env && cargo clippy -p workbench --all-targets -j 4 -- -D warnings
Finished `dev` profile
```

Focused frontend checkpoint in the existing Linux-native dependency mirror:

```text
$ pnpm --filter workbench test
Test Files  5 passed (5)
Tests       43 passed (43)

$ pnpm --filter workbench build
tsc && vite build
✓ 43 modules transformed
✓ built
```

PR-wide final review also passed the exact rebased tree through:

```text
$ cargo test --workspace -j 4
all workspace unit/integration/doc tests passed

$ cargo check --workspace -j 4
Finished `dev` profile

$ cargo clippy --workspace --all-targets -j 4 -- -D warnings
Finished `dev` profile

$ cargo fmt --all -- --check
exit 0

$ pnpm test
17 frontend workspace projects passed

$ pnpm build
17 frontend workspace projects built successfully
```

The frontend full gate used an exact source mirror of this worktree with the existing Linux-native
dependency tree; no new install or watch process was started. `git diff --check` was clean after the
final audit. GitHub Actions remains the merge gate.

## Remaining Windows Checkpoint

Windows W1 packaged smoke must cover fresh/stale/expired transitions, WSL Desktop stopped/missing,
corrupt snapshot recovery, multiple container sources for one port, accept then cancel versus Save,
keyboard/focus, narrow panel scrolling and absence of WSL/Docker process launches during preview/accept.
