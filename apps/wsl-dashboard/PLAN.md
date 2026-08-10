# wsl-dashboard — WSL Dashboard

WSL2 개발환경(Ubuntu, Docker, Git, 포트)을 Windows GUI로 관리하는 대시보드 앱.
`crates/process`(port-manager에서 추출)와 `crates/wsl`을 재사용한다.
산출물: `WSLDashboard.exe`. 모노레포 위치: `devbox/apps/wsl-dashboard`.

## 1. 목표
- WSL 배포판 상태(실행 여부, 메모리, CPU) 한눈에 확인
- Docker 컨테이너 실행/중지/재시작
- 프로젝트별 git 상태(브랜치/변경사항) 확인
- 터미널·VS Code 열기, WSL 재시작 등 액션

## 2. 핵심 기능

### MVP (v1)
| 기능 | 설명 |
|---|---|
| 배포판 카드 | `wsl.exe -l -v` 파싱 → 이름/버전/상태/기본 여부 |
| 시스템 리소스 | WSL 안에서 `free`/`top` 또는 Docker 통계로 메모리·CPU |
| Docker 목록 | `docker ps -a` 파싱, 컨테이너 start/stop/restart |
| 프로젝트 목록 | config에 등록된 경로들의 git branch/status |
| 액션 | Open Terminal(wt.exe), Open VS Code, Restart WSL |

### v2+
- Docker 이미지·볼륨·로그 보기
- WSL 내 개발 서버 목록 (포트 매핑, port-manager 연동)
- WSL 내 파일 시스템 빠른 탐색 (everything-plus 연동)
- 자원 사용률 시계열 차트

## 3. 기술 설계

### 명령 실행 패턴 (전체의 핵심)
```rust
let output = std::process::Command::new("wsl.exe")
    .args(["-d", "Ubuntu", "--", "docker", "ps", "-a"])
    .output().await?;
```
- 모든 명령은 `tokio::process::Command`로 async 실행
- 파싱은 각각 앱 로컬 `core/`(→ 이후 `crates/wsl`)에 격리 + 유닛 테스트 (샘플 출력 픽스처)
- 실행은 모두 `wsl.exe -d <distro> -- <cmd>` 형태로 통일 (Docker Desktop CLI는 WSL 안에서 호출)

### Rust 모듈
- `commands/wsl.rs` — `list_distros()`, `distro_status()`, `run_command(distro, cmd)`
- `commands/docker.rs` — `docker_ps()`, `docker_action(container_id, action)`
- `commands/git.rs` — `git_status(project_path)` (git CLI 호출, 서브프로세스)
- `commands/actions.rs` — `open_terminal()`, `open_vscode(path)`, `restart_wsl()`
- `core/parsers.rs` — wsl/docker/git 출력 파서 → 중복 발생 시 `crates/wsl`로 추출
- 프로세스/포트 기능은 `crates/process` 사용 (port-manager에서 추출)
- `db.rs` 없음 — 설정은 `config.json` (등록 프로젝트 경로 목록, 기본 distro)

### 데이터 모델
- `DistroInfo { name, version, state, default }`
- `ContainerInfo { id, name, image, status, ports }`
- `GitStatus { branch, changes, ahead/behind, clean }`

## 4. UI 설계
```
[WSLDashboard]  배포판: Ubuntu 24.04 ● Running
  Ubuntu 24.04           Ubuntu 22.04
  Status  ● Running      Status  ○ Stopped
  Memory  4.2 GB
  CPU     7%
  [Open Terminal] [Open VS Code] [Restart WSL]

Docker ─────────────────────────────
  postgres    ● Running   [Stop] [Logs]
  redis       ● Running   [Stop] [Logs]
  nginx       ○ Stopped   [Start]

Projects ───────────────────────────
  FamilyCard    main     clean
  port-manager  dev      3 changes
```
- 상단 탭: Overview / Docker / Projects / Ports(WSL 내부)
- 설정 페이지: 프로젝트 경로 추가/삭제, 기본 distro 선택

## 5. 구현 단계
1. 스캐폴드 + 공통 규약
2. `wsl.exe` 호출 헬퍼 + 파서 테스트 (distro 목록/상태)
3. Overview 카드 UI (distro + 리소스)
4. Docker 목록 + start/stop/restart (async 명령)
5. 프로젝트 git 상태 조회 + 테이블
6. 액션 버튼 (터미널/VS Code/WSL 재시작) — port-manager의 `open_browser` 방식 재사용
7. 설정(config.json) + 포트 탭 (`crates/process` 재사용)
8. Windows 빌드 검증

## 6. 테스트
- Rust: 각 파서 유닛 테스트 (wsl/docker/git 샘플 출력)
- 통합: 실제 Docker 컨테이너 하나로 start/stop 반복 검증
- 프론트: 카드/테이블 컴포넌트 스모크

## 7. 확장/연계
- port-manager: `crates/process`로 포트 스캐너 재사용 (WSL 내부 포트도 netstat로)
- activity-timeline: WSL 사용 시간 추적 소스
- life-log: git activity 소스로 직결
- 공통 추출 후보: `crates/wsl`(명령 실행 헬퍼·파서), async 명령 훅

## 8. 완료 정의(Done)
- 배포판·Docker·git 상태 실시간 조회 및 액션 동작
- 파서 테스트 통과, async 명령에 타임아웃·에러 처리 포함
- Windows 배포 빌드 성공
