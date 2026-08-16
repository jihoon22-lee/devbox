# Changelog

이 프로젝트의 모든 주요 변경사항은 이 파일에 기록한다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며, 버전은 `vX.Y.Z` 태그와 함께 릴리스된다.

## [v0.4.0] - 2026-08-15

기능 추가 릴리스. 신규 앱 3종(Workbench, Webhook Lab, Repo Manager)과 devbox-manager 환경 진단이 추가되어
총 13개 앱이 되었다. 배포 워크플로가 카탈로그 기반으로 정비되고, 신규 앱에도 CSP 기준선이 적용되었다.

### Added

- **workbench (신규)** — 프로젝트 기반 orchestration 셸. ProjectProfile CRUD(기존 wsl-desktop·life-log 저장소 흡수),
  Git/WSL/포트/서비스 사전 점검, Run Manager 서비스·WSL Desktop layout·Code Pad workspace 시작,
  idempotent 실행 기록과 `Stop What I Started`(Workbench가 시작한 자원만 정리).
- **webhook-lab (신규)** — 로컬 웹훅/콜백 서버(inbound HTTP). method/path별 request history, 응답 rule·지연·오류 재현,
  JSON fixture 저장, 수신 요청의 API Playground request 변환, `Authorization`·`Cookie`·API key 헤더 masking,
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
