# 로컬 검증 운영

`pnpm verify:affected`는 변경과 역의존 소비자를 검사한다. release 준비·검증기 변경·명시적
전체 감사는 `pnpm verify:all`을 사용한다. affected가 all을 선택하면 같은 전체 검사이므로
한 번만 실행한다. 위험과 변경 영향에 필요한 검사만 선택한다. 횟수를 늘리는 대신 실제 동작과 결과를 확인한다.

## 실행 시점과 중복 방지

상세 검증의 단위는 **계획한 작업을 모두 구현한 PR**이다. 커밋은 변경을 나누는 단위이며,
커밋할 때마다 전체 수용 검증을 다시 실행하는 단위가 아니다. 현재 재정비의 R00–R16은
모두 통합 브랜치에 모으며 최종 PR 1개에서 전체 검증한다. 앱/작업별 CI는 실행하지 않는다.

| 시점 | 수행할 확인 |
|---|---|
| 과제 구현 중 | 실패하는 테스트 작성 → 해당 테스트와 직접 영향받는 테스트만 실행 → 통과 후 과제 커밋 |
| PR 전체 구현 완료 | `pnpm verify:affected` 및 미포함 회귀·Windows/WSL 실기 수행 |
| 완료 검증 실패 후 | 확인된 수정들을 모두 마친 뒤 실패·영향 범위만 모아 재실행 |
| 최종 머지·릴리스 | PR 최종 변경의 CI 통과 확인. 릴리스의 exact-main 후보 수용 조건은 release policy 적용 |

- 과제 단위 테스트는 좁게 선택한다. `--workspace`나 전체 패키지 테스트를 과제마다 돌리지 않는다.
  clippy·전체 build·affected·실기 검사는 PR 끝에 한 번 모은다.
- 로컬·수동 CI뿐 아니라 push 자동 실행에도 적용한다. 중간 커밋은 로컬에 모으고
  PR 개발 완료 시 push한다. 이미 수행한 검사와 겹치는 추가 실행을 예약하지 않는다.
  최종 CI와 필수 수용 조건은 유지한다. 필요한 선행 변경을 통합 브랜치에 반영하면
  후속 개발을 진행한다. 작업마다 main 머지나 CI 대기를 만들지 않는다.
- 과제별 TDD의 좁은 테스트와 PR 끝의 통합 검증은 목적이 다르다. PR 완료 시
  `verify:affected`에 포함된 테스트·타입·빌드·lint를 별도 전체 검사로 중복 실행하지 않는다.
- 검증 기록에는 대상 변경, 결과, 아직 남은 수용 항목을 구분한다. 재실행은 실패·관련 변경·
  새 위험 등 구체적인 근거가 있을 때만 한다. 커밋 생성·문서 갱신·작업 재개는 재실행 사유가 아니다.
- PR 완료 테스트는 선택한 범위의 실패를 한 번에 모은다. Cargo `--no-fail-fast`와
  pnpm `--no-bail`로 첫 실패 뒤에도 남은 테스트를 실행하되 최종 실패 종료 코드는 유지한다.
- Rust 실패 재검사는 기존 Cargo feature 통합과 빌드 조건을 유지한다. 패키지 제외로
  실행 범위를 줄이면서 의존성 조합까지 바꿔 전체 재컴파일을 유발하지 않는다. 먼저 만든
  같은 코드·feature의 검사 산출물에서 미완료 target만 실행할 때는 Cargo의 패키지 작업
  디렉터리·런타임 환경과 공통 자원 제한을 보존하고, 완료/실패/미실행 target을 기록한다.
- 최종 코드가 검사 후 바뀌면 영향을 받은 검증을 보충한다. resolver가 요구한 all 범위와
  최종 CI·Windows 수용 조건은 지킨다. 불필요한 반복을 줄이기 위해 gate 자체를 생략하지 않는다.

Worktree마다 dependency/feature 구성이 다르면 공유 Cargo target이 있어도 이전 검사
실행 파일을 다시 컴파일할 수 있다. 단일 회귀 검사에서 관행적으로 `--workspace`를 붙이지
않는다. 필요한 package/test target을 선택하고, 변경되지 않은 실행 파일만 재사용한다.
컴파일 범위가 커졌다는 이유로 이미 통과한 테스트 전체를 다시 실행하지 않는다.

### CI 성공 근거 재사용

최종 PR/main의 CI gate는 현재 입력과 검증 범위에 대한 성공 근거를 요구한다.
동일 입력으로 이미 성공한 컴파일·테스트를 다시 실행하는 횟수를 요구하지 않는다.
CI가 같은 저장소의 신뢰 가능한 이전 성공에서 실제 실행한 compiler/test 단계와 입력·범위를
확인한 경우에만 최근 7일의 해당 근거를 재사용한다. 패키지별로 입력이 같은 성공 근거를 합쳐 현재 범위를
충족할 수 있지만, 재사용·skip 결과를 새 원본 성공으로 다시 연결하지 않는다. 소스뿐 아니라
의존성·lockfile·선언된 compiler 명령·setup·toolchain 버전·runner label·환경도 비교하며,
다른 범위의 PASS나 빌드 캐시 적중만으로 성공을 인정하지 않는다.

현재 CI 결과에는 새 실행과 재사용을 구분하고, 재사용한 원본 run·job·source와 입력·범위를
추적할 수 있는 provenance를 남긴다. 정상 조회 결과에서 입력이 달라졌거나 성공 근거가
없으면 필요한 범위를 실행한다. 실패·미완료·신뢰 조건을 충족하지 않는 결과는 재사용하지 않는다.
조회·전송·로그 해석 등 근거 수집 자체의 예상하지 못한 오류는 scope gate에서 진단과 함께
중단한다. 오류를 근거 없음으로 숨겨 전체 컴파일·테스트를 자동 실행하지 않는다.
주간 전체 감사는 실제로 전 범위를 실행하며, 현재 의존성 정책·advisory 검사는 새로 수행한다.
Frontend의 format·component size·fixture 계약 검사도 compiler 결과 재사용과 별개로 실행한다.
이 재사용은 compiler/test CI 근거에 한정한다. 최종 exact-main 후보의 네 native scope,
40개 설치 사용자 여정, WSL2/Docker, migration/recovery 및 sealing 조건은 그대로 적용한다.

## 기존 서비스와 공유 네트워크 보호

로컬 테스트 때문에 기존 Docker 서비스, 방화벽 또는 네트워크 상태가 바뀌어서는 안 된다.
2026-09-13 사용자는 전용 WSL2 테스트 배포판에서 Docker를 준비한 뒤 기존 서비스 영향과
iptables 일부 손실을 보고했다. 배포판/파일/data-root 소유권과 네트워크 격리는 별개다.
Docker는 bridge를 위해 **호스트 network namespace에 iptables 규칙을 만든다**
([Docker 공식 설명](https://docs.docker.com/engine/network/firewall-iptables/)).
따라서 고유 배포판·socket·container 이름을 만들고 나중에 제거했다는 사실로 안전을 판단하지 않는다.

- 로컬 Windows/WSL 및 기존 서비스가 있는 self-hosted runner에서는 Docker 패키지 설치,
  daemon 시작/중지, 실제 container/network/volume 조작, firewall/route/sysctl 변경을 하지 않는다.
  패키지 설치 중의 service hook도 공유 네트워크를 바꿀 수 있으므로 테스트 본문 직전이 아니라
  **provisioning 전에** 차단한다. 임시 WSL 배포판을 만드는 이 수용 runner도 로컬에서 차단한다.
- `.github/scripts/windows-workspace-owned-wsl2.ps1`은 파일 복사·배포판 등록 전에 GitHub-hosted
  Windows runner와 현재 저장소/run/source를 확인한다. Node 진입점과 container 함수도 별도로
  차단한다. 로컬 허용 switch는 없고 CI 환경 변수를 꾸며내거나 과거 사설 복사본으로 우회하지 않는다.
- Docker/WSL2 실기는 일회성 hosted runner로 옮긴다. 독립 VM을 사용할 경우에도 그 VM의
  kernel/network와 대상 Docker endpoint가 기존 서비스와 분리된 별도 실행 경로가 필요하다.
  현재 runner의 CI 차단 조건을 바꾸는 방식으로 VM을 승인하지 않는다.
- 격리 환경을 확보하지 못한 수용 항목은 미실행으로 기록한다. 이미 통과한 순수 단위/타입
  검사를 반복하지 않고 구현을 계속한다. 기존 서비스 재시작이나 iptables 복원은 증거 없이
  자동으로 시도하지 않으며 별도의 명시적 복구 요청 범위에서 처리한다.

## 기본 자원 예산

WSL/Linux의 두 verify 명령은 같은 supervisor를 사용한다. 직접 실행한 `cargo`/`pnpm test`,
에디터·Codex·다른 앱, Windows 패키징에는 이 제한을 자동 적용하지 않는다.

| 환경 변수 | 기본값 | 적용 대상 |
|---|---:|---|
| `DEVBOX_VERIFY_PACKAGES` | 1 | pnpm recursive package 동시 실행 |
| `DEVBOX_VERIFY_WORKERS` | 2 | 패키지별 Vitest worker |
| `DEVBOX_VERIFY_BUILD_JOBS` | 2 | Cargo build job |
| `DEVBOX_VERIFY_TEST_THREADS` | 2 | Rust test harness thread |
| `DEVBOX_VERIFY_CPUS` | 4 | 상속 CPU affinity; systemd CPUQuota 400% |
| `DEVBOX_VERIFY_MEMORY_HIGH_MB` | 6144 | cgroup reclaim/throttle 기준 6GiB |
| `DEVBOX_VERIFY_MEMORY_MAX_MB` | 8192 | cgroup memory 상한 8GiB |
| `DEVBOX_VERIFY_SWAP_MAX_MB` | 1024 | cgroup swap 상한 1GiB |

모든 숫자는 1~65536의 정수이고 memory high는 max 이하여야 한다. nice를 10 증가시켜
다른 작업에 CPU 우선권을 준다. CPU affinity는 현재 허용된 CPU 중 최대 지정 개수를 상속한다.
패키지 수와 worker 수를 함께 늘리면 총 프로세스 수가 곱으로 늘어날 수 있다.

`DEVBOX_VERIFY_CGROUP=auto`가 기본이다. systemd user scope와 resource controller 사용을
작은 프로세스로 확인한 뒤 검증과 자식 프로세스에 메모리·swap·CPU quota를 적용한다.
지원하지 않는 호스트에서는 미적용 메시지를 출력하고 worker·affinity·nice 제한을 유지한다.
`required`는 메모리 제한을 적용할 수 없으면 검증 시작 전 실패하며, `off`는 scope를 끈다.
강한 메모리 제한이 필요한 작업에는 `required`를 사용한다.

```bash
source ~/.cargo/env
DEVBOX_VERIFY_CGROUP=required pnpm verify:affected
# 다른 작업의 부하가 높은 경우
DEVBOX_VERIFY_CPUS=2 DEVBOX_VERIFY_WORKERS=1 DEVBOX_VERIFY_BUILD_JOBS=1 pnpm verify:affected
```

memory max 초과 시 OOM으로 검증이 실패할 수 있다. 실패를 성공으로 바꾸거나 무제한으로
자동 재실행하지 않는다. 측정과 현재 호스트 여유를 확인해 해당 실행의 예산을 조정한다.
`CI=true`이면 `ci` profile로 기존 CI 실행을 유지한다. `DEVBOX_VERIFY_PROFILE=local`은
명시적으로 로컬 제한을 선택한다. 로컬에서 `ci` profile로 제한을 우회하지 않는다.

Windows compiler/native acceptance CI는 실패한 실행에서도 Rust 의존성 빌드 캐시를
보존한다(`cache-on-failure: true`). 첫 테스트 실패 때문에 완료된 의존성을 다음
실행에서 다시 컴파일하지 않도록 하기 위한 설정이다. compiler·Cargo manifest/lockfile·
환경 해시 키와 기본 workspace crate 제외 정책은 유지하며, 캐시를 테스트 PASS 근거나
이전 제품 실행 파일의 재사용 허가로 취급하지 않는다.
Windows CI는 `shared-key: devbox-windows-unit-tests-v1`로 test codegen 의존성을 보관한다.
`shared-key`가 있으면 별도 `key` 입력은 무시되므로 namespace를 shared-key 자체에 둔다. 이전 check/Clippy 전용
불변 캐시가 적중한 채 테스트 의존성을 매번 다시 빌드하는 상황을 반복하지 않는다.

Windows Rust CI의 Cargo build job은 2개다. 네 제품 build.rs는
`crates/product-shell-tauri/build_support.rs`의 OS file lock을 보유한 동안만
Tauri 공유 staging을 복사한다. Windows sharing violation 32를 일으키던 복사 구간을
직렬화하고 독립 crate 컴파일은 병렬로 유지한다. Linux/로컬 예산은 그대로다.

## 실행 잠금과 측정

Git common directory의 `devbox-verification/lock`으로 linked worktree까지 한 번에 하나의
verify 실행만 허용한다. 충돌은 대기하거나 기존 작업을 종료하지 않고 exit 75로 끝난다.
기존 실행 종료 후 다시 실행한다. 잠금 파일이 남아 있어도 활성 OS lock이 없으면 실행 가능하다.
일반적인 SIGINT/SIGTERM 취소는 process group으로 전달하고, 5초 후 남은 자식을 종료한다.
supervisor 자체를 SIGKILL로 강제 종료하는 경우에는 이 정리 절차를 실행할 수 없다.

최신 실행의 결과는 `$(git rev-parse --git-common-dir)/devbox-verification/latest.json`이다.
경과 시간·exit code·적용 예산·0.25초 간격의 process group RSS/swap 최대 합계를 기록한다.
RSS 합계는 공유 페이지 중복과 샘플 사이의 peak 누락이 가능하므로 실제 물리 메모리 peak와
동일하지 않다. systemd 적용 시 cgroup memory peak·CPU 사용 시간·마지막 swap 값도 기록한다.
실패/OOM으로 측정을 마치지 못하면 `measurement_incomplete`로 표시하며 이전 성공 기록을
재사용하지 않는다. 파일은 `.git` 아래의 호스트 진단 자료이며 커밋하지 않는다.

## 실제 Tab 순서 회귀 검사

`packages/a11y` 테스트는 jsdom 검사 뒤 독립 임시 프로필의 headless Chromium에서 CSS 숨김,
양수/음수 tabindex, radio 그룹, 실제 Tab/Shift+Tab 이동과 dialog 순환을 확인한다.
Linux의 Google Chrome/Chromium 또는 `DEVBOX_TEST_CHROME`으로 지정한 실행 파일을 사용한다.
브라우저가 없으면 미실행을 성공으로 숨기지 않고 실패한다. 사용자 프로필과 기존 창은 건드리지 않는다.

공유 외부 Rust 의존성의 버전과 feature 합집합은 루트 `[workspace.dependencies]`에서 관리한다.
member는 `workspace = true`로 가져오며, 원래 기본 feature가 필요한 member는 이를 명시한다.
TOML 0.8과 1.1 파서는 기존 해석 동작을 유지하도록 별도 이름으로 선언한다.
개발·테스트 빌드는 `line-tables-only`로 파일·행 backtrace를 유지하면서 변수 debug 정보를 줄인다.

디스크 사용량은 작업 시작, 각 과제의 검증 완료, 묶음 완료 시 확인한다. 과제 사이에
재생성 가능한 오래된 incremental 세대와 중복 테스트 실행 파일을 정리하고, 묶음을
머지한 뒤에는 CONVENTIONS §8의 순서로 완료한 전용 worktree와 브랜치를 정리한다.
PR·ledger에는 정리 전후 사용량과 보존한 활성 작업을 기록한다.

공유 Cargo 산출물 정리는 검증과 같은 실행 잠금 안에서 수행한다. 별도로 실행 중인
cargo·rustc·clippy 프로세스도 확인하고, 다른 worktree가 검증 중이면 정리를 미룬다.
현재 작업의 최근 산출물, 라이브러리와 빌드 캐시는 보존한다. 사용자 데이터·다른 프로젝트의
캐시·dirty 또는 미머지 worktree는 디스크 확보 대상으로 삼지 않는다. 전체 삭제
(`cargo clean`)는 다음 빌드가 오래 걸리므로 마지막 수단이다. `cargo-sweep`을 이미
사용하는 환경에서는 `cargo sweep --time 30`으로 오래된 산출물을 정리할 수도 있지만,
월간 정리만으로 단계별 점검을 대체하지 않는다.

## 현재 통합 작업의 자원과 정리

[현 실행 계획 §4–5](superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)의 최소 검사와 자원 정책을 따른다.
시작/대형 검사 전후 WSL·Windows 가용 메모리와 swap/commit을 확인한다. 가용 메모리 20% 미만이나
paging과 지연 증가가 함께 관찰되면 새 대형 작업을 보류하고 worker/job을 1로 낮춘다.
Windows와 WSL 대형 빌드를 겹치지 않는다. 하위 에이전트의 검사도 공통 lock을 사용한다.
완료한 소유 fixture·임시 설치본·중복 archive는 필요한 최초 실패/최신 증거를 보존한 뒤 정리한다.
공유 cache를 매번 삭제하지 않는다. 정리 전에 소유권·실행 프로세스·통합 여부를 확인한다.
40개 수용 ID는 추적 단위로서 공통 사용자 여정에서 함께 판정하며 별도 40회 실행을 뜻하지 않는다.
