# devbox

Windows 11 + WSL에서 여러 프로젝트를 AI 에이전트와 함께 열고·돌리고·지켜보고·기록하는 개인 개발 허브.

- 상태: 정식 버전 단계다(첫 정식 버전은 1.0.0). 공개된 버전과 설치 파일은 [GitHub Release](https://github.com/jihoon22-lee/devbox/releases)에서 확인한다. 릴리스 후보(`-rc.N`)는 prerelease로 올리고 앱 업데이트가 꺼져 있으므로, 후보에서 정식 버전으로 올릴 때는 설치 파일을 받아 직접 설치한다. 릴리스와 태그는 사용자의 별도 명시 승인 뒤에만 만든다. 실제 Windows 설치본 수용·성공 기준 측정을 포함한 검증 결과는 [진행 원장](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)을 따른다.
- 이전 소스: [v0.9.0 태그](https://github.com/jihoon22-lee/devbox/tree/v0.9.0). 공개 상태는 GitHub Release에서 확인한다.
- 설계: [01-design](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md)
- 진행 원장: [PROGRESS](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md)
- 사용 안내: [docs/guide.md](docs/guide.md). 문서 전체는 아래 [문서](#문서)에 있다.
- 라이선스: MIT([LICENSE](LICENSE)). 함께 배포하는 서드파티 소프트웨어의 고지는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)에 있다. 두 파일은 설치 폴더에도 들어간다.

## 요구 사항

- Windows 11과 systemd가 켜진 WSL2 배포판.
- WSL의 tmux 3.2 이상. 더 낮으면 환경 점검이 실패로 표시한다. 프로그램이 Shift+Enter를 Enter와 다른 키로 받으려면(pi의 줄바꿈) tmux 3.5 이상이 필요하다. 3.2–3.4에서는 Shift+Enter도 Enter로 동작하므로 pi에서도 줄을 바꾸지 않고 보낸다(Ubuntu 24.04의 tmux는 3.4다). devbox는 자기 tmux 서버(`tmux -L devbox-prod`)에만 확장 키 옵션을 켜고, 사용자의 다른 tmux는 건드리지 않는다. 이 옵션은 WSL 쪽 devbox가 시작할 때 적용하므로, 그 전에 확장 키를 요청한 프로그램은 다시 시작할 때까지 Shift+Enter를 Enter로 받는다. devbox는 터미널을 열 때마다 이미 떠 있는 devbox tmux 서버의 버전을 물어 Shift+Enter를 확장 키로 보낼지 정하므로, tmux를 3.4 이하에서 3.5 이상으로 올려도 이미 떠 있는 devbox tmux 서버가 다시 시작될 때까지는 Shift+Enter가 Enter로 동작한다. tmux 3.7 전 판에는 제어 모드 클라이언트가 연결되는 동안 tmux 서버가 비정상 종료(SIGSEGV)하는 버그가 있다([Ubuntu LP #2169164](https://bugs.launchpad.net/bugs/2169164), upstream `e5a2a25`). devbox는 터미널마다 제어 모드 클라이언트를 붙이므로, 이 버그가 나면 그 tmux 서버의 devbox 터미널·에이전트 작업 세션이 모두 끝난다. 고친 판을 쓴다: tmux 3.7 이상, 또는 Ubuntu 22.04 `3.2a-4ubuntu0.3`·24.04 `3.4-1ubuntu0.2`·26.04 `3.6a-2ubuntu0.2` 이상(2026-10-21 updates, `sudo apt install --only-upgrade tmux`). 이미 떠 있는 tmux 서버는 다시 시작될 때까지 옛 코드로 돈다.
- WSL의 git. PR 만들기에는 WSL의 GitHub CLI(`gh`) 로그인이 필요하다.
- 쓸 에이전트 CLI(`claude`·`codex`·`opencode`·`pi`)를 WSL에 설치하고 각 CLI에서 로그인한다. 환경 점검은 devbox가 연결 형식(hook·플러그인·확장)을 맞춰 본 가장 낮은 버전보다 낮으면 경고한다: Claude Code 2.1.300, opencode 1.19.3, pi 1.3.0. Codex는 기준 버전이 없다. 실제 도구로 에이전트 작업 전 과정을 확인한 것은 아니다([진행 원장](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md) §3 2).

## 설치와 첫 실행

설치 파일은 [GitHub Release](https://github.com/jihoon22-lee/devbox/releases)의 `devbox-<버전>-setup.exe` 하나다. 사용자 단위 NSIS 설치라 관리자 권한이 필요 없다. 받은 폴더의 PowerShell에서 `Get-FileHash .\devbox-<버전>-setup.exe`(기본 SHA256)를 실행해 나온 `Hash`를 Release 자산의 digest(`sha256:` 뒤 값)와 비교한다. `Get-FileHash`는 대문자로, Release는 소문자로 보이므로 대소문자는 무시한다. 설치 파일에는 코드 서명이 없어([ADR 0016](docs/adr/0016-personal-security-scope.md)) 실행할 때 Windows SmartScreen 경고가 날 수 있다. 실제로 어떤 경고와 문구가 보이는지는 아직 확인하지 않았다.

1. systemd가 켜진 WSL2 배포판이 필요하다. WSL이 없거나 WSL1이거나 systemd가 꺼져 있으면 첫 화면이 실행할 명령을 안내하고 [다시 시도]를 보여 준다.
2. 배포판이 여러 개면 "devbox가 쓸 WSL 배포판을 고르세요"에서 하나를 고른다.
3. 앱은 시작할 때마다 내장 Linux 실행 파일을 `~/.local/share/devbox/bin/devbox`에 배치하고 `devbox setup`으로 user unit `devbox@prod.socket`·`devbox@prod.service`·`devbox-tmux@prod.service`를 켠다. 데몬 서비스(`devbox@prod.service`)는 unit·환경 파일·`tmux.conf`가 바뀌었거나, 실행 중이 아니거나, 지금 배치한 실행 파일로 실행 중이 아닐 때만 다시 시작한다. 연결 화면의 [다시 시도], 앱이 열려 있는 동안 연결이 5번 실패하면 상단 배너에 나오는 [다시 시도], 설정 → 앱 진단의 [다시 설정]은 바뀐 것이 없어도 데몬 서비스를 다시 시작한다. 연결 화면에서 배포판을 고르는 것은 다시 시작하지 않는다.
4. "devbox 시작하기"에서 Windows 시작 시 실행 여부를 고른다. 트레이에 있는 동안 WSL 연결을 유지한다.

prod 채널 기준 WSL 쪽 데이터는 `~/.local/share/devbox/prod`, 설정은 `~/.config/devbox`, 로그는 `~/.local/state/devbox/prod/logs`에 있다. Windows 앱 데이터는 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox`다.

## 에이전트 도구

기본으로 쓸 수 있는 도구는 4종이다. devbox는 에이전트 작업을 시작할 때마다 hook·플러그인·확장을 명령 인자나 환경 변수로 붙인다. opencode 플러그인과 pi 확장은 사용자가 이미 쓰는 것을 대신하지 않고 함께 읽히도록 붙인다. 실제 도구에서 함께 동작하는지는 아직 확인하지 않았다. 붙인 hook·플러그인·확장은 devbox 에이전트 작업 밖에서는 아무것도 하지 않는다. 다른 도구를 더하거나 명령을 바꾸려면 [설정 파일 안내](docs/config.md)의 `[[agents.profiles]]`를 쓴다.

| 도구 | 상태를 아는 방법 | 재개 | 토큰 합계 |
|---|---|---|---|
| Claude Code | 작업마다 붙이는 hook(`--settings`). 승인 대기는 도구 이름과 입력으로 짝지어, 같은 호출의 완료·실패·거절이 와야 끝난다. hook이 알리지 않는 폴더 신뢰 대화와 입력창의 셸 모드(`!`로 시작하는 초안)는 터미널 화면으로 알아본다 | `claude --resume <Claude 세션 id>` | `~/.claude/projects`의 그 세션 기록 |
| Codex | 작업마다 붙이는 `notify`(`-c`, 턴 완료 후보)와 `~/.codex/sessions`의 rollout 기록. 승인·질문 화면, Codex가 스스로 여는 선택 목록, 폴더 신뢰·hook 검토 화면, 그리고 Enter가 보낸 글을 Codex에게 주는 대신 그 화면에서 처리하는 입력 상자(목표 편집·이름 바꾸기 등)·설정·선택 목록과 입력창의 셸 모드는 터미널 마지막 줄로, 화면 가운데에 띄우는 확인 상자와 화면을 덮는 보기(대화 기록 보기 등)는 화면 전체로 알아본다. 이런 화면이 보이는 동안에는 보내지 않는다 | `codex resume <Codex 세션 id>` | rollout 기록 |
| opencode | 작업마다 붙이는 devbox 플러그인(`OPENCODE_CONFIG_CONTENT`). 권한 요청과 질문도 알린다. 입력창의 셸 모드는 터미널 화면으로 알아본다 | `opencode -s <opencode 세션 id>` | `opencode export`. 하위 에이전트 토큰은 빠진다 |
| pi | 작업마다 붙이는 devbox 확장(`-e`). 확장이 준비됐다고 알릴 때까지 보내기를 미루고, 알림이 없으면 도구 시작 30초 뒤부터 보낸다. 폴더 신뢰 질문도 알아본다. 입력창의 셸 모드(`!`)는 알아보지 못한다 | `pi --session-id <pi 세션 id>` | pi 세션 파일(기본 `~/.pi/agent/sessions` 아래). pi 설정의 `sessionDir`은 읽지 않는다 |

- 토큰 합계는 입력·출력·캐시 읽기·캐시 쓰기 토큰이다. 비용은 어느 도구도 더하지 않는다.
- 재개할 때 도구의 세션 id를 모르면 pi는 지시문 없이 새 pi 세션을 연다. Claude Code·Codex·opencode는 처음 지시문으로 새로 시작한다.
- 에이전트 작업은 로그인 셸 환경 가운데 허용 목록(`PATH`·`LANG`·`LC_*`·`NVM_DIR`·`CARGO_HOME`·`PNPM_HOME`·`TZ` 등)만 받는다. 셸에서 설정한 `ANTHROPIC_API_KEY`·`OPENAI_API_KEY` 같은 API 키와 `CODEX_HOME`·`CLAUDE_CONFIG_DIR`·`OPENCODE_CONFIG_DIR`·`PI_CODING_AGENT_DIR` 같은 설정 위치 변수는 넘기지 않으므로, 도구는 각 CLI의 로그인과 기본 설정 위치를 쓴다. `HOME`과 `XDG_CONFIG_HOME` 같은 XDG 폴더 변수는 devbox 서비스 설정이 정한 값이다. 앱이 WSL 쪽을 설정할 때는 셸 설정 파일을 읽지 않으므로 보통 기본 위치(`~/.config` 등)다.
- rc.1에서 시작한 Claude Code 작업은 그 뒤 판(rc.2나 1.0.0)을 설치한 뒤에도 rc.1의 hook 설정을 쓴다. 이 설정에는 PostToolUse·PostToolUseFailure·PermissionDenied hook이 없어, 터미널에서 승인한 뒤에도 그 턴이 끝날 때까지 보내기가 "에이전트가 선택을 기다리고 있습니다."로 막힌다. 작업을 재개하면 새 hook 설정으로 다시 시작한다.

### 도구 경로 찾기

- 에이전트 작업을 시작할 때 도구 명령의 첫 단어를 devbox 터미널과 같은 대화형 로그인 셸(`bash -lic`)에 먼저 묻고, 셸이 찾지 못하면 로그인 셸 PATH(설정 때 기록한 환경 파일)에서 찾는다. 대화형 셸의 출력은 경로 한 줄만 쓰고 기록하지 않는다.
- 둘 다 찾지 못하면 작업 폴더를 만들기 전에 "<명령> 명령을 찾을 수 없습니다."로 끝난다. 설치했다면 설정 → 앱 진단의 [셸 환경 다시 읽기]를 누른다.
- 셸이 답한 결과만 5분 동안 다시 쓴다. 셸이 5초 안에 답하지 않으면 그 작업은 로그인 셸 PATH에서 찾은 도구로 시작하고, 다음 작업을 시작할 때 셸에 다시 묻는다.
- 설정 → 앱 진단의 환경 점검은 매번 셸에 다시 묻고, 셸이 답한 결과를 다음 작업 시작에 쓴다. 셸이 답하지 않으면 그 줄에는 로그인 셸 PATH에서 찾은 도구가 보인다.
- 도구가 공유 폴더(`/usr/local/sbin`·`/usr/local/bin`·`/usr/sbin`·`/usr/bin`·`/sbin`·`/bin`·`/snap/bin`·`/home/linuxbrew/.linuxbrew/bin`)에 있으면 절대 경로로 실행하고 그 폴더를 PATH 끝에 붙인다. 다른 폴더에 있으면 그 폴더를 PATH 앞에 두고 이름으로 실행한다. 환경 점검의 버전 확인도 같은 규칙을 따른다.

## MCP

`devbox mcp`는 에이전트 도구가 프로젝트·에이전트 작업·실행·로그·문제·노트를 조회하는 MCP stdio 서버다. 쓰기 도구(`devbox_agent_send`·`devbox_run_task`·`devbox_note_add`)는 `config.toml`의 `[mcp] write = true`일 때만 켜진다. `devbox mcp`는 시작할 때 설정을 한 번 읽으므로, 바꾼 뒤에는 MCP를 쓰는 도구를 다시 시작한다.

아래 등록 명령은 각 도구의 사용자 설정을 고친다. 괄호 안은 이 문서가 문법을 확인한 도구 버전이다. 설치본으로는 아직 확인하지 않았다.

- Claude Code(2.1.308, `claude mcp add --help`): `claude mcp add -s user devbox -- ~/.local/share/devbox/bin/devbox mcp`
- Codex(0.164.0·0.165.0, `codex mcp add --help`): `codex mcp add devbox -- ~/.local/share/devbox/bin/devbox mcp`
- opencode(1.19.3): `opencode mcp add`에는 로컬 명령을 넘기는 옵션이 없다. `~/.config/opencode/opencode.json`에 `mcp` 키를 더한다. `<사용자>`는 WSL 사용자 이름이다.

  ```json
  {
    "mcp": {
      "devbox": {
        "type": "local",
        "command": ["/home/<사용자>/.local/share/devbox/bin/devbox", "mcp"],
        "enabled": true
      }
    }
  }
  ```

- pi(1.3.0, `docs/mcp.md`): `pi mcp add devbox -- ~/.local/share/devbox/bin/devbox mcp`. 사용자 수준(`~/.pi/agent/mcp.json`)에 등록된다. 이미 열린 pi에서는 `/reload`를 실행한다. `pi mcp list`로 연결을 확인할 수 있다.

accept 채널의 실행 파일은 `~/.local/share/devbox-accept/bin/devbox`이고 채널 인자가 필요하다: `~/.local/share/devbox-accept/bin/devbox --instance accept mcp`. 인자도 `DEVBOX_INSTANCE`도 없으면 prod 채널에 연결한다.

## 백업과 다른 PC로 옮기기

- 비밀: 설정 → 비밀의 [복구 문자열 보기]로 복구 문자열을 안전한 곳에 보관한다. 비밀 값은 이 PC의 `devbox.db`에 암호화해 두고, 그 키는 Windows DPAPI(현재 사용자)로 봉인한다. 키를 풀 수 없으면 [복구 문자열 입력]으로 되살리거나 비밀 값을 다시 입력한다. 복구 문자열은 그 키를 쓰는 `devbox.db`에서만 받아들이고(키 id 비교) 다른 `devbox.db`에서는 거절한다. 그래서 새 PC에서는 비밀 이름이 `config.toml`로 따라와도 값은 다시 입력한다.
- 설정: `config.toml`을 여러 PC에서 함께 쓰려면 파일을 복사하거나 `~/.config/devbox` 폴더를 Git으로 관리한다. 심볼릭 링크는 쓰지 않는다. `config.toml`이 링크면 앱에서 비밀 값과 활동 기록 규칙을 저장하지 못하고, `~/.config/devbox`나 그 위 폴더가 링크면 WSL 쪽 설정이 실패한다([설정 파일 안내](docs/config.md)). 폴더를 Git으로 관리하면 PC별 설정 `config.local.toml`, 동시 쓰기 잠금 `.config.toml.lock`(지우지 않는다), devbox가 만드는 `prod/` 같은 하위 폴더를 ignore에 넣는다.
- 노트: vault(기본 `~/notes`)는 Git으로 관리한다.
- `~/.local/share/devbox/prod/devbox.db`(비밀 값·활동 기록 등)는 PC별 데이터다.

## 앱 업데이트

설정 → 정보 → 앱 업데이트에서 한다. 공개 업데이트는 prod 채널의 정식 버전(`x.y.z`)에서만 켜진다. dev·accept 채널, 개발 버전(예: `1.0.0-dev`)과 릴리스 후보(예: `1.0.0-rc.2`)에서는 꺼져 있다.

- 확인: 앱이 24시간에 최대 한 번 자동으로 확인하며 [새 버전 확인]으로 직접 확인할 수도 있다. 이 저장소의 최신 정식 Release와 그 안의 `devbox-<버전>-setup.exe` 하나만 받아들인다.
- 다운로드: [업데이트 다운로드]를 눌러야 시작하고 [다운로드 취소]로 멈춘다. 파일은 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\updates` 아래에 받고, 크기와 SHA-256 확인을 마친 뒤에만 "업데이트 파일 준비 완료"로 표시한다. 내려받기만으로는 설치하지 않는다.
- 설치 확인: [업데이트]를 누르면 "업데이트 설치" 창이 설치할 버전과 WSL의 에이전트 작업·서비스 수를 보여 준다. 상태 조회가 끝나고 막는 조건이 없을 때만 [설치하고 다시 시작]이 켜진다. [취소]를 누르면 아무것도 설치하지 않는다.
- 앱에서 실행한 Windows 작업이 진행 중이거나 그 상태를 확인하지 못하면 설치할 수 없다. 완료를 기다리거나 실행 화면에서 직접 중지한 뒤 [다시 조회]한다. 업데이트가 이 작업을 대신 멈추지 않는다.
- 모든 창이 종료를 승인해야 설치한다. 열린 문서의 저장 준비가 끝나지 않으면 설치를 취소한다. 준비 중에는 [설치 준비 취소]로 그만둘 수 있다.
- 앱이 끝난 것을 확인한 뒤 설치 프로그램을 조용히 실행하고 새 앱을 다시 연다. 업데이트 기능은 WSL 쪽 unit·tmux와 에이전트 작업·작업·서비스를 멈추지 않는다. 새 앱은 실행 파일이 바뀌었으므로 평소 시작처럼 데몬 서비스만 다시 시작한 뒤 다시 연결한다. 앱이 띄운 Windows 프로세스가 업데이트 뒤에도 유지된다고 약속하지 않는다.
- 실패: 설치나 재실행에 실패하면 다음 실행 때 앱 업데이트에 원인을 표시한다. [새 버전 확인]으로 다시 확인한다. 새 앱이 다시 열리지 않으면 시작 메뉴에서 Devbox를 연다. "업데이트 설치 상태를 확인할 수 없습니다"가 보이면 앱은 자동으로 다시 시도하거나 파일을 지우지 않는다. 설치 프로그램이 끝났는지 확인한 뒤 앱을 다시 시작한다.
- 예전 Windows 사본: `~/.config/devbox/config.local.toml`의 `[windows] root` 아래 예전 `<root>\<프로젝트 이름>` 사본은 다시 쓰지도, 옮기거나 지우지도 않는다. 현재 사본은 `<root>\.devbox-v1\` 아래에 따로 만든다. 예전 사본이 필요 없으면 직접 정리한다.

## v0.9.0에서 옮기기

v0.9.0 데이터는 자동으로 옮기지 않는다. 순서는 **새 앱 수동 설치 → 정상 동작 확인 → v0.9.0 수동 제거**다.

- v1(`io.github.jihoon22lee.devbox`)과 v0.9.0(`com.devbox.v08.*`, 설치 위치 `%LOCALAPPDATA%\DevboxSuite`)은 식별자가 달라 함께 설치될 수 있다. Windows 설정의 설치된 앱에서 버전 0.9.0 항목만 직접 제거하고 1.0.0 항목은 남긴다.
- v0.9.0의 업데이트 기능은 v1 설치 파일 이름을 찾지 않으므로 v0.9.0이 v1로 저절로 바뀌지 않는다.
- v0.9.0 제거가 그 데이터를 어떻게 처리하는지는 v1이 정하지 않는다. 남겨 둘 자료는 제거 전에 직접 백업한다.
- v0.9.0 Knowledge의 활동 기록을 쓰고 있었다면 활동 기록 동의 화면 안내대로 한쪽만 켠다.

## 제거

1. 앱의 설정 → 앱 진단에서 [WSL 쪽 정리]를 누른다. 이 채널의 터미널 세션을 끝내고 WSL 서비스와 실행 파일을 지운다. 데이터·설정과 다른 채널은 남는다.
2. Windows 설정의 설치된 앱에서 Devbox를 제거한다. accept 채널을 조용히 제거한 수용 시험에서는 레지스트리 값 `HKCU\Software\github\Devbox Accept`(설치 폴더 기억)가 남았다. 그 밖에 Windows에 남는 항목은 아직 확인하지 않았다.
3. WSL에는 devbox가 쓰는 설정(`~/.config/devbox`), 데이터(`~/.local/share/devbox/prod`), 로그(`~/.local/state/devbox/prod/logs`) 폴더가 남는다. 필요 없으면 직접 지운다. 노트 vault(기본 `~/notes`)는 devbox 폴더가 아니라 사용자의 노트 파일이므로 제거와 상관없이 그대로 남는다.

## 알려진 제한

- 첫 시작이 느리다. 설치 직후 첫 시작(WSL 실행 중)에서 첫 데이터가 보이기까지 9036ms·6093ms(accept 설치본 측정) 걸려 목표 3초를 넘었다. 반복 실행은 아직 재지 않았다.
- 필수 PR CI의 Rust (Windows) job이 27m24s(#648)·17m47s(#649) 걸려 목표 15분을 넘었다.
- Windows 작업은 프로젝트의 Windows 사본에서 실행된다. 그 실행이 남긴 프로세스가 도는 동안 사본을 바꾸지 않도록 Windows 실행마다 숨은 powershell.exe 감시 프로세스가 하나 더 뜬다. 실행을 시작한 프로세스가 끝난 뒤 감시 프로세스만 강제로 끝나고 남은 프로세스가 계속 돌면 보호가 일찍 풀린다. 원본 저장소는 바뀌지 않는다. 실제 Windows에서는 아직 확인하지 않았다(진행 원장 §3 I1).
- Windows 실행이 남긴 프로세스가 도는 동안은 실행이 끝났어도 Windows 사본을 쓰는 중으로 본다. 빌드 서버(dotnet build의 VBCSCompiler·MSBuild, Gradle 등)처럼 빌드가 끝난 뒤에도 남는 프로세스도 그렇다. 그동안 바뀐 내용으로 하는 다음 실행은 시작하지 않으므로(`runtime.windows_copy_busy`), 그 프로세스가 스스로 끝나기를 기다리거나, Windows 터미널에서 `dotnet build-server shutdown`·`gradle --stop`(Gradle Wrapper 프로젝트는 `.\gradlew --stop`)을 실행하거나 작업 관리자에서 끝낸 뒤 다시 실행한다. WSL에서 실행한 `dotnet`·`gradle`은 Linux 쪽 빌드 서버만 끝낸다. 모두 끝났는데도 1분 넘게 계속되면 Windows 앱 연결을 확인한다.
- 앱 자동 시작 때 설정이 파일을 쓴 뒤 다시 시작하기 전에 끊기면 다음 자동 시작도 다시 시작하지 않는다. [다시 설정]으로 복구한다.
- Codex는 rollout 기록의 끝 8 MiB만 읽는다. 한 턴의 기록이 8 MiB를 넘으면 그 턴의 시작이 읽는 범위 밖에 있어 다음 턴까지 추적하지 못한다.
- 확인 상태: 실행하지 않은 확인은 PASS로 적지 않으며, 원본 결과와 근거는 [진행 원장](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md) §3·§4에 있다(줄이기 전 전문은 [docs/history.md](docs/history.md)). 위 첫 두 항목(첫 시작 시간, PR CI 시간)은 목표를 넘어 미달이다. Windows 실기 확인은 대부분 실행하지 않았다(NOT_RUN). 전체 목록과 결과는 진행 원장 §3·§4와 [계획의 S7-14 표](docs/superpowers/plans/2026-10-08-devbox-v1/10-s7-s8-release.md#s7-14-windows-후보accept-수용)에 있고, 주요 분류는 다음과 같다: Windows 사본 보호(위 알려진 제한), rc.1에서 rc.2로 올린 뒤의 동작, 도구 화면 알아보기(승인·폴더 신뢰·셸 모드·Shift+Enter, 실제 Claude Code·Codex·opencode·pi 화면과 연결), 연결 배너·재시작·연결 유지(Windows 실제 동작)와 [나중에]·깨진 설정 파일 안내, 에이전트 작업의 작업 폴더 정리와 폐기, 서비스·노트·배포판 선택, 설치 폴더의 고지 파일, 설치 경고 문구, 제거 뒤 남는 항목, MCP 등록 명령의 설치본 확인. 지난 수용에서는 전역 단축키로 명령 팔레트를 여는 시간(300ms 목표)과 설치본 지원 번들의 가림(시도했으나 판정하지 못함), accept 업데이트 화면을 확인하지 않았다. rc.1·rc.2의 Windows 설치·실행 수용과 릴리스 후보에서 1.0.0으로 올리는 수동 설치는 확인하지 않았다. accept 조용한 제거 직후의 제거 등록 확인은 FAIL이다. WSL 유휴 유지(30분)는 사용자 확인으로 남아 있고, `wsl --shutdown` 뒤 복귀는 WSL 재시작을 요청하지 않기로 해 확인하지 않았다.

## 문제 해결

- 연결 화면의 안내(WSL 없음·WSL1·systemd 꺼짐·WSL 쪽 설치 실패·데몬 연결 실패)를 따른 뒤 [다시 시도]한다. 앱이 열린 뒤 연결이 계속 끊기면 상단 배너의 [다시 시도]를 누르거나 설정 → 앱 진단의 [다시 설정]을 쓴다. "앱 설정 파일을 읽지 못했습니다"가 보이면 Windows 앱 설정 파일(`settings.json`)이 깨진 것이다. 앱은 그 파일을 옮기거나 지우지 않으므로, 화면이 알려 주는 경로의 파일을 지우거나 이름을 바꾼 뒤 [다시 시도]하면 기본 설정으로 시작한다([설정 파일 안내](docs/config.md)).
- 설정 파일에 오류가 있으면 앱의 경고 배너가 파일과 영역을 알려 준다. 고친 뒤 [다시 설정]을 누른다([설정 파일 안내](docs/config.md)).
- 설정 → 앱 진단에서 데몬 상태·로그 위치·점검 결과를 본다. [다시 설정]은 내장 WSL 실행 파일을 다시 배치하고 이 채널의 데몬을 다시 시작한다. [WSL 쪽 정리]는 이 채널의 터미널 세션을 끝내고 WSL 서비스와 실행 파일을 지우며 데이터·설정과 다른 채널은 남긴다.
- "Devbox를 시작하지 않았습니다" 안내 후 앱이 열리지 않으면, 이전 업데이트 설치가 끝났는지 확인하지 못한 상태다. 업데이트 설치 프로그램이 실행 중이면 끝날 때까지 기다린다. 실행 중이 아닌데도 계속 이 안내가 나오면 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\updates`의 `install-`로 시작하는 폴더만 지우고 다시 시작한다(accept 채널은 `io.github.jihoon22lee.devbox.accept`). 같은 위치의 다른 파일은 지우지 않는다.
- devbox 터미널과 에이전트 작업 세션이 한꺼번에 끝났고 WSL이 꺼진 경우가 아니라면 WSL의 tmux 서버가 비정상 종료했을 수 있다. tmux 3.7 전 판에는 알려진 버그가 있으니 [요구 사항](#요구-사항)의 고친 판을 쓴다.
- 진단 ID: 예상하지 못한 오류 화면의 [진단 복사]로 `devbox 진단 ID: …`를 복사한다. 같은 ID가 데몬 로그에 남는다.
- 지원 번들: 앱 진단의 [지원 번들]이 최근 7일 데몬 로그·crash 로그·점검 결과를 로컬 ZIP(`~/.local/share/devbox/prod/support/`)으로 만든다. 알려진 비밀과 홈·노트 경로를 가리고 자동으로 전송하지 않는다. 비밀 저장소가 잠겨 있으면 로그를 넣지 않는다.

## 문서

- [사용 안내](docs/guide.md): 화면별 흐름, 단축키, 명령줄.
- [설정 파일 안내](docs/config.md): 설정 파일 네 개의 위치와 키.
- [SECURITY](SECURITY.md): 보안 범위와 취약점 신고.
- [끝난 계획과 이전 기록](docs/history.md): 줄이기 전 진행 원장, v1 계획 문서, v0.9 기록.
- [CONVENTIONS](CONVENTIONS.md): 개발 규약.

## 개발

WSL에서 Node 24·pnpm 9·Rust 1.98.1을 사용한다.
저장소 루트에서 `pnpm dev`를 실행하고 `http://localhost:1450`에서 화면을 연다.
필수 도구·상세 명령·검증 범위는 [앱 개발 안내](app/README.md)를 따른다.
