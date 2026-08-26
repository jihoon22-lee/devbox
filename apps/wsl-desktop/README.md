# wsl-desktop — WSL Desktop

앱 안에 내장된 임베디드 WSL 터미널. Windows Terminal처럼 탭·분할로 여러 WSL 세션을 관리한다.
산출물: `WSLDesktop.exe` (`apps/wsl-desktop`).

## 주요 기능

- **임베디드 터미널** — xterm.js + PTY(ConPTY), WSL 배포판 선택·지정 경로로 열기
- **탭 + 분할** — 탭 안에 격자/가로/세로 분할, 드래그로 탭 이동·재배치, 단축키 전환
- **클립보드** — 선택 자동 복사(기본 켬, 설정 저장), `Ctrl+Shift+C/V`, 가운데 버튼
  붙여넣기. `Ctrl+C`는 선택 유무와 무관하게 항상 셸의 SIGINT로 남고, 개행이 든
  붙여넣기는 내용 대신 줄 수만 표시해 확인한다. 단일 paste는 최대 1,000,000자다.
- **검색·메타데이터·링크** — 팬별 스크롤백 검색(`Ctrl+Shift+F`, Enter/Shift+Enter),
  OSC 0/2 제목과 OSC 7 현재 cwd, OSC 8·일반 HTTP(S) 링크를 지원한다. 자동 탭 제목은
  활성 팬을 따르지만 사용자가 바꾼 탭 이름은 OSC가 덮어쓰지 않는다. 링크는 scheme과
  자격 증명을 검사하고 host 확인 뒤 기본 브라우저에서 연다. 검색어는 최대 512자다.
- **팬·탭 컨텍스트 메뉴** — 팬 복사·붙여넣기·검색·세로/가로 분할·cwd 복사·확인 후
  닫기, 탭 닫기·다른 탭 닫기·이름 변경·레이아웃 전환. 우클릭과
  `Shift+F10`/Menu 키를 지원하고 닫힌 뒤 실제 터미널로 focus를 복원한다. 복사와 cwd
  복사는 우클릭한 exact 팬에 selection 또는 유효한 OSC 7 값이 있을 때만 활성화된다.
- **글꼴·스크롤백·resize** — 글꼴 크기 `Ctrl++/-/0`과 툴바 조절값을 저장하며 xterm을
  재마운트하지 않는다. 10,000줄 scrollback, ConPTY wrap 보정, resize ack 후 commit·실패
  재시도, hidden pane/최소 크기 보호를 적용한다.
- **워크스페이스·프로필** — stable pane key로 마지막 탭/팬/distro/cwd/layout/시작 명령을
  복원하고, 현재 구성을 이름 있는 터미널 프로필로 저장한다. `OpenTarget::Profile` cold/hot
  요청은 같은 전환 경로를 사용한다. 시작 명령은 실행 전에 최종 문자열을 확인하고 새 세션에
  한 번만 보낸다.
- **명령 팔레트** — `Ctrl+Shift+P`에서 활성 팬 분할·닫기·출력 검색·cwd 복사와 프로필
  전환을 키보드로 실행한다.
- **동시 입력(broadcast)** — 기본 OFF. 활성 탭의 팬을 2개 이상 직접 선택하고 대상 수를
  확인해야 켤 수 있다. 여러 줄 붙여넣기와 위험 명령 Enter는 대상 수와 실행 위험을 다시
  확인하며 취소한 위험 명령은 다음 Enter에서도 재확인한다.
- **선택적 프로세스 유지** — native workspace는 외부 도구 없이 완전하게 동작한다. 이미
  설치된 tmux/zellij만 감지해 stable `wsld-*` 세션에 opt-in attach하며, 설치·download하지
  않고 부재/감지 실패 시 native로 폴백한다.
- **상태 패널** — WSL 배포판과 선택 distro의 Docker 컨테이너를 표시한다. 260px의 좁은
  패널에서도 이름·정규화 상태·축약 port mapping을 먼저 보여 주고, 컨테이너를 펼치면 Docker가
  반환한 ID·image·status·ports 원문을 확인하고 start/stop/restart할 수 있다. Docker가 없으면
  설치 안내만 표시하며 engine 설치·설정·리소스 관리는 수행하지 않는다.
- **open path 핀·최근 경로** — 자주 쓰는 작업 경로 저장

## 기술

- `portable-pty` 기반 ConPTY (PTY resize, 탭 모델, 드래그와 앱/터미널 단축키)
- 공용 `packages/context-menu` — viewport 배치·keyboard navigation·focus 복원·submenu를
  공유하고, WSL 전용 항목·exact pane/tab 대상·danger 확인은 앱이 소유한다.
- 공식 xterm MIT addon(`addon-search`, `addon-web-links`)과 Tauri clipboard plugin을 앱에
  포함하므로 설치 뒤 검색·링크 감지·붙여넣기는 network나 별도 외부 도구 없이 동작한다.
  clipboard capability는 읽기 텍스트 하나만 허용한다.
- tmux/zellij 어댑터는 shell 문자열 조립 없이 exact argv만 사용한다. tmux UI option은 해당
  session에만 적용하고 zellij는 내장 `disable-status` layout과 frame/mouse off option을
  사용해 앱의 탭·팬 UI와 xterm selection을 유지한다.
- 공용 크레이트 `crates/wsl` — 프로세스를 실행하지 않는 WSL 공용 프리미티브로, `wsl.exe` 실행
  argv(`--cd` 포함)·`wslpath` argv 조립, distro 이름 검증, WSL 출력 디코딩, Windows↔WSL 경로와
  canonical project key 정규화를 제공한다.
- Docker 목록은 기본 공백 table을 추측해 파싱하지 않고 `docker ps -a --no-trunc --format`으로
  ID/name/image/status/ports 다섯 필드만 요청한다. 요약용 상태·port만 frontend에서 파생하며 원문
  필드는 변경하거나 저장하지 않는다. COMMAND, 환경 변수, credential과 resource summary는
  조회하지 않는다.
- WSL 기준선은 `wsl.exe --cd <cwd>`를 지원하는 최신 Microsoft Store WSL이다. 구형 inbox WSL은
  `wsl --update`로 먼저 업데이트하는 것을 권장한다. WSL2는 필요하면 `wsl --install` 후 재부팅하며,
  컨테이너 패널에는 선택 distro에서 실행 가능한 Docker CLI와 engine이 필요하다. devbox가 이를
  자동 download하거나 설치하지 않는다.

## 데이터

- 프로젝트·git 상태는 Workbench로 이관됨 (`com.devbox.workbench\project-profiles.json`)
- `localStorage`: cwd 핀·최근 경로 5개, selection 자동 복사 여부, 터미널 글꼴 크기, version 1
  마지막 레이아웃. 터미널 출력·selection·clipboard 내용과 runtime session id는 저장하지 않는다.
- Docker 컨테이너 목록과 detail 원문은 runtime memory에만 두며 localStorage나 profile에 저장하지
  않는다.
- `app_local_data_dir/terminal-profiles.json`: version 1 이름 있는 터미널 프로필. atomic replace,
  탭 16개·팬 32개·한 줄 시작 명령 4,096자 제한, 참조 무결성·안전한 절대 cwd·명백한 평문
  credential 검증을 적용한다.

## 개발

- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: [`docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`](../../docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md)
