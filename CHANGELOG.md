# Changelog

이 프로젝트의 모든 주요 변경사항은 이 파일에 기록한다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며, 버전은 `vX.Y.Z` 태그와 함께 릴리스된다.

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
