# Run Manager Job, Service, and History Context Menus

## Overview

Issue #254의 P1-06-Run 범위로 Run Manager의 작업, 서비스, 실행 이력 행에
`@devbox/context-menu`를 적용했다. mouse right-click, Shift+F10, Menu key가 같은 action 경로를
사용하고 메뉴가 열리기 전에 대상 행을 선택하며 닫힌 뒤 원래 행으로 focus를 복구한다.

메뉴에 기존 버튼을 단순 복제하는 데서 끝내지 않고 실제 lifecycle snapshot과 backend state machine을
대조했다. 그 결과 기존 `retry_waiting` 서비스는 UI에서 정지나 재시작 의도를 표현할 방법이 없었고,
backend command를 직접 호출해도 pending backoff를 취소하지 못한다는 결함을 발견했다. 명시적 stop은
예약된 재시도를 취소하고, restart는 대기를 건너뛰어 새 generation을 시작하도록 저장 전이를 함께
보강했다.

실행 이력의 “로그 저장”은 외부 프로그램이나 임의 파일 경로를 연결하지 않는다. 기존 Run Manager가
소유하고 backend에서 경계를 검증하는 회전 로그만 decimal cursor로 읽으며, 현재 stdout/stderr 한
스트림을 최대 50MiB까지 저장한다.

Quick Open, log search, task import, Log Lens handoff, status snapshot 생산은 이 PR 범위가 아니다.
각 항목은 v0.5.0 계획의 별도 P1/P2/P3 기능 PR이 소유한다.

## Context

- 작업과 서비스에는 inline button이 있었지만 keyboard context menu가 없고, 실행 이력에는 선택·로그
  열람 외에 해당 행 재실행과 로그 저장 action이 없었다.
- context menu를 연 행과 이전 선택이 다를 수 있어 action 대상이 stale selection에 의존하면 다른
  작업을 실행하거나 삭제할 위험이 있었다.
- 서비스 목록의 최초 load는 정의만 읽고 인스턴스 상태를 가져오지 않아, 첫 refresh 전까지 실행 중인
  서비스를 stopped처럼 표시할 수 있었다. 상태 기반 disabled 항목에는 이 오차를 허용할 수 없다.
- 작업 enable 토글을 일반 `updateJob` DTO로 구현하면 frontend에 공개되지 않는 secret environment를
  clear할 위험이 있다. backend의 좁은 `set_job_enabled` command를 직접 소비해야 한다.
- `retry_waiting`은 live process가 없지만 자동 재시작 예약을 가진 lifecycle 상태다. 기존
  `begin_service_stop`은 running/starting만 받았기 때문에 명시적 stop이 no-op이고 restart도 이어지는
  stopped claim에서 실패했다.
- Run Manager 로그는 stream당 10MiB segment 5개를 유지한다. frontend가 app-local 경로나 전체
  프로세스 정의를 export에 끌어들이지 않고 기존 `tail_log` capability만 재사용해야 한다.
- cursor는 `u64` 범위를 가질 수 있으므로 JavaScript `number`로 변환하면 2^53 이후 정밀도를 잃는다.

## Changes Made

### 1. 공용 메뉴 primitive 연결과 대상 우선 선택

Files:

- `apps/run-manager/package.json`
- `pnpm-lock.yaml`
- `apps/run-manager/src/App.tsx`
- `apps/run-manager/src/App.css`
- `apps/run-manager/src/components/RunHistory.tsx`

Run Manager에 기존 workspace package `@devbox/context-menu` direct-consumer edge를 추가했다. 새 registry
package, binary, sidecar, network service는 없다.

각 trigger는 opaque row ID만 `data-*`에 둔다.

- job card: `data-job-id`
- service card: `data-service-id`
- history button: `data-run-id`

공용 hook의 `onBeforeOpen`에서 현재 collection으로 ID를 다시 찾고 선택 상태와 action snapshot을 함께
갱신한다. 항목이 refresh 또는 delete로 사라지면 열린 메뉴와 stale snapshot을 닫고, 화면이 바뀌어도
이전 화면의 메뉴가 portal에 남지 않게 정리한다.

작업·서비스 card는 focusable listitem으로, 실행 이력은 기존 button listitem으로 유지했다.
`aria-current`/`aria-pressed`와 selected style이 pointer 및 keyboard target을 같은 방식으로 표현한다.
위치 계산, viewport flip, 화살표 navigation, disabled skip, Esc close, focus restore는 공용 package가
소유한다.

### 2. 작업 메뉴와 좁은 enable command

Files:

- `apps/run-manager/src/App.tsx`
- `apps/run-manager/src/api.ts`
- `apps/run-manager/src/App.test.tsx`

정확한 topology:

```text
지금 실행
활성화 | 비활성화
편집
로그 열기
────────
삭제 (danger)
```

- “로그 열기”는 우클릭한 job ID를 `RunHistory.requestedJobId`로 전달해 이전 filter가 아니라 해당 작업의
  최근 50회 history를 연다.
- enable 토글은 `set_job_enabled(id, enabled)`만 호출한다. command, cwd, overlap policy, encrypted
  environment를 round-trip하지 않는다.
- browser mock도 backend와 같이 실제 enabled 변화 때 `lastEvaluatedAt` checkpoint를 갱신하고 나머지
  정의를 보존한다.
- active-run snapshot을 아직 정상 확인하지 못했거나 해당 job이 active이면 context와 inline delete를
  모두 disabled로 둔다. backend의 `active-run-must-stop` guard는 최종 방어선으로 유지한다.
- active run stop과 job delete는 context/inline/history 어느 entry point에서도 동일한 명시적 confirm을
  통과해야 한다.

### 3. 서비스 최초 snapshot과 상태별 메뉴

Files:

- `apps/run-manager/src/App.tsx`
- `apps/run-manager/src/App.test.tsx`

초기 load와 refresh가 모두 `listServices` 뒤 각 `getServiceInstance`를 읽은 하나의 service snapshot을
사용한다. 따라서 메뉴를 처음 여는 순간부터 실제 durable state를 기준으로 항목을 결정한다.

정확한 topology:

```text
시작
정지 (danger)
재시작
────────
편집
삭제 (danger)
```

상태 계약:

| 상태 | 시작 | 정지 | 재시작 | 삭제 |
|---|---:|---:|---:|---:|
| `stopped` | 가능 | 불가 | 불가 | 확인 후 가능 |
| `starting` | 불가 | 확인 후 가능 | 가능 | 불가 |
| `running` | 불가 | 확인 후 가능 | 가능 | 불가 |
| `retry_waiting` | 불가 | 확인 후 가능 | 가능 | 불가 |
| `stopping` | 불가 | 불가 | 불가 | 불가 |
| snapshot 없음 | 불가 | 불가 | 불가 | 불가 |

inline button도 같은 전이 표를 사용한다. 특히 retry-waiting을 stopped처럼 보고 “시작/삭제”를 노출하거나,
stopping 중에 delete를 허용하던 기존 오차를 제거했다. 인스턴스 snapshot이 없는 손상·부분 마이그레이션
상태도 stopped로 추정하지 않고 “상태 확인 불가”로 표시한다.

### 4. 재시도 대기 취소와 즉시 재시작 backend

Files:

- `apps/run-manager/src-tauri/src/storage.rs`
- `apps/run-manager/src-tauri/src/scheduler.rs`

`begin_service_stop`은 per-service scheduler mutex 안에서 `retry_waiting`도 `stopping`으로 CAS한다.
이 상태에는 live process가 없으므로 `stop_active_at`은 no-op이지만, 이어지는 generation-checked
`mark_service_stopped`가 명시적 취소를 commit한다. stopped transition은 `next_retry_at`도 clear해
오래된 backoff metadata를 남기지 않는다.

동시 due-retry supervisor도 같은 service mutex와 `retry_waiting` claim CAS를 사용한다. 따라서 사용자
stop/restart와 자동 retry 중 하나만 먼저 상태를 소유하며, 임의 PID 추측이나 별도 process kill은 없다.

`restart_service_at`은 retry-waiting을 위 경로로 stopped로 만든 다음 기존 `start_service_at`을 사용한다.
새 generation, owner instance ID, attempt token, service run row와 execution adapter handshake는 기존의
검증된 start transaction을 그대로 거친다.

### 5. 실행 이력 메뉴와 bounded log export

Files:

- `apps/run-manager/src/components/RunHistory.tsx`
- `apps/run-manager/src/components/RunHistory.test.tsx`

정확한 topology:

```text
로그 보기
재실행
로그 저장
```

- “로그 보기”는 메뉴가 열린 exact run을 선택한다. 보존 로그가 없으면 disabled다.
- “재실행”은 history filter의 현재 job이 아니라 context run의 `jobId`를 사용한다.
- “로그 저장”은 선택된 stdout/stderr stream을 저장하며 보존 로그가 없거나 action 중이면 disabled다.

`collectRunLog` contract:

1. cursor는 `string | null`로만 유지하고 parseInt/Number/BigInt round-trip을 하지 않는다.
2. backend 한 응답과 frontend 요청 chunk는 최대 256KiB다.
3. 한 사용자 action은 현재 stream의 production 보존 상한과 같은 50MiB에서 중단한다.
4. chunk를 배열로 모으고 종료 시 한 번만 최종 `Uint8Array`를 할당한다. 병렬 read나 background export는
   없다.
5. cursor가 전진하지 않으면 무한 loop 대신 truncated 결과로 종료한다.
6. 정확히 50MiB에 닿으면 1 byte probe로 남은 데이터 여부를 판정한다.
7. retention rotation이나 비정상 oversized response가 있으면 저장된 부분은 유지하되 사용자에게 부분
   저장 경고를 표시한다.

backend `tail_log`는 run row를 조회하고 app-local root, stored relative `log_dir`, exact run ID를 다시
검증한 뒤 stdout/stderr stream을 연다. frontend는 filesystem path를 전달하거나 받지 않는다.

파일명은 `run-<safe opaque id>-<stdout|stderr>.log`다. run ID는 `[A-Za-z0-9_-]` 이외 문자를 `_`로
바꾸고 64자로 제한한다. job name, command, cwd, target distro, environment, native log path는 filename,
error, menu state에 포함하지 않는다. Blob URL은 click 직후 `finally`에서 revoke한다.

### 6. Documentation and dependency boundary

Files:

- `apps/run-manager/README.md`
- `docs/architecture.md`
- `workthrough/2026-08-26-run-manager-context-menu.md`

README에는 세 메뉴 topology, confirmation, retry control, log export 상한을 기록했다. architecture에는
context-menu rollout, Run Manager data flow, lifecycle fail-closed 상태와 app-owned log boundary를
추가했다.

이 PR은 locked internal workspace package만 새로 소비한다. registry dependency, license surface,
CSP capability, filesystem scope, network scope에는 변화가 없다. importer edge로 `pnpm-lock.yaml`이
바뀌었으므로 canonical dependency generator로 `THIRD_PARTY_NOTICES.md`의 provenance hash만 갱신했다.

## Test Coverage

Frontend tests cover:

- job right-click target selection과 exact menu topology
- job Shift+F10 open, exact enable toggle, close 후 focus restore
- 두 번째 job의 “로그 열기”가 exact ID로 history filter를 여는 동작
- active run stop과 job delete cancel/confirm 양쪽 경로
- initial running service snapshot과 start/stop/restart/delete disabled state
- stopped service delete cancel/confirm
- retry-waiting 서비스의 stop/restart 허용과 start/delete 차단
- stopping 서비스의 모든 lifecycle/destructive action fail-closed
- service instance snapshot이 없는 경우의 lifecycle/destructive action fail-closed
- history right-click selection과 exact menu topology
- history Shift+F10 exact rerun과 focus restore
- selected stream export, opaque bounded filename, Blob URL revoke
- 2^53보다 큰 decimal cursor를 문자열 그대로 여러 chunk에 전달
- non-advancing cursor의 bounded termination
- 기존 bounded history, stdout/stderr tail, retention warning, manual run과 stop 회귀

Rust tests cover:

- retry-waiting service의 명시적 stop이 stopped로 전이하고 `next_retry_at`을 clear함
- 미래 supervisor tick이 취소한 서비스를 다시 시작하지 않음
- retry-waiting restart가 backoff를 건너뛰고 generation을 올려 새 process run을 시작함
- 기존 service stop/restart/retry와 전체 scheduler/storage 회귀는 최종 app test suite와 CI에서 확인

## Verification Results

### Focused frontend tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter run-manager test -- --maxWorkers=1 \
    src/App.test.tsx src/components/RunHistory.test.tsx
Test Files  2 passed (2)
Tests      16 passed (16)
exit 0
```

### Focused Rust tests

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p run-manager service_stop_cancels_retry_waiting_backoff \
    --lib -j1 -- --test-threads=1
1 passed; 0 failed; 155 filtered out
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p run-manager service_restart_bypasses_retry_waiting_backoff \
    --lib -j1 -- --test-threads=1
1 passed; 0 failed; 155 filtered out
exit 0
```

### Pre-review full frontend suite and final feature verification

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter run-manager test -- --maxWorkers=1
Test Files  6 passed (6)
Tests      32 passed (32)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter run-manager build
vite v7.3.6
48 modules transformed
dist/assets/index-BpckU2id.css   14.88 kB | gzip  4.22 kB
dist/assets/index-DXfqnI6-.js   264.75 kB | gzip 79.41 kB
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter run-manager test -- --maxWorkers=1 src/App.test.tsx
Test Files  1 passed (1)
Tests       9 passed (9)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter run-manager test -- --maxWorkers=1 \
    src/components/RunHistory.test.tsx
Test Files  1 passed (1)
Tests       8 passed (8)
exit 0

$ cargo fmt --all -- --check
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p run-manager --lib -j1 -- --test-threads=1
156 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p run-manager -j1
Finished dev profile
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p run-manager --all-targets -j1 -- -D warnings
Finished dev profile
exit 0

$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml
exit 0

$ bash .github/scripts/check-catalog.sh
exit 0

$ git diff --check
exit 0
```

전체 frontend suite 뒤 production TypeScript가 test spy의 암시적 `this` 타입을 1건 잡았다. test-only
callback을 `HTMLAnchorElement`로 명시한 뒤 production build와 해당 RunHistory 8개 테스트를 다시
통과시켰다. 최종 review에서 service instance snapshot 부재를 fail-closed로 바꾼 뒤 App 9개 테스트와
production build도 다시 통과시켰다.

## Files Changed

- `apps/run-manager/package.json`
- `apps/run-manager/README.md`
- `apps/run-manager/src/App.css`
- `apps/run-manager/src/App.tsx`
- `apps/run-manager/src/App.test.tsx`
- `apps/run-manager/src/api.ts`
- `apps/run-manager/src/components/RunHistory.tsx`
- `apps/run-manager/src/components/RunHistory.test.tsx`
- `apps/run-manager/src-tauri/src/storage.rs`
- `THIRD_PARTY_NOTICES.md`
- `apps/run-manager/src-tauri/src/scheduler.rs`
- `docs/architecture.md`
- `pnpm-lock.yaml`
- `workthrough/2026-08-26-run-manager-context-menu.md`

## Follow-ups

- P1 Run Manager status snapshot은 별도 기능 PR에서 producer schema와 privacy boundary를 구현한다.
- P2 log search와 `log-source/v1` contract는 이 PR의 bounded reader를 무단 확장하지 않고 별도 설계를
  따른다.
- 선택 P3 Log Lens producer handoff, history filter 강화, task import는 Log Lens receiver와 applink v2
  선행 계약 뒤 각각 기능 단위 PR로 진행한다.
- Windows packaged runtime에서 context menu pointer/keyboard positioning, native log download, Job Object
  stop과 retry-waiting restart를 acceptance 항목으로 다시 확인한다.
