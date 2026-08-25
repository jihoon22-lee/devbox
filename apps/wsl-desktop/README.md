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
- **동시 명령(broadcast)** — 여러 터미널에 같은 명령 전송
- **상태 패널** — WSL 배포판 / Docker
- **open path 핀·최근 경로** — 자주 쓰는 작업 경로 저장

## 기술

- `portable-pty` 기반 ConPTY (PTY resize, 탭 모델, 드래그 2종, 단축키 5종)
- 공용 `packages/context-menu` — viewport 배치·keyboard navigation·focus 복원·submenu를
  공유하고, WSL 전용 항목·exact pane/tab 대상·danger 확인은 앱이 소유한다.
- 공식 xterm MIT addon(`addon-search`, `addon-web-links`)과 Tauri clipboard plugin을 앱에
  포함하므로 설치 뒤 검색·링크 감지·붙여넣기는 network나 별도 외부 도구 없이 동작한다.
  clipboard capability는 읽기 텍스트 하나만 허용한다.
- 공용 크레이트 `crates/wsl` — 프로세스를 실행하지 않는 WSL 공용 프리미티브로, `wsl.exe` 실행
  argv(`--cd` 포함)·`wslpath` argv 조립, distro 이름 검증, WSL 출력 디코딩, Windows↔WSL 경로와
  canonical project key 정규화를 제공한다.
- WSL 기준선은 `wsl.exe --cd <cwd>`를 지원하는 최신 Microsoft Store WSL이다. 구형 inbox WSL은
  `wsl --update`로 먼저 업데이트하는 것을 권장한다. WSL2는 필요하면 `wsl --install` 후 재부팅하며,
  Docker 컨테이너 관리에는 Docker Desktop이 필요하다.

## 데이터

- 프로젝트·git 상태는 Workbench로 이관됨 (`com.devbox.workbench\project-profiles.json`)
- `localStorage`: cwd 핀·최근 경로 5개, selection 자동 복사 여부, 터미널 글꼴 크기. 터미널
  출력·selection·clipboard 내용은 저장하지 않는다.

## 개발

- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: [`docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`](../../docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md)
