# wsl-desktop — WSL Desktop

앱 안에 내장된 임베디드 WSL 터미널. Windows Terminal처럼 탭·분할로 여러 WSL 세션을 관리한다.
산출물: `WSLDesktop.exe` (`apps/wsl-desktop`).

## 주요 기능

- **임베디드 터미널** — xterm.js + PTY(ConPTY), WSL 배포판 선택·지정 경로로 열기
- **탭 + 분할** — 탭 안에 격자/가로/세로 분할, 드래그로 탭 이동·재배치, 단축키 전환
- **동시 명령(broadcast)** — 여러 터미널에 같은 명령 전송
- **상태 패널** — WSL 배포판 / Docker / 프로젝트·git 상태
- **open path 핀·최근 경로** — 자주 쓰는 작업 경로 저장

## 기술

- `portable-pty` 기반 ConPTY (PTY resize, 탭 모델, 드래그 2종, 단축키 5종)
- 공용 크레이트 `crates/wsl` 사용 (argv·경로 정규화)
- WSL2 필요: `wsl --install` 후 재부팅 (Docker 컨테이너 관리엔 Docker Desktop 필요)

## 데이터

- 프로젝트·git 상태는 Workbench로 이관됨 (`com.devbox.workbench\project-profiles.json`)

## 개발

- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-wsl-desktop-tabs-design.md`
