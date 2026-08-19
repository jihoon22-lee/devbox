# Changelog

이 프로젝트의 모든 주요 변경사항은 이 파일에 기록한다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며, 버전은 `vX.Y.Z` 태그와 함께 릴리스된다.

## [v0.4.1-rc3] - 2026-08-20

v0.4.1-rc3는 RC2 Windows acceptance 중 발견된 Run Manager 시작 결함을 수정한 추가 acceptance
후보 빌드다. 이 후보는 안정판 v0.4.1의 Windows acceptance가 완료됐다는 뜻이 아니다.

### Fixed

- **run-manager 0.3.2** — RC2 Windows acceptance에서
  `apps/run-manager/src-tauri/src/lifecycle.rs:144`의 scheduler `tokio::spawn`이 Tauri `setup`
  경계에서 현재 Tokio runtime 없이 실행되어 panic하고 프로세스가 즉시 종료되는 현상을 직접
  관찰했다. 후속 코드 검토와 회귀 테스트에서 maintenance task에도 같은 setup-runtime 결함이
  있음을 확인했으며, 두 lifecycle task를 Tauri가 구성한 async runtime에서 시작하도록 변경했다.

### Tests

- 동기 `setup` 경계에서 scheduler와 maintenance task가 panic 없이 시작·종료되는 회귀 테스트를
  추가했다.

### Release status

- RC3는 수정 사항을 확인하기 위한 Windows acceptance 후보이며, 선택된 실기 검증이 끝날 때까지
  안정판 v0.4.1로 간주하지 않는다.

## [v0.4.1-rc2] - 2026-08-20

v0.4.1-rc2는 코드 변경과 자동화 게이트를 반영한 Windows acceptance 후보 빌드다. 이 후보는
Windows 실기 실행이 완료됐다는 뜻이 아니며, 선택된 Windows acceptance 검증을 남겨 둔다.

### Fixed

- **wsl-desktop 0.3.2** — Windows cwd를 변환하거나 셸 문자열로 조립하지 않고 `wsl.exe --cd`의
  별도 argv 값으로 그대로 전달해 공백이 있는 경로도 경계를 보존한다.
- **wsl-desktop** — 자연 종료된 터미널 세션의 리소스 정리를 추가하고, 오래된 reader가 교체된
  세션을 지우지 않도록 teardown 및 cleanup 경합을 안전하게 처리했다.

### Tests

- resize 거부 후 재시도와 활성화 시 대기 중 resize 취소 회귀 테스트를 추가했다.
- 동시 세션 생성에서 ID가 충돌하지 않는지 검증하는 회귀 테스트를 추가했다.

### Release gate

- 릴리스 노트 섹션이 없거나 비어 있으면 실패하도록 추출 게이트를 fatal로 변경했다.
- RC2는 Windows acceptance 후보이며, 안정판 v0.4.1 배포를 위한 Windows runtime 완료를 주장하지 않는다.

## [v0.4.1-rc1] - 2026-08-19

v0.4.1 핫픽스의 릴리스 후보(v0.4.1-rc1)다. 터미널 PTY 전송과 앱 간 링크의 결함을 수정했으며,
Windows 수동 검증 매트릭스를 위한 후보 빌드다.

### Fixed

- **wsl-desktop** — PTY 읽기 경계에서 잘리던 UTF-8을 carry 버퍼로 재조립하고, ConPTY 빌드 정보·Unicode11·
  스크롤백·리사이즈 하드닝을 적용해 한글·박스 드로잉과 창 크기 변경 시 터미널 화면 손상을 줄였다. v0.3.0에서
  `bash -lc "cd ..."`가 파일명으로 처리되던 Open Terminal 실패도 `wsl.exe -d <distro> --cd <cwd> --` 형태의
  정확한 분리 argv로 수정했다.
- **앱 간 링크** — `devbox://open` 수신과 single-instance pending-open 전달을 보강해 콜드/웜 시작 모두 대상 경로를
  소비하도록 했다. repo-manager는 Code Pad에 `Workspace`, WSL Desktop·Workbench에 구체적인 `Path`를 보내며,
  Workbench도 WSL Desktop에 프로필 id 대신 실제 프로젝트 경로를 전달한다.

### Release status

- 이 RC의 검증 범위는 Windows 실기·패키지·프로토콜·경로·시각 수동 검증이며, 결과는 안정판 배포 판단의 근거로
  사용한다.

## [v0.4.0] - 2026-08-18

기능 추가 릴리스. 신규 앱 3종(Workbench, Webhook Lab, Repo Manager)과 devbox-manager 환경 진단이 추가되어
총 13개 앱이 되었다. 배포 워크플로가 카탈로그 기반으로 정비되고, 신규 앱에도 CSP 기준선이 적용되었다.

### 업그레이드 참고 (v0.3.0 이하 사용자)

PR #180에서 릴리스 asset 명명이 `<ProductName>_<태그버전>_x64-setup.exe`에서
`<app-id>_<앱버전>_x64-setup.exe`로 바뀌었고, portable 설치 layout도
`apps/<id>/<tag>/<id>.exe`에서 `apps/<id>/versions/<version>/<id>.exe`로 바뀌었다.
**v0.3.0 이하 Devbox Manager는 이 새 이름·layout을 인식하지 못해 "Install (setup)"
버튼이 동작하지 않는다** (휴대용 재설치는 계속 동작한다). Releases 페이지에서 새
Devbox Manager를 직접 내려받아 먼저 설치한 뒤, 나머지 앱을 새 Manager로 설치하세요.

### Added

- **workbench (신규)** — 프로젝트 기반 orchestration 셸. ProjectProfile CRUD(기존 wsl-desktop·life-log 저장소 흡수),
  Git/WSL/포트/서비스 사전 점검, Run Manager 서비스·WSL Desktop layout·Code Pad workspace 시작,
  idempotent 실행 기록과 `Stop What I Started`(Workbench가 시작한 자원만 정리).
- **webhook-lab (신규)** — 로컬 웹훅/콜백 서버(inbound HTTP). method/path별 request history, 응답 rule·지연·오류 재현,
  `Authorization`·`Cookie`·API key 헤더 masking, body/history 상한,
  기본 bind 127.0.0.1 + LAN 공개는 명시적 설정.
- **repo-manager (신규)** — Git repository 탐색·브랜치/dirty/ahead-behind/worktree 상태 목록, worktree 생성,
  Code Pad·WSL Desktop·Workbench로 열기. force delete·reset·clean 기본 동작 없음, worktree remove 전
  uncommitted/untracked 검사.
- **devbox-manager** — 환경 진단(dev environment doctor) 탭: WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids 점검.
- **crates** — `crates/wsl`, `crates/search`, `crates/integration`, `crates/secrets`가 공용 크레이트로 추출되었다.
- **packages** — `packages/tokens`, `packages/editor`, `packages/diff-view`가 공용 React 패키지로 추출되었다.
- **배포 정비** — 릴리스 워크플로가 `apps/catalog.json`에서 빌드 대상을 읽고, portable·installer를 staging해
  `release-manifest.json`(asset 명칭·SHA-256)과 함께 게시하며 verify 단계가 asset을 대조한다.
- **CSP 기준선** — 신규 앱 3종에도 `default-src 'self'; ...; connect-src 'self' ipc: http://ipc.localhost` 기준선 적용.

### Changed

- 앱 카탈로그가 13개로 확장 (`apps/catalog.json`).
- GitHub Releases 산출물 명칭 통일: 휴대용 `<app-id>.exe`, 설치 `<app-id>_<version>_x64-setup.exe`.
- 앱 버전을 기능 추가·수정에 맞게 개별 갱신 (버전은 각 앱이 독립적으로 가져간다):
  - 0.2.0 → **0.3.0**: api-playground(컬렉션·환경·시크릿), everything-plus(watcher·결과 액션),
    knowledge-base(CodeMirror·watcher·snapshot), devbox-manager(카탈로그·manifest·원자 설치·환경 진단)
  - 0.2.2 → **0.3.0**: life-log(활동 추적 흡수·idle·privacy·자동 시작·프로젝트 귀속), wsl-desktop(wsl-dashboard 흡수·탭)
  - 0.3.0 → **0.3.1**: code-pad(복구·problems·탐색 이력), run-manager(관찰성·export/import)
  - 0.2.0 → **0.2.1**: port-manager, developer-toolbox (identifier 이관 등 내부 정비)
  - 0.1.0 → **0.1.1**: repo-manager (git·앱 실행 안정화)

### Fixed

- **repo-manager·workbench·life-log** — Windows에서 `git` 하위 프로세스가 실패해 브랜치 `?`/`n/a`,
  커밋 수 0으로 표시되던 문제 수정 (`crates/git`).
- **repo-manager·workbench** — 설치된 앱 실행(`open_in`·Start Workspace)이 잘못된 exe명으로 실패하던 문제 수정 (`crates/launch`).
- **devbox-manager** — GitHub release asset redirect 대상 변경(`release-assets.githubusercontent.com`) 미반영 수정.
- **devbox-manager** — 환경 진단 WSL 버전 UTF-16LE 깨짐 수정.
- **life-log** — 실행 파일 중복 실행 방지(단일 인스턴스 + 기존 트레이 포커스).
- **wsl-desktop** — grid 행 높이 불균형·팬 간 이동 불가 수정 (Alt+Arrow).
- **code-pad** — 창 축소 시 하단 잘림 수정 (`.content-area` 높이 제약).
- **workbench** — Windows에서 `wsl.exe -l -v` 출력을 UTF-16LE로 디코딩하지 않아 프로젝트 사전 점검(project health)의
  WSL 배포판 확인이 항상 "distro 없음"으로 표시되던 문제 수정. devbox-manager(#183)와 같은 원인으로, 공용
  `crates/wsl` 디코더를 재사용했다 (#192).
- **repo-manager** — `scan_root`가 탐색 깊이 제한·제외 규칙 없이 전체 파일시스템을 재귀 탐색해 `node_modules`·
  `target`·`AppData` 등까지 들어가고 Windows junction 순환에 취약하던 문제 수정. 비-repo 디렉터리 가지치기와
  탐색 깊이·방문 디렉터리 상한을 추가했다. 상한에 걸리면 `scan_root`가 `truncated` 플래그를 반환하고 화면에
  배너로 알린다 (#193).

### 알려진 문제

- **wsl-desktop 터미널 출력 손상.** PTY 읽기 경계에 걸친 멀티바이트 문자(한글·박스드로잉)가 손상돼 화면이
  간헐적으로 깨지고, `htop`/`vim`/`lazygit` 같은 TUI의 프레임이 어긋난다. 긴 줄이 있는 상태에서 창 크기를
  바꾸면 기존 출력이 망가지는 문제도 함께 있다.
  설계·수정 계획: [`docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`](docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md) §2
- **앱 간 "다른 앱으로 열기"가 경로를 전달하지 못한다.** repo-manager의 Code Pad/WSL Desktop/Workbench 열기와
  workbench의 Start Workspace는 대상 앱을 실행하지만, 대상 앱이 명령줄 인자를 읽지 않아 빈 상태로 열린다.
  설계·수정 계획: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](docs/superpowers/specs/2026-08-17-app-interop-design.md) §5.1

## [v0.3.0] - 2026-08-13

기능 추가 릴리스. 신규 앱 2종(Code Pad, Run Manager)이 추가되어 총 12개 앱이 되었다.

### Added

- **code-pad (신규)** — CodeMirror 6 기반 경량 코드 에디터. 문법 하이라이팅, 탭·분할 2뷰, 찾기/바꾸기(정규식),
  인코딩/줄바꿈(CRLF/LF) 감지·변환, 큰 파일 가드, `.md`/`.mmd` 프리뷰. 언어 중립 LSP 클라이언트와 Windows 로컬 stdio
  서버 관리(진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프)를 제공하며, rust-analyzer·typescript-language-server·
  basedpyright·vscode-langservers-extracted를 검증된 고정 버전으로 설치한다.
- **run-manager (신규)** — 예약 실행(크론 잡)과 상시 실행(서비스)을 한곳에서 관리. 1초 스케줄러, occurrence 원자적 claim,
  중복 실행 정책(skip/queue/kill-previous), Windows(Job Object)·WSL(session/group) 실행 어댑터, DPAPI 환경변수 보호,
  stdout/stderr 회전 로그 tail, 실패 Windows toast 알림. 서비스는 start/stop/restart·자동 시작·재시작 정책(never/on-failure/always)·
  백오프·프로세스 생존/로컬 TCP 헬스체크를 지원한다.
- **crates** — `crates/filesystem`(제한 순회 API), `crates/markdown`, `crates/process`가 공용 크레이트로 추출되었다.

### Changed

- code-pad·run-manager 버전을 0.3.0으로 설정.

### Fixed

- (v0.2.x에서 수정된 항목 유지)

## [v0.2.2] - 2026-08-11

### Fixed

- **wsl-desktop** — \`+ Terminal\` 클릭 시 \`wsl.exe\`가 \`0xc0000142\`(DLL initialization failed)로 실패하고 터미널이 열리지 않던 문제 수정. portable-pty의 ConPTY(HPCON)를 보관하는 master가 세션 종료 시 함께 닫히면서, 아직 시작 중인 wsl.exe가 잘못된 pseudoconsole 핸들로 초기화를 시도했던 것. master를 세션 핸들에 보관해 ConPTY 수명을 유지하도록 함.
- **life-log** — 주간/월간 조회 시 "No activity in this period"만 표시되던 문제 수정. 기본 활동 데이터 소스 경로가 activity-timeline의 실제 저장 경로(\`%LOCALAPPDATA%\\com.workbench.activitytimeline\\data.db\`)와 달랐던 것. 문서(CONVENTIONS·windows-guide)의 데이터 위치도 실제 identifier 기준 경로로 정정.

## [v0.2.1] - 2026-08-11

### Fixed

- **wsl-desktop** — Windows에서 터미널 창이 열리지만 shell이 로딩되지 않던 문제 수정. portable-pty 0.9의 ConPTY 시작 교착(PSEUDOCONSOLE_INHERIT_CURSOR가 커서 위치 조회 후 응답을 기다려 자식 프로세스를 정지)을 해소하기 위해 세션 시작 직후 `ESC[1;1R`을 입력 파이프로 전송.

## [v0.2.0] - 2026-08-11

기능 추가 릴리스. 신규 앱 2종(WSL Desktop, Devbox Manager)이 추가되어 총 10개 앱이 되었다.

### Added

- **wsl-desktop (신규)** — 임베디드 터미널(xterm.js + PTY). WSL 배포판 선택·지정 경로로 터미널 열기, 격자/세로/가로 분할, 여러 터미널에 동시 명령(broadcast).
- **devbox-manager (신규)** — devbox 앱 버전 체크·설치·업데이트·실행. 휴대용 exe 또는 설치 패키지 방식 선택.
- **life-log** — 캘린더 날짜 선택, 로딩 표시, 주간/월간 조회(일별 사용량 차트), 지난 날짜 세션 캐시.
- **api-playground** — 현재 요청을 curl 명령으로 변환·복사.
- **activity-timeline** — 30초 자동 새로고침.
- **everything-plus** — re-index 진행률 표시, 정규식 검색 모드, 텍스트 내용 검색(확장자 선택·루트별 옵션).
- **wsl-dashboard** — Docker 미설치 안내 배너, 프로젝트 경로 입력 수정·localStorage 저장.

### Changed

- 모든 앱 버전을 0.2.0으로 통일.

### Fixed

- (v0.1.1에서 수정된 항목 유지)

## [v0.1.1] - 2026-08-11

Windows에서 발견된 버그 수정 릴리스. port-manager / wsl-dashboard / knowledge-base / life-log의 버전을 0.1.1로 올렸다.

### Fixed

- **port-manager** — 한국어 Windows에서 `netstat` 출력(OEM 코드페이지, 예: CP949)이 UTF-8 디코딩을 실패하던 문제 수정. 자식 프로세스의 콘솔 창 깜빡임도 제거.
- **wsl-dashboard** — 파이프로 실행된 `wsl.exe -l -v`가 UTF-16LE(NUL 포함)로 출력돼 "null byte found in provided data" 오류가 나던 문제 수정 (UTF-16 디코딩 추가). wsl/git 자식 프로세스의 콘솔 창 깜빡임도 제거.
- **knowledge-base** — Windows 절대경로 처리 버그로 파일 작업·데일리 노트에서 "경로가 루트 밖을 벗어납니다"가 나던 문제 수정.
- **life-log** — git 커밋 집계 자식 프로세스의 콘솔 창 깜빡임 제거.

## [v0.1.0] - 2026-08-11

최초 릴리스: 8개 데스크톱 앱 (Tauri v2, Rust + React).

### Added

- **port-manager** — 포트/프로세스 조회·검색·필터, 프로세스 종료, localhost 열기
- **developer-toolbox** — 14종 개발 도구 (JSON/Base64/URL/타임스탬프/Case/Hash/UUID/Regex/Diff/JWT)
- **wsl-dashboard** — WSL 배포판·Docker·git 상태 대시보드, 컨테이너 start/stop/restart
- **api-playground** — REST 요청 빌더, CORS 없는 응답 확인, 요청 history
- **activity-timeline** — 포그라운드 창 기반 사용 기록, 하루 타임라인·앱 통계, 트레이 상시 실행
- **everything-plus** — 파일명 FTS5 인덱스·검색, 루트 관리, 백그라운드 재인덱스
- **knowledge-base** — 마크다운 저장소, 태그, 본문 검색, 데일리 노트
- **life-log** — 활동·git 집계 일일 요약, 데이터 소스 설정

### Changed

- 없음 (최초 릴리스)

### Fixed

- 없음 (최초 릴리스)

### Known issues

- 개인 빌드라 코드 서명이 없어 설치 시 SmartScreen 경고가 표시된다 (`추가 정보 → 실행`).
- activity-timeline 포그라운드 추적, wsl-dashboard 등 **Windows 전용 기능은 Windows에서만 동작**한다.
- everything-plus 내용(body) 검색은 v2 예정. 현재는 파일명 검색만 지원.
