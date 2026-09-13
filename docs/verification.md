# 로컬 검증 운영

`pnpm verify:affected`는 변경과 역의존 소비자를 검사한다. release 준비·검증기 변경·명시적
전체 감사는 `pnpm verify:all`을 사용한다. affected가 all을 선택하면 같은 전체 검사이므로
한 번만 실행한다. 범위·검사 항목·CI 완료 조건을 줄여 자원을 절약하지 않는다.

## 실행 시점과 중복 방지

상세 검증의 단위는 **계획한 작업을 모두 구현한 PR**이다. 커밋은 변경을 나누는 단위이며,
커밋할 때마다 전체 수용 검증을 다시 실행하는 단위가 아니다. v0.8에서는 B01~B09의 각
묶음에 포함된 UI·native engine·importer·fixture까지 구현한 시점을 기준으로 한다.

| 시점 | 수행할 확인 |
|---|---|
| 구현·커밋 전 | diff와 수용 계획 대조, 문법·타입 오류를 찾는 최소 검사. 문서는 내용·링크·diff만 확인 |
| 구현 중 구체적 결함 조사 | 재현 없이는 해결할 수 없는 문제의 최소 재현 또는 해당 검사만 실행 |
| PR 전체 구현 완료 | 수용 기준과 검사 항목을 대응시키고 `pnpm verify:affected` 및 미포함 회귀·migration·Windows/WSL 실기 수행 |
| 검증 실패 수정 후 | 실패한 검사와 수정으로 영향받은 범위를 다시 확인. 관련 없는 통과 검사는 유지 |
| 최종 머지·릴리스 | PR 최종 변경의 CI 통과 확인. 릴리스의 exact-main 후보 수용 조건은 release policy 적용 |

- 커밋 전 검사에 test·Clippy·build·affected를 관행적으로 모두 붙이지 않는다. 변경을 이해하는
  데 필요한 최소 진단을 고르고 구현을 계속한다. 재현·검증 fixture 작성은 구현 중에도 진행한다.
- PR 완료 검증 전에 `verify:affected`가 이미 실행하는 항목을 확인한다. 포함된 테스트·타입·
  빌드·lint를 별도 집중 검사로 먼저 실행한 뒤 같은 범위의 verify를 다시 실행하지 않는다.
- 검증 기록에는 대상 변경, 결과, 아직 남은 수용 항목을 구분한다. 재실행은 실패·관련 변경·
  새 위험 등 구체적인 근거가 있을 때만 한다. 커밋 생성·문서 갱신·작업 재개는 재실행 사유가 아니다.
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

Windows Rust CI의 Cargo build job은 1개다. 여러 Tauri build script가 같은
target staging의 고지 파일을 동시에 복사하면 Windows sharing violation 32가
발생하므로 직렬화한다. 전체/scoped check·Clippy·test 범위와 test harness 동시성은
유지하며, Linux CI와 로컬 검증의 별도 예산은 바꾸지 않는다.

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

## 경량 메타데이터 경계

`.agents/skills/devbox-release/agents/openai.yaml`만 compiler/test 범위에서 제외한다.
로컬과 CI scope job에서 별도 checker가 `policy: allow_implicit_invocation: false`의
두 줄 구조를 검증한다. 주석·빈 줄만 추가 허용하며 새 필드·다른 정책 값은 거부한다.
새 schema가 필요하면 checker와 회귀 테스트를 함께 변경한다. 다른 agent YAML·실행 스크립트·
미분류 경로는 기존 fail-safe 전체 검증을 유지한다.
