# 로컬 검증 운영

`pnpm verify:affected`는 변경과 역의존 소비자를 검사한다. release 준비·검증기 변경·명시적
전체 감사는 `pnpm verify:all`을 사용한다. affected가 all을 선택하면 같은 전체 검사이므로
한 번만 실행한다. 범위·검사 항목·CI 완료 조건을 줄여 자원을 절약하지 않는다.

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
