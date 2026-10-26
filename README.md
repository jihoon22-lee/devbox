# devbox

Windows 11 + WSL에서 여러 프로젝트를 AI 에이전트와 함께 열고·돌리고·지켜보고·기록하는 개인 개발 허브.

- 상태: v1.0.0 재구축 중. S0b–S5는 main에 머지됐고 S6(마무리·출시)은 `v1/s6-release` 브랜치에서 로컬로 구현 중이다. S6 PR·머지와 v1 공개 릴리스는 아직 하지 않았다. 실제 Windows 설치본 수용·성공 기준 측정을 포함한 검증 결과는 [진행 원장](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)을 따른다.
- 이전 소스: [v0.9.0 태그](https://github.com/jihoon22-lee/devbox/tree/v0.9.0). 공개 상태는 GitHub Release에서 확인한다.
- 설계: [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md)
- 진행 원장: [PROGRESS](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)
- 공통 규약: [CONVENTIONS](CONVENTIONS.md)

## 설치와 첫 실행

v1 설치 파일은 공개 릴리스의 `devbox-<버전>-setup.exe` 하나다(아직 공개 전). 사용자 단위 NSIS 설치라 관리자 권한이 필요 없다.

1. systemd가 켜진 WSL2 배포판이 필요하다. WSL이 없거나 WSL1이거나 systemd가 꺼져 있으면 첫 화면이 실행할 명령을 안내하고 [다시 시도]를 보여 준다.
2. 배포판이 여러 개면 "devbox가 쓸 WSL 배포판을 고르세요"에서 하나를 고른다.
3. 앱은 시작할 때마다 내장 Linux 실행 파일을 `~/.local/share/devbox/bin/devbox`에 배치하고 `devbox setup`으로 user unit `devbox@prod.socket`·`devbox@prod.service`·`devbox-tmux@prod.service`를 켠 뒤 데몬 서비스(`devbox@prod.service`)만 다시 시작한다.
4. "devbox 시작하기"에서 Windows 시작 시 실행 여부를 고른다. 트레이에 있는 동안 WSL 연결을 유지한다.

prod 채널 기준 WSL 쪽 데이터는 `~/.local/share/devbox/prod`, 설정은 `~/.config/devbox`, 로그는 `~/.local/state/devbox/prod/logs`에 있다. Windows 앱 데이터는 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox`다.

## v0.9.0에서 옮기기

v0.9.0 데이터는 자동으로 옮기지 않는다. 순서는 **새 앱 수동 설치 → 정상 동작 확인 → v0.9.0 수동 제거**다.

- v1(`io.github.jihoon22lee.devbox`)과 v0.9.0(`com.devbox.v08.*`, 설치 위치 `%LOCALAPPDATA%\DevboxSuite`)은 식별자가 달라 함께 설치될 수 있다. Windows 설정의 설치된 앱에서 버전 0.9.0 항목만 직접 제거하고 1.0.0 항목은 남긴다.
- v0.9.0의 업데이트 기능은 v1 설치 파일 이름을 찾지 않으므로 v0.9.0이 v1로 저절로 바뀌지 않는다.
- v0.9.0 제거가 그 데이터를 어떻게 처리하는지는 v1이 정하지 않는다. 남겨 둘 자료는 제거 전에 직접 백업한다.
- v0.9.0 Knowledge의 활동 기록을 쓰고 있었다면 활동 기록 동의 화면 안내대로 한쪽만 켠다.

## 앱 업데이트

설정 → 정보 → 앱 업데이트에서 한다. 공개 업데이트는 prod 채널의 정식 버전(`x.y.z`)에서만 켜진다. dev·accept 채널과 개발 버전(예: `1.0.0-dev`)에서는 꺼져 있다.

- 확인: 앱이 24시간에 최대 한 번 자동으로 확인하며 [새 버전 확인]으로 직접 확인할 수도 있다. 이 저장소의 최신 정식 Release와 그 안의 `devbox-<버전>-setup.exe` 하나만 받아들인다.
- 다운로드: [업데이트 다운로드]를 눌러야 시작하고 [다운로드 취소]로 멈춘다. 파일은 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\updates` 아래에 받고, 크기와 SHA-256 확인을 마친 뒤에만 "업데이트 파일 준비 완료"로 표시한다. 내려받기만으로는 설치하지 않는다.
- 설치 확인: [업데이트]를 누르면 "업데이트 설치" 창이 설치할 버전과 WSL의 에이전트 작업·서비스 수를 보여 준다. 상태 조회가 끝나고 막는 조건이 없을 때만 [설치하고 다시 시작]이 켜진다. [취소]를 누르면 아무것도 설치하지 않는다.
- 앱에서 실행한 Windows 작업이 진행 중이거나 그 상태를 확인하지 못하면 설치할 수 없다. 완료를 기다리거나 실행 화면에서 직접 중지한 뒤 [다시 조회]한다. 업데이트가 이 작업을 대신 멈추지 않는다.
- 모든 창이 종료를 승인해야 설치한다. 열린 문서의 저장 준비가 끝나지 않으면 설치를 취소한다. 준비 중에는 [설치 준비 취소]로 그만둘 수 있다.
- 앱이 끝난 것을 확인한 뒤 설치 프로그램을 조용히 실행하고 새 앱을 다시 연다. 업데이트 기능은 WSL 쪽 unit·tmux와 에이전트 작업·작업·서비스를 멈추지 않는다. 새 앱도 평소 시작처럼 데몬 서비스만 다시 시작한 뒤 다시 연결한다. 앱이 띄운 Windows 프로세스가 업데이트 뒤에도 유지된다고 약속하지 않는다.
- 실패: 설치나 재실행에 실패하면 다음 실행 때 앱 업데이트에 원인을 표시한다. [새 버전 확인]으로 다시 확인한다. 새 앱이 다시 열리지 않으면 시작 메뉴에서 Devbox를 연다. "업데이트 설치 상태를 확인할 수 없습니다"가 보이면 앱은 자동으로 다시 시도하거나 파일을 지우지 않는다. 설치 프로그램이 끝났는지 확인한 뒤 앱을 다시 시작한다.
- 예전 Windows 사본: `~/.config/devbox/config.local.toml`의 `[windows] root` 아래 예전 `<root>\<프로젝트 이름>` 사본은 다시 쓰지도, 옮기거나 지우지도 않는다. 현재 사본은 `<root>\.devbox-v1\` 아래에 따로 만든다. 예전 사본이 필요 없으면 직접 정리한다.

## 문제 해결

- 연결 화면의 안내(WSL 없음·WSL1·systemd 꺼짐·WSL 쪽 설치 실패·데몬 연결 실패)를 따른 뒤 [다시 시도]한다.
- 설정 → 앱 진단에서 데몬 상태·로그 위치·점검 결과를 본다. [다시 설정]은 내장 WSL 실행 파일을 다시 배치하고 이 채널의 데몬을 다시 시작한다. [WSL 쪽 정리]는 이 채널의 터미널 세션을 끝내고 WSL 서비스와 실행 파일을 지우며 데이터·설정과 다른 채널은 남긴다.
- "Devbox를 시작하지 않았습니다" 안내 후 앱이 열리지 않으면, 이전 업데이트 설치가 끝났는지 확인하지 못한 상태다. 업데이트 설치 프로그램이 실행 중이면 끝날 때까지 기다린다. 실행 중이 아닌데도 계속 이 안내가 나오면 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\updates`의 `install-`로 시작하는 폴더만 지우고 다시 시작한다(accept 채널은 `io.github.jihoon22lee.devbox.accept`). 같은 위치의 다른 파일은 지우지 않는다.
- 진단 ID: 예상하지 못한 오류 화면의 [진단 복사]로 `devbox 진단 ID: …`를 복사한다. 같은 ID가 데몬 로그에 남는다.
- 지원 번들: 앱 진단의 [지원 번들]이 최근 7일 데몬 로그·crash 로그·점검 결과를 로컬 ZIP(`~/.local/share/devbox/prod/support/`)으로 만든다. 알려진 비밀과 홈·노트 경로를 가리고 자동으로 전송하지 않는다. 비밀 저장소가 잠겨 있으면 로그를 넣지 않는다.

## 개발

WSL에서 Node 24·pnpm 9·Rust 1.98.1을 사용한다.

```bash
source ~/.cargo/env
pnpm install --frozen-lockfile
pnpm dev
```

화면은 `http://localhost:1450`에서 연다. `pnpm dev`는 기존 dev 데몬에 연결하거나 이번 실행 소유 데몬을 시작한다.
`pnpm gen`은 RPC 타입을 생성한다. 전체 검사는 모든 변경을 마친 뒤 `pnpm check`로 한 번 실행하며,
E2E 브라우저는 `pnpm --filter app exec playwright install chromium`으로 준비한다.

실제 앱 실행·설치 파일 빌드는 Windows에서 한다. PowerShell의 `scripts/win-dev.ps1`(dev 채널)·`scripts/win-build.ps1`(prod 채널)은
WSL 바이너리를 빌드하고 소유한 Windows 작업 폴더에서 Tauri를 실행한다. 로컬 Windows 도구가 없으면
push한 브랜치의 Windows CI 설치 파일을 `bash scripts/dogfood.sh [브랜치] [--channel accept]`로 받는다. 정확한 원격 커밋·실행·채널·파일 해시를 확인하고
Windows Downloads 아래 새 폴더에 저장하며, 설치는 자동 실행하지 않는다.
성공 기준 SC2·SC6·SC10 측정 절차는 [수용 계측 안내](app/acceptance/README.md)를 따른다.

Claude Code MCP 등록 예시(prod 채널): `claude mcp add devbox -- ~/.local/share/devbox/bin/devbox mcp`.
설정 폴더(`~/.config/devbox`, accept는 `~/.config/devbox-accept`)의 빈 파일 `.config.toml.lock`은 `config.toml` 동시 쓰기 잠금이므로 지우지 말고, 그 폴더를 dotfiles로 Git 관리하면 ignore에 넣는다.

필수 도구·상세 명령·검증 범위는 [앱 개발 안내](app/README.md)를 따른다.
