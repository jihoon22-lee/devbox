# devbox — 공통 개발 규약

이 문서는 공통 규약의 원장이다. v1은 Windows 11의 단일 Tauri v2 앱과 WSL Rust 데몬이다.
설계는 [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md),
실행 계획은 [10-s7-s8-release](docs/superpowers/plans/2026-10-08-devbox-v1/10-s7-s8-release.md)(끝난 계획은 [docs/history.md](docs/history.md)),
진행 상태의 유일한 원장은 같은 폴더의 PROGRESS.md다. 계획 예제와 충돌하면 사용자 지시와 이 규약을 우선한다.

## 1. 환경

pnpm 9, Node 24, Rust 1.98.1 edition 2024를 사용한다. WSL에서 Rust 사용 전 `source ~/.cargo/env`.
편집·프론트 개발·플랫폼 독립 테스트는 WSL에서, 실제 Windows 앱 실행·배포 빌드는 Windows에서 한다.
WSL 성공을 Windows 실기 PASS로 보고하지 않는다. 개인 인증·전역 설정은 변경하거나 저장소에 복사하지 않는다.

## 3. 스택과 코드

Tauri v2·React 19·TypeScript·Vite·순수 CSS와 v1 설계를 따른다. 도메인 crate는 Tauri에 의존하지 않는다.
경계 타입에 `Result<T, String>`·`serde_json::Value`를 쓰지 않는다(RPC RawValue 중계 예외).
RPC는 `method!`·`topic!` 선언에서 TS를 생성한다. 생성물을 손으로 수정하지 않는다.
화면 문구는 한국어이며 docs/ui-terms.md를 따른다. 파일 600줄·컴포넌트 300줄을 넘으면 나눈다.
tracing 로그·TanStack 등 v1 설계가 승인한 스택은 해당 단계에서 구현한다. 불필요한 추측성 공용 모듈은 만들지 않는다.

## 5. 검증

필요한 개발·회귀·CI 검증은 에이전트가 직접 수행한다. 미구현 기능은 해당 구현 단계에서 검증하며 사용자 실기 항목으로 넘기지 않는다. 중간 설치·사용자 실기 요청을 개발·머지 gate로 두지 않는다. 실사용은 기능 완성 뒤가 기본이며 조기 사용은 사용자가 고른다(안내만 하고 기다리지 않는다). 사용자가 알려 준 문제는 PROGRESS §4에 적고 진행 중인 브랜치에서 고친다. 공개 릴리스의 명시 승인은 별도다.

과제마다 로직의 실패 테스트부터 작성하고 해당 테스트와 직접 영향받는 테스트만 실행한다.
전체 검증·clippy·build·E2E·Windows/WSL 실기는 PR 모든 과제 완료 뒤 한 번 모아 실행한다. Windows 설치본 스모크는 01 §11과 아래 "Windows 수용"대로 에이전트가 수행한다.
실패를 고친 뒤 실패·영향 범위만 재실행한다. 통과한 무관한 검사는 반복하지 않는다.
완료 커밋·push 전 `pnpm exec biome ci .`를 실행한다. flaky 테스트는 해당 PR에서 수정하거나 제거한다.
`.github`+`scripts`는 합계 2,000줄 이하, 워크플로는 3개 이하를 유지한다(2026-10-26 사용자 추가 승인).

로컬 검증은 Cargo jobs 2·Vitest workers 2·Rust test threads 2, CPU 4개·nice +10으로 제한한다.
사용 가능한 cgroup에서 memory high 6GiB/max 8GiB·swap max 1GiB를 적용한다.
cgroup이 걸리지 않으면 available 메모리가 2 GiB 아래일 때 무거운 빌드를 멈추고, 실행 결과에 미적용을 적는다(사용자 결정 2026-10-22).
무거운 검증은 `python3 scripts/verify-resources.py [--wait] [--lock <이름>] -- <명령>`으로 실행한다.
Git common-dir의 공유 잠금을 비차단으로 획득하고 경합 시 exit 75로 종료한다. 무거운 검증은 worktree 전체에서 하나만 실행한다. 여러 작업을 동시에 진행할 때의 전체 검사·E2E는 `--wait`(최대 60분)를 쓴다. 기한 초과 exit 75는 검사 대기 초과로 보고하고 재시도한다. 작업별 좁은 검사는 잠금 없이 위 자원 제한을 지킨다.

전역 Rust target-dir(`~/.cache/targets/devbox`)은 worktree마다 incremental 산출물이 따로 쌓여 커진다. 작업 PR이 머지된 뒤와 `du -sh ~/.cache/targets/devbox`가 150 GiB를 넘을 때 정리하되, 실행 중인 cargo·rustc가 없을 때(`pgrep -a -x cargo; pgrep -a -x rustc`가 비어 있을 때)만 한다.
다른 Rust 프로젝트도 쓰는 target이므로 산출물만 지운다(사용자 결정 2026-10-27 결정 6: §8 공유 cache 보존의 예외, 다시 만들 수 있는 target 산출물에 한함). 먼저 `cargo sweep --time 3`(최근 3일 쓰지 않은 산출물)을 하고, 그래도 150 GiB 아래가 아니면 `debug/incremental/`만 비운다. 의존성 산출물은 남겨 새 worktree의 첫 빌드를 줄인다.
`cargo sweep`이 없으면 `cargo install cargo-sweep --locked`로 설치한다(사용자 홈의 도구라 묻지 않는다). `scripts/sweep.sh`는 정리를 유보하는 no-op이다.
동시 진행용으로 따로 둔 target-dir(`devbox-s5` 등)은 그 작업이 머지된 뒤 지운다.
WSL에서 지워도 Windows 드라이브 공간은 VHDX 압축 전에는 돌아오지 않는다. 압축(`wsl --shutdown` 필요) 시점은 사용자가 정한다.

기존 서비스·tmux·systemd unit·Docker·방화벽·공유 네트워크를 테스트로 변경하지 않는다.
Docker 설치/시작/중지/container/network 조작이나 iptables/nftables·라우팅 변경 검사는 일회성 hosted CI 또는 독립 VM에서만 한다.
별도 WSL/socket/data-root는 네트워크 격리 증거가 아니다. 차단을 옵션·환경 변수로 우회하지 않는다.
테스트는 소유 fixture·고유 인스턴스를 쓰고 소유 자원만 정리한다. 사용자 데이터·secret은 fixture로 쓰지 않는다.
미실행 실기·수동 유보는 PASS와 구분한다.

Windows 수용(사용자 결정 2026-10-13·2026-10-14):
- PR 전체 변경을 `scripts/ci-scope.sh`가 `code=true`로 판정하는 PR마다 PR 전에 에이전트가 WSL에서 PowerShell을 절대 경로(`/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe`)로 불러 CI 설치본을 설치·실행·제거한다. 최종 소스 SHA의 `windows.yml` 산출물만 쓴다: `bash scripts/dogfood.sh <브랜치> --channel accept`가 저장소·workflow·run·source SHA·SHA256을 확인한다(절차와 안전 경계는 01 §11). 문서만 바꿔 `code=false`인 PR은 설치본이 생기지 않으므로 수용을 건너뛴다(#641·#643·#647·#650 선례). 그 PR 본문에는 "Windows 수용: NOT_RUN(문서 전용)"으로 적고 PASS로 쓰지 않는다.
- 설치본은 prod와 겹치지 않는 `accept` 채널(별도 identifier·설치 폴더·앱 데이터·프로토콜·자동 시작 이름, WSL `devbox@accept` 인스턴스와 채널 전용 바이너리·unit 템플릿)만 쓴다. prod 설치본과 `devbox@prod`는 사용자의 실사용 자원이라 수용 검증이 바꾸지 않으며(불변 확인용 읽기 전용 해시·상태 비교만 한다), 있다고 멈추지 않는다. `accept` 채널 자원이 소유 표식 없이 이미 있을 때만 멈춰 묻는다.
- 수용은 한 번에 하나만 한다(Windows 데스크톱·UI 자동화·`accept` 채널이 하나라서). 설치→검증→제거·소유 자원 정리 전체 동안 `windows-acceptance` 잠금(`scripts/verify-resources.py --wait --lock windows-acceptance`)을 잡는다.
- 이번 수용이 만든 설치 경로·데이터·WSL unit·소켓만 제거한다. v0.9.0(`com.devbox.*`, DevboxSuite)과 기존 devbox 데이터는 보존한다. UAC·재부팅·사용자 클릭이 필요하면 멈추고 보고한다.
- 그 작업이 더한 Windows 기능을 확인한다. 앱·데몬 로그, user unit 상태, 진단, 필요하면 스크린샷을 근거로 PASS·FAIL·NOT_RUN을 PROGRESS와 PR 본문에 구분해 적는다. UI 자동화 수단이 없으면 해당 항목만 NOT_RUN으로 남긴다.

## 8. Git

시작 시 status/worktree·원격 작업 인증 상태를 확인한다. 원본 main의 사용자 커밋은 보존한다.
작업 하나 = 브랜치 하나 = PR 하나다. 브랜치는 `feat/<이름>`·`fix/<이름>`·`docs/<이름>`·`release/<이름>` 가운데 하나이고 전용 worktree 하나(`../devbox-wt/<브랜치 이름의 /를 -로>`, 원본 체크아웃 `/home/jihoon/projects/devbox` 기준)를 쓰며 `origin/main`에서 만든다. 예: `fix/<이름>` → `/home/jihoon/projects/devbox-wt/fix-<이름>`. 1.0.0 전(S0–S7)은 하위 프로젝트마다 `v1/<s>-<name>`을 썼다. 1.0.0 공개(S8)까지는 진행 중인 `v1/s8-release`와 [계획 10](docs/superpowers/plans/2026-10-08-devbox-v1/10-s7-s8-release.md)으로 끝내고, 그 뒤 작업부터 이 모델을 따른다.
그 작업의 모든 과제와 하위 에이전트는 이 브랜치·worktree에서 차례로 일한다. 원본 체크아웃은 `main`으로 두고 직접 작업하지 않는다(다른 세션이 동시에 일할 수 있다). 오타·계획 문서 같은 사소한 수정은 진행 중인 브랜치에 함께 넣고 따로 PR을 만들지 않는다.
영어 Conventional Commits로 과제마다 커밋하며 작성자는 noreply 주소를 쓴다: `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit`(GitHub 이메일 비공개 설정 때문이며 저장소 설정은 바꾸지 않는다).
작업 중간에는 push하지 않고 상세 최종 검증 뒤 push·PR을 한 번 한다. CI·리뷰 수정 뒤 필요한 push와 Windows 산출물을 만들기 위한 백업 push·workflow 실행만 예외다. 백업으로 push한 브랜치도 그 작업의 마무리 과제 전에는 PR을 열지 않는다(리뷰 회차는 PR을 연 뒤의 push마다 센다). 하네스가 push 뒤 PR 생성을 기본으로 안내해도 이 규칙이 우선한다.
main은 squash만 사용한다. PR 최종 변경의 `Frontend (pnpm)`, `Rust (Cargo workspace)`, `Rust (Windows)` CI 성공이 필수다.
작업의 마지막 과제가 끝나면 브랜치 전체 검토·최종 로컬 검사·Biome CI 뒤 최종 소스 SHA를 push한다. `windows.yml`을 그 SHA의 브랜치에서 수동 실행해 검증된 설치본으로 스모크(§5 Windows 수용)를 수행한 뒤 PR을 한 번 연다. 문서 전용 PR(`code=false`)은 수동 실행·스모크 없이 연다(§5 Windows 수용).
브랜치 전체 검토는 PR 전에 별도로 수행하고 결과를 PROGRESS에 남긴다. 과제별 검토로 대신하지 않으며, 기록이 없으면 수행하지 않은 것으로 본다.
최종 SHA 원칙: 실패 수정으로 SHA가 바뀌면 새 SHA 산출물로 필요한 스모크를 다시 수행한다. 문서만 바뀌어도 이 원칙을 유지한다. 최종 스모크 결과는 PR 본문과 PROGRESS 작업 사본에 기록하고, 결과 기록만 담은 PROGRESS 변경은 다음 작업 브랜치의 문서 커밋으로 인계하여 검증한 소스 SHA를 바꾸지 않는다. 검증 뒤 현재 브랜치에 추가 커밋을 만들면 그 SHA의 설치본을 다시 확인한다.

lock·생성 파일 병합(00 §3.3·§3.6에서 옮김):
- clone마다 한 번 `git config merge.lockfile.driver true && git config rerere.enabled true`를 한다. `.gitattributes`가 `Cargo.lock`(루트와 `app/src-tauri`)·`pnpm-lock.yaml`·`app/src/rpc/gen/rpc.ts`를 `merge=lockfile`로 둔다. `rerere`는 반복 충돌 해결에 활용한다.
- 이 파일들은 손으로 합치지 않는다. 양쪽이 모두 바꾼 파일은 병합 드라이버가 현재 쪽 판을 남긴다: rebase에서는 main 쪽 판이고, 브랜치에서 `git merge origin/main`을 하면 브랜치 판이다. merge라면 main 판을 받는다(`git checkout origin/main -- <파일>`).
- 그 뒤 다시 만든다: `rpc.ts`는 `pnpm gen`, Cargo lock은 `cargo metadata --format-version 1 >/dev/null`(빠진 의존성만 더함, 앱 lock은 `--manifest-path app/src-tauri/Cargo.toml`), `pnpm-lock.yaml`은 `pnpm install`. 재생성은 lock만 바꾼 갱신(전이 의존성의 보안 갱신 등)을 되살리지 않으므로 브랜치의 그런 갱신은 다시 적용한다. CI가 `--locked`·`--frozen-lockfile`로 확인한다.

PR 리뷰(사용자 결정 2026-10-27):
- 판정: PR을 연 뒤 10분 안에 `chatgpt-codex-connector[bot]`이 `codex-pull-request-review-summary` 댓글을 남기고, 그것이 사용량 한도 안내가 아니면 "자동 리뷰 켜짐"이다. 아니면 "꺼짐"이다. (#638–#645의 첫 반응은 PR 생성 0.1–0.3분 뒤, #646 이후는 반응 없음.)
- 켜짐: 리뷰가 끝난 서로 다른 head SHA 최대 3개까지 반영한다.
  - 지적은 코드·회귀 근거로 타당성을 판단해 고친다. 다음 push 전에 그 회차 수정을 모은다.
  - 4회째부터는 차단 기준만 고치고 나머지는 PROGRESS §4 열린 후속에 적는다.
  - 회차 리뷰가 push 뒤 60분 안에 끝나지 않거나 한도 안내가 오면 그때부터 꺼짐으로 본다.
- 꺼짐: 하네스의 독립 코드·보안 검토를 PR마다 한 번만 한다.
  - 타당한 지적은 고치고, 과제별 별도 검토·직접 영향 검사·필수 CI로 확인한다. PR 검토를 다시 하지 않는다.
  - 자동 리뷰는 NOT_RUN(꺼짐)으로 기록하고 PASS로 쓰지 않는다.
  - 켜짐 회차를 일부 쓴 뒤 꺼지면, 남은 회차 대신 독립 검토를 한 번 한다.
- PR 전 브랜치 전체 검토는 이 PR 리뷰와 별개로 한다.
- 회차·지적 수·판정·이월 항목을 PR 본문과 PROGRESS에 남긴다.
- 타당하지 않은 지적은 판단 근거를 PR 본문에 남긴다.
- 켜짐에서 3회째 지적까지 처리하고 그 수정이 별도 검토·필수 CI를 통과하면 4회째 리뷰를 기다리지 않는다.
- 최종 SHA에 자동 리뷰가 없어도 위 규칙 안에서 끝났다면(꺼짐이면 독립 검토 1회로) 머지할 수 있다.
- 차단 기준: 일반 사용에서 실제로 사용자 데이터 손실·손상, 비밀 유출, 승인하지 않은 명령 실행이 일어나는 것만(ADR 0016 개인용 범위). 리뷰의 P1/P2 표시가 아니라 별도 검토 하위 에이전트의 재현·판정으로 정한다. 차단 지적의 수정은 별도 검토·직접 영향 검사·필수 CI로 확인하고 자동 리뷰를 다시 기다리지 않는다.

릴리스(사용자 결정 2026-10-26 "모든 릴리스는 main 브랜치에서"):
- 모든 릴리스는 main에서 한다. 버전 변경은 `release/<버전>` PR로 main에 squash 머지하고(#649 선례), 공개는 main의 그 커밋에 주석 태그를 만들어 push한다. 정식은 `vX.Y.Z`, 후보는 `vX.Y.Z-rc.N`(GitHub prerelease, stable 사용자는 받지 않는다)이다.
- 태그 push와 GitHub Release 공개는 그때마다 별도 명시 사용자 승인과 수용 근거가 있어야 한다(§11 "먼저 사용자에게 물을 것").
- 릴리스 노트는 주석 태그 메시지다. `scripts/release.py`가 그 메시지를 GitHub Release 본문으로 쓰고 빈 메시지는 거절한다.
- 버전은 manifest 네 곳(`Cargo.toml`의 workspace, `app/package.json`, `app/src-tauri/Cargo.toml`, `app/src-tauri/tauri.conf.json`)과 Cargo lock 두 곳(`Cargo.lock`, `app/src-tauri/Cargo.lock`의 workspace 항목)에 있다(예: `a6f75a68`). 한 커밋에서 바꾼다: 두 manifest에 `cargo update --workspace --offline`, `pnpm install --frozen-lockfile --offline` 통과(`pnpm-lock.yaml`에는 앱 버전이 없다), `scripts/tests/test_release.py`의 버전 fixture 확인, 런타임에 들어가는 경로에 옛 버전 0건, 같은 커밋에서 `cargo run -p xtask -- notices`로 고지 재생성과 `cargo test -p xtask notices` PASS.

보호 규칙을 약화하거나 우회하지 않는다. CI 이전 성공을 재사용하면 같은 입력·범위와 출처를 기록한다.
진행 커밋 완료와 PR 수용 완료를 구분한다. 릴리스 공개는 별도 명시 요청과 수용 근거가 필요하다.
전용 worktree 정리는 clean·merged 확인 → 제거 → prune → 로컬 브랜치 → 원격 브랜치 삭제 순서다.
공유 cache·사용자 자료·dirty/active/locked/unmerged/호스트 소유 worktree는 보존한다.

## 9. 기술 정책

추가 스택은 구체적인 사용자 필요·대안·배포/CI 영향을 확인한다. UI는 Tauri v2 하나다.
v0.9 데이터 이전은 하지 않는다. 기존 버전은 태그와 Git 이력으로 참조한다.

## 11. 기록과 인계

PROGRESS.md가 진행 상태의 유일한 원장이다. 결정·검증·미실행 항목은 현재 계획과 단일 PR 본문·artifact에 기록한다.
PROGRESS는 지금 상태만 200줄 이하로 적는다(구성은 그 파일 §0, 사용자 결정 4). 줄이기 전 전문과 끝난 계획은 [docs/history.md](docs/history.md)가 가리킨다.
새 workthrough·날짜별 복제 보고서를 만들지 않는다. 닫힌 이슈 #580(v0.9.0 출시 후 이슈·실사용 추적 원장)을 재오픈하거나 사용자 실기 요청을 만들지 않는다.
인계에는 목표·범위·branch/worktree·결정·검증·다음 과제를 남기고 재개 시 실제 Git/CI와 대조한다.
일반 개발은 별도 스킬을 요구하지 않는다. 승인된 계획은 과제별 구현 → 별도 검토 → 다음 과제 순서로 실행한다.
실행 하네스가 제공하는 하위 에이전트를 사용한다(과제마다 구현 → 별도 검토). 하위 에이전트가 없는 하네스는 같은 순서(구현 → 새 맥락에서 검토 → 커밋)를 직접 지킨다.

세션 시작(어느 세션이든 같은 절차로 시작한다):
1. 원본 체크아웃을 확인한다: `cd /home/jihoon/projects/devbox && git fetch origin && git status --short && git log -1 --oneline`. main이고 깨끗해야 한다. 아니면 고치지 말고 사용자에게 보고한다(다른 세션이 쓰는 중일 수 있다).
2. 진행 중인 일을 찾는다: `git worktree list`, `gh pr list --state open --json number,headRefName,title,isDraft`, `git ls-remote --heads origin 'v1/*' 'feat/*' 'fix/*' 'chore/*' 'docs/*' 'release/*'`. PROGRESS §1 현재 위치와 대조하고, 이 PC에 worktree가 없는데 원격 브랜치가 있으면 `git worktree add ../devbox-wt/<이름> <브랜치>`로 받는다. PR 없이 백업으로 push한 브랜치가 있을 수 있다(브랜치 push는 CI를 돌리지 않는다). 로컬과 원격이 다르면 더 앞선 쪽을 확인하고, 갈라졌으면 고치지 말고 사용자에게 묻는다.
3. 진행 중인 브랜치(여럿일 수 있다)의 PROGRESS를 읽고 `git -C <worktree> log --oneline origin/main..HEAD`로 끝난 과제(과제당 커밋 하나)를, `git -C <worktree> status --short`로 하던 과제를 확인한다. 커밋되지 않은 변경이 있으면 그 과제의 "실패 확인" 단계 시험을 다시 돌려 어디까지 됐는지 보고 이어서 한다.
4. 커밋되지 않은 변경은 절대 버리지 않는다(`git checkout -- .`·`git reset --hard`·`git clean`·`git stash drop` 금지). 이어갈 수 없으면 `git diff > /tmp/<브랜치>-<날짜>.patch`로 보존하고 사용자에게 묻는다.
5. 지금 과제의 계획 문서와 Spec 절을 읽고 도구·인증(§1 환경, `gh auth status`)을 확인한다.

세션 끝(또는 컨텍스트가 모자라 요약될 것 같을 때):
1. 완료한 과제는 검토·직접 영향 검사와 Biome CI 뒤 영어 Conventional Commit으로 남긴다. 미완료 변경은 그대로 보존하고 재개 지점을 기록한다. WIP 커밋·amend·force push를 의무화하지 않는다.
2. 해당 브랜치 PROGRESS의 §1 현재 위치와 §7 최근 세션(최근 10줄만 남긴다)에 완료/미완료·검증·다음 단계를 적어 커밋한다. §2 결정·§3 게이트·§4 후속이 바뀌었으면 함께 고친다. 새 보고서는 만들지 않는다.
3. push는 §8을 따르고 기존 미푸시 커밋을 보존한다.
4. 사용자에게 한 일·남긴 상태·다음 단계를 짧게 보고한다.

먼저 사용자에게 물을 것(혼자 정하지 않는다. 멈추고 묻고 PROGRESS §2에 적는다):
- 01-design의 구조·결정(D1–D16·Q1–Q4)을 바꿔야 함. 범위를 더하거나 빼야 함(01 §2.6 제외 항목 되살리기 포함). SC1–SC10 미달을 계획 안의 수정으로 고칠 수 없음. 계획 순서를 바꾸거나 PR 경계를 크게 바꿔야 함.
- 사용자 PC 설정 변경: `.wslconfig`, `/etc`, linger 켜기, `sudo`·`apt`·`winget` 설치, Windows 도구 설치. 사용자의 기존 tmux·systemd unit·Docker·방화벽을 건드려야 함(금지).
- 커밋되지 않은 작업 버리기, 머지 안 된 브랜치·worktree 강제 삭제(되돌릴 수 없음).
- 보호 규칙 변경이 권한 오류(403)로 막히거나 관리자 우회 머지가 필요해 보임(우회 금지). 태그 push·GitHub Release 공개(계획에 적힌 버전 변경은 제외), v0.9.0 긴급 수정.

멈추는 동안에도 그 결정에 기대지 않는 다른 과제는 계속한다. SC8 규모 예산 초과는 단순화 후보를 PROGRESS §2에 적어 알리되 기다리지 않는다.

혼자 정해도 되는 것: 계획 코드의 컴파일 오류를 같은 의도로 고치기, 시험 이름·파일 분할, 라이브러리 패치 버전, 문구 다듬기, 사용자 홈에 까는 개발 도구(`cargo install`, 저장소의 `pnpm install`), 저장소 범위의 git 설정(`merge.lockfile.driver`·`rerere`). 고친 것은 현재 계획의 그 과제 아래(계획 문서가 없으면 PROGRESS §1)와 PR 본문에 적는다.

과제 검토 기준(별도 검토자는 과제 문서·바뀐 파일·시험 결과를 보고 확인한다. 하나라도 어기면 되돌려 보낸다):
1. 실패하는 시험을 먼저 썼고, 그 시험과 바꾼 crate·화면의 시험이 통과한다.
2. 이름·시그니처가 과제의 Interfaces → Produces와 같다(다르면 현재 계획의 그 과제 아래나 PROGRESS §1에 적었는가).
3. 계획 문서의 Global Constraints와 01-design §5.3(오류·`Result<T, String>` 허용 범위)·§8.6(정합성 규칙)을 지킨다.
4. 바꾼 도메인의 01-design §9.1 회귀 조건과 §9.4 추적표의 시험이 이 과제에 걸려 있으면 들어 있다.
5. 과제 범위를 넘는 기능·리팩터링·방어 장치가 없다. 파일 600줄·컴포넌트 300줄을 넘지 않는다.
6. 비밀·문서 내용·경로가 로그에 나가지 않는다. 사용자의 기존 tmux·unit·Docker를 건드리지 않는다.

막혔을 때:
- 계획 코드대로 컴파일·시험이 안 됨: 같은 의도로 고치고 현재 계획의 그 과제 아래(계획 문서가 없으면 PROGRESS §1)와 PR 본문에 적는다. 의도 자체가 틀렸으면 위 "먼저 사용자에게 물을 것".
- 계획 문서의 이름과 이미 머지된 코드가 다름: 코드가 사실이다. 다음 상세화 과제에서 계획을 고친다.
- CI만 실패하고 로컬은 통과: 원인을 고친다. flaky면 그 PR에서 고치거나 지운다(재시도 래퍼 금지).
- 시험이 사용자 환경(systemd user·tmux)에 따라 다름: 시험이 `DEVBOX_INSTANCE=test-<난수>` 자원만 쓰는지 확인한다. CI에서 systemd user를 못 쓰면 01-design §11의 대체를 쓴다(그 시험만 WSL 로컬 필수).
- 디스크 부족: `du -sh ~/.cache/targets/devbox` 뒤 §5의 target 정리를 한다. 그래도 모자라면 사용자에게 묻는다.
- push가 `push declined due to email privacy restrictions`로 거절됨: 그 브랜치의 내 커밋을 §8의 noreply 작성자로 다시 쓴다(`git -c … commit --amend --no-edit --reset-author`, 여러 개면 `git rebase -x '…같은 명령…' origin/main`).
