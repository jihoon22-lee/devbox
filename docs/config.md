# 설정 파일

devbox 설정은 아래 네 파일에 있다. 경로는 prod 채널 기준이다. accept 채널은 `~/.config/devbox-accept`와 `%LOCALAPPDATA%\io.github.jihoon22lee.devbox.accept`를 쓴다.

| 파일 | 위치 | 담는 것 |
|---|---|---|
| `config.toml` | `~/.config/devbox/config.toml` | 여러 PC가 함께 쓸 설정 |
| `config.local.toml` | `~/.config/devbox/config.local.toml` | 이 PC에만 둘 설정. `[windows]`는 이 파일에만 둔다 |
| `.devbox/devbox.toml` | 각 프로젝트 저장소 | 작업·서비스·예약, 에이전트 작업의 준비·테스트·정리 명령, 시작 구성 |
| `settings.json` | `%LOCALAPPDATA%\io.github.jihoon22lee.devbox\settings.json` | Windows 앱 설정: 배포판, 전역 단축키, 활동 기록 동의 |

아래 `toml` 예시의 첫 줄은 그 예시가 들어갈 파일 이름이다. 저장소의 `crates/cli/tests/docs_config_examples.rs`가 이 문서의 `toml` 예시를 모두 devbox가 쓰는 읽기 함수로 읽어 문제가 없는지 확인한다.

## 읽는 때

- WSL 쪽 devbox는 시작할 때 `config.toml`과 `config.local.toml`을 읽고, 다시 시작할 때까지 그 값을 쓴다. 파일을 직접 고쳤으면 설정 → 앱 진단의 [다시 설정]으로 다시 시작해야 반영된다. `[secrets] names`만은 비밀 이름 목록이 필요할 때마다(설정 → 비밀, 지원 번들 등) 두 파일에서 다시 읽는다.
- `devbox mcp`(MCP 서버)는 자기가 시작할 때 한 번 읽는다. `[mcp]`를 바꿨으면 MCP를 쓰는 에이전트 도구를 다시 시작한다.
- 앱에서 바꾼 활동 기록 규칙과 비밀 이름은 `config.toml`에 저장되고 바로 적용된다. 이때 그 영역의 키만 고치고 다른 영역과 주석은 그대로 둔다. 같은 폴더의 임시 파일(권한 0600)에 쓴 뒤 바꿔 넣으므로 하드 링크는 끊어진다. `config.toml`이 TOML로 읽히지 않거나 일반 파일이 아니면(심볼릭 링크 포함) 저장을 거절한다(`activity.config_unavailable`, `secrets.config_invalid`). 비밀은 값을 저장할 때마다 이름을 먼저 `config.toml`에 쓰므로, 이때는 이름이 이미 목록에 있어도 비밀 값을 저장하지 못한다.
- `config.toml`은 실제 폴더 안의 일반 파일로 둔다. `~/.config/devbox`나 그 위 폴더가 심볼릭 링크면 WSL 쪽 설정(`devbox setup`)이 실패한다. dotfiles로 관리하려면 파일을 복사하거나 `~/.config/devbox` 폴더를 Git으로 관리한다([README](../README.md)의 "백업과 다른 PC로 옮기기").
- 설정 폴더의 빈 파일 `.config.toml.lock`은 `config.toml` 동시 쓰기를 막는 잠금이다. 지우지 않는다.

## 병합 규칙

1. `config.toml`을 먼저 읽고 `config.local.toml`을 그 위에 합친다. 표(`[영역]`)는 키별로 합치고, 그 밖의 값과 배열은 `config.local.toml`의 값이 통째로 바꾼다. 예를 들어 `config.local.toml`에 `[[agents.profiles]]`가 하나라도 있으면 `config.toml`의 프로필 목록은 쓰지 않는다.
2. 어느 파일이든 읽을 수 없거나 TOML 문법이 틀리면 모든 영역을 아래 실패 정책으로 처리한다.
3. 파일마다 영역을 따로 확인한다. 어느 파일에서든 망가진 영역은 실패 정책을 따르고, 다른 파일에 있는 같은 영역으로 대신하지 않는다. `[notes]`·`[search]`·`[activity]`도 같다. 예를 들어 `config.toml`의 `[notes] vault`가 잘못됐으면 `config.local.toml`에 올바른 `vault`가 있어도 노트를 끈다. rc.1은 두 파일을 합친 값만 확인해서 이 경우에도 노트를 썼다.
4. 모르는 영역과 `config.toml`의 `[windows]`는 문제로 알리고 무시한다.
5. `[[agents.profiles]]`는 항목마다 확인한다. 잘못된 항목과 앞 항목과 `id`가 겹치는 항목만 건너뛴다. 같은 `id`는 처음 나온 항목이 가지며, 그 항목이 잘못됐어도 마찬가지다. 기본 도구 4종은 그대로 쓸 수 있다.

문제가 있으면 앱의 경고 배너가 파일·영역·내용을 보이고 [다시 설정]과 [경로 복사]를 준다. 파일을 고친 뒤 [다시 설정]을 누른다.

### 오류 동작

| 영역 | 읽기 실패 시 |
|---|---|
| 파일 전체 문법 오류 | 아래 모든 영역을 실패로 처리 |
| `[notes]` | 노트를 끈다. 기본 vault로 대체하지 않는다 |
| `[search]` | 검색을 끈다 |
| `[windows]`(config.local.toml) | Windows 사본을 쓰는 실행을 `windows.config_invalid`로 시작하지 않는다 |
| `[[agents.profiles]]` | 잘못된 항목·중복 id만 건너뛴다 |
| `[mcp]` | `write = false` |
| `[activity]` | 창 제목을 남기지 않는다. `excluded_processes`도 쓰지 않으므로 모든 앱의 이름과 시간은 기록한다 |
| 그 밖의 영역 | 기본값 |

## config.toml

모든 영역은 정해진 키만 받는다. 모르는 키가 있으면 그 영역을 읽지 못한 것으로 본다. `[notes]`와 `[search]`는 값이 아래 규칙에 맞지 않을 때도 그렇다.

| 영역 | 키 | 기본값 | 뜻 |
|---|---|---|---|
| `[projects]` | `roots` | `["~/projects"]` | 프로젝트 후보를 찾을 폴더. 각 폴더 바로 아래 폴더를 후보로 보인다(`.`으로 시작하거나 `-agents`로 끝나는 폴더 제외) |
| `[pc]` | `name` | 호스트 이름 | 이 PC 이름. 일일 기록의 "오늘 한 일" 소제목에 쓴다 |
| `[terminal]` | `profiles` | 없음 | `[[terminal.profiles]]`의 `name`·`command`. 기본 셸(`bash -l`) 뒤에 터미널 목록으로 붙는다 |
| `[agents]` | `cleanup_after_close` | `true` | 병합하거나 PR을 만들어 닫은 에이전트 작업의 작업 폴더 정리 명령을 실행하고 작업 폴더와 브랜치를 지운다. 바뀐 파일이 남은 작업 폴더는 지우지 않는다 |
| `[agents]` | `profiles` | 없음 | 에이전트 도구 프로필. 아래 "에이전트 도구 프로필" |
| `[mcp]` | `write` | `false` | MCP 쓰기 도구를 켠다. 아래 "MCP" |
| `[secrets]` | `names` | 없음 | 비밀 이름 목록. 값은 이 PC의 `devbox.db`에 암호화해 저장한다. 설정 → 비밀에서 고친다 |
| `[notes]` | `vault` | `"~/notes"` | 노트 vault 폴더. 절대 경로이거나 `~/`로 시작해야 하고 `..`를 쓸 수 없다 |
| `[notes]` | `daily_folder`·`inbox`·`templates_folder` | `"daily"`·`"inbox.md"`·`"templates"` | vault 안의 상대 경로. `/`로 나누며 `\`·`:`·빈 부분·`.`·`..`·`.`으로 시작하는 부분은 쓸 수 없다. `inbox`는 `.md`로 끝나야 한다 |
| `[search]` | `exclude` | 없음 | 코드·파일 이름 검색에서 뺄 gitignore 형식 glob. 64개까지, 하나에 1–256바이트 |
| `[activity]` | 아래 "활동 기록" | | |

```toml
# config.toml
[projects]
roots = ["~/projects", "~/work"]

[pc]
name = "desk"

[[terminal.profiles]]
name = "로그인 셸"
command = "bash -l"

[agents]
cleanup_after_close = true

[notes]
vault = "~/notes"
daily_folder = "daily"
inbox = "inbox.md"
templates_folder = "templates"

[search]
exclude = ["**/dist/**", "**/node_modules/**"]
```

### 에이전트 도구 프로필

기본 도구 4종(`claude`·`codex`·`opencode`·`pi`)은 설정 없이 쓸 수 있다. `[[agents.profiles]]`로 도구를 더하거나 기본 도구를 바꾼다.

| 키 | 필수 | 뜻 |
|---|---|---|
| `id` | 예 | 프로필 id |
| `label` | 예 | 화면에 보이는 이름 |
| `command` | 예 | 실행할 명령. 첫 단어가 이름이면 도구 경로 찾기로 찾고, 절대 경로면 그 파일만 확인한다. 나머지는 적힌 대로 붙인다. 첫 단어가 상대 경로이거나 셸 문법(`VAR=값`·따옴표·`$`·셸 내장 명령 등)이면 찾지 않고 적힌 대로 실행한다 |
| `hooks` | 예 | devbox가 작업 실행마다 붙이는 연결. 아래 표 |
| `waitingPatterns` | 아니요 | 입력 대기 화면을 알아볼 Rust 정규식 목록. `waiting_patterns`로 써도 된다 |

| `hooks` | 실행할 때 붙이는 것 | 지시문 | 재개 |
|---|---|---|---|
| `claude` | `--settings <devbox hook 설정 파일>` | `-- "<지시문>"` | `--resume <id>` |
| `codex` | `-c notify=[…devbox agent event --from codex]` | `-- "<지시문>"` | `resume <id>` |
| `opencode` | `OPENCODE_CONFIG_CONTENT`로 devbox 플러그인 | `--prompt="<지시문>"` | `-s <id>` |
| `pi` | `-e <devbox 확장>` | `--session-id <id> -- "<지시문>"` | `--session-id <id>` |
| `none` | 없음 | 인자로 넘기지 않는다 | 없음 |

- 실행 환경에는 `DEVBOX_PROMPT_FILE`(지시문 파일 경로)·`DEVBOX_AGENT_ID`·`DEVBOX_INSTANCE` 등이 들어간다. `hooks = "none"`인 도구는 지시문을 이 파일에서 읽을 수 있다.
- `waitingPatterns`는 터미널 마지막 15줄 가운데 비어 있지 않은 마지막 6줄을 줄바꿈으로 이어 맞춰 본다. 맞은 화면이 2초 넘게 그대로면 "입력 대기"로 표시한다. 화면이나 MCP에서 보내기를 할 때도 먼저 확인하고, 맞으면 보내지 않는다.
- 기본 도구와 같은 `id`를 쓰면 그 도구의 `label`·`command`·`hooks`는 이 프로필 값을 쓴다.
- `id`가 `pi`가 아니고 `hooks = "pi"`인 프로필도 pi 확장이 준비됐다고 알릴 때까지 보내기를 미룬다(도구 시작 뒤 최대 30초). 기본 pi 프로필의 폴더 신뢰 화면 패턴은 따라오지 않는다.

아래 예시는 여러 경우를 한데 모은 것이다. 필요한 프로필만 옮겨 적는다.

```toml
# config.toml
[[agents.profiles]]
id = "my-agent"
label = "내 에이전트"
command = "my-agent --verbose"
hooks = "none"
waitingPatterns = ['(?m)^Continue\? \[y/N\]\s*$']

[[agents.profiles]]
id = "claude-work"
label = "Claude (작업용)"
command = "claude"
hooks = "claude"

[[agents.profiles]]
id = "codex-local"
label = "Codex (직접 설치)"
command = "/opt/codex/bin/codex"
hooks = "codex"

[[agents.profiles]]
id = "opencode-alt"
label = "opencode"
command = "opencode"
hooks = "opencode"

[[agents.profiles]]
id = "pi-alt"
label = "pi"
command = "pi"
hooks = "pi"
# 기본 pi 프로필의 폴더 신뢰 화면 패턴을 그대로 옮긴 것
waiting_patterns = ['(?im)^\s*(?:→\s*)?Do not trust\s*\n\s*(?:→\s*)?Do not trust \(this session only\)\s*$']
```

### MCP

`devbox mcp`의 읽기 도구(`devbox_projects`·`devbox_agents`·`devbox_runs`·`devbox_logs`·`devbox_problems`·`devbox_notes_search`·`devbox_note_read`)는 늘 켜져 있다. 쓰기 도구(`devbox_agent_send`·`devbox_run_task`·`devbox_note_add`)는 `[mcp] write = true`일 때만 목록에 나오고 실행된다. 이 값 말고 쓰기 도구를 켜는 방법은 없다.

```toml
# config.toml
[mcp]
write = true
```

`config.local.toml`에 `write = false`를 두면 이 PC에서만 끈다.

### 활동 기록

`[activity]`는 활동 기록에 남길 앱 이름·창 제목의 개인정보 규칙이다. 설정 → 활동에서 고쳐도 같은 곳에 저장된다.

| 키 | 기본값 | 뜻 |
|---|---|---|
| `excluded_processes` | 없음 | 제외할 앱 이름. 대소문자를 가리지 않고 이름이 같으면 그 앱 사용을 기록하지 않는다 |
| `excluded_title_patterns` | 없음 | 제목 제외 정규식. 맞으면 제목만 비운다 |
| `redact_title_patterns` | 없음 | 제목 가림 정규식. 맞은 부분을 `[redacted]`로 바꾼다 |
| `mask_all_titles` | `false` | 모든 창 제목 숨기기 |
| `idle_threshold_ms` | `300000` | 마지막 입력 뒤 이 시간(ms)이 지나면 자리를 비운 것으로 보고 기록을 끊는다. 60000–86400000 밖이면 기본값을 쓴다 |

- 목록마다 64개, 규칙 하나에 512자까지다.
- 규칙 하나라도 맞지 않으면(빈 규칙, 너무 긴 규칙, 컴파일되지 않는 정규식) 모든 제목을 남기지 않는다. `[activity]`를 읽지 못해도 같다. 이때는 `excluded_processes`도 쓰지 않으므로 제외하려던 앱도 이름과 시간이 기록된다.

```toml
# config.toml
[activity]
excluded_processes = ["KeePassXC.exe"]
excluded_title_patterns = ['(?i)private']
redact_title_patterns = ['[\w.+-]+@[\w-]+\.[\w.]+']
mask_all_titles = false
idle_threshold_ms = 300000
```

## config.local.toml

`config.toml`과 같은 영역을 쓸 수 있고, 같은 키는 이 파일 값이 이긴다. `[windows]`는 이 파일에만 둔다.

| 영역 | 키 | 뜻 |
|---|---|---|
| `[windows]` | `root` | Windows 작업(`target = "windows"`)이 쓸 프로젝트 사본을 만들 Windows 폴더. `C:\…`처럼 드라이브 문자와 `:\`로 시작하고 `\`로 나눈다(`/`는 쓸 수 없다). Windows 작업을 시작할 때 확인한다. 사본은 `<root>\.devbox-v1\` 아래에 만든다 |

`root`가 없으면 Windows 작업을 시작하지 않는다. TOML에서 역슬래시를 그대로 쓰려면 작은따옴표 문자열을 쓴다.

```toml
# config.local.toml
[pc]
name = "laptop"

[windows]
root = 'C:\devbox'

[mcp]
write = false
```

## .devbox/devbox.toml

프로젝트 저장소의 `.devbox/devbox.toml`(256 KiB 이하)이다. 이 파일의 명령은 앱의 "프로젝트 명령 실행 승인" 창에서 내용을 확인하고 [명령 실행 승인]을 누른 뒤에만 실행한다. 승인은 그때의 파일 내용 그대로에 대한 것이라 내용이 바뀌면 다시 승인한다.

| 영역 | 키 | 뜻 |
|---|---|---|
| `[agent]` | `copy` | 작업 폴더를 만든 직후 저장소에서 복사할, Git이 추적하지 않는 파일(무시된 파일 포함, 예: `.env.local`). 표 아래 참고 |
| `[agent]` | `setup` | 작업 폴더 준비 명령. 에이전트 도구를 시작하기 전에 작업 폴더에서 차례로 실행하고, 하나라도 실패하면 도구를 시작하지 않는다 |
| `[agent]` | `test` | 테스트 명령 |
| `[agent]` | `teardown` | 작업 폴더 정리 명령 |
| `[launch]` | `services`·`terminals`·`agents` | 프로젝트 카드의 [시작 구성 실행]이 시작할 서비스 id, 열 터미널(`name`, 선택 `command`), 시작할 에이전트 작업(`tool`은 프로필 id, `worktree`는 작업 폴더를 만들지 여부로 기본 `false`, 선택 `prompt`) |
| `[[task]]` | 아래 표 | 작업 |
| `[[service]]` | 아래 표 | 서비스 |
| `[[schedule]]` | 아래 표 | 예약 |

`copy`의 경로는 저장소 안 상대 경로다. 폴더면 안의 파일을 모두 복사하고, 저장소에 없는 경로는 건너뛴다. 복사할 파일이 작업 폴더에 이미 있거나(Git이 추적하는 파일은 작업 폴더를 만들 때 이미 생긴다) 심볼릭 링크·특수 파일이면 작업 준비가 실패해 도구를 시작하지 않는다. 작업 폴더 없이 시작한 작업에는 복사하지 않는다.

`[[task]]`와 `[[service]]`의 키:

| 키 | 기본값 | 뜻 |
|---|---|---|
| `id` | 필수 | 1–128바이트, NUL 없음. 작업·서비스·예약 사이에서 겹치면 안 된다 |
| `command` | 필수 | 실행할 명령 |
| `cwd` | `"."` | 실행 폴더. 저장소(또는 `worktree`의 작업 폴더) 기준 상대 경로 |
| `env` | 없음 | 환경 변수 표. 키는 영문자·숫자·`_`이고 숫자로 시작하지 않는다. `HOME`·`XDG_CONFIG_HOME`·`XDG_DATA_HOME`·`XDG_STATE_HOME`·`XDG_RUNTIME_DIR`·`DEVBOX_INSTANCE`·`DEVBOX_RUN_ATTEMPT`·`INVOCATION_ID`·`NOTIFY_SOCKET`·`LISTEN_FDS`·`LISTEN_PID`는 쓸 수 없다. 값에는 제어 문자를 쓸 수 없고, `{{secret:이름}}`으로 비밀 값을 넣을 수 있다 |
| `target` | `"linux"` | `"linux"` 또는 `"windows"` |
| `dependsOn` | 없음 | 먼저 실행할 작업·서비스 id. `depends_on`으로 써도 된다 |
| `worktree` | 없음 | 실행할 작업 폴더. 없거나 `"main"`이면 저장소에서 실행한다. 그 밖에는 같은 프로젝트에서 작업 폴더가 있고 준비가 끝난, 닫히지 않은 에이전트 작업의 id다. 이때 `env`의 `DEVBOX_PORT`와 `portsHint`가 없으면 그 작업의 포트를 `DEVBOX_PORT`로 넘긴다 |
| `secrets` | 없음 | 같은 이름의 환경 변수로 넘길 비밀 이름 |

위 `env`·`worktree` 규칙은 대부분(빈 키, 키의 `=`, NUL 말고) 파일을 읽을 때가 아니라 실행을 시작할 때 확인한다. 맞지 않으면 그 실행을 시작하지 않는다.

`[[service]]`만 받는 키:

| 키 | 기본값 | 뜻 |
|---|---|---|
| `restart` | `"no"` | 끝났을 때 다시 시작할지: `"no"`·`"onFailure"`·`"always"`. `"on-failure"`·`"on_failure"`도 받는다 |
| `restartSec` | `1` | 다시 시작하기 전에 기다리는 초. 되풀이되면 30초까지 늘어난다 |
| `startLimit` | `{ burst = 5, intervalSec = 60 }` | `intervalSec`초 안에 `burst`번 넘게 시작하면 더 시작하지 않는다 |
| `portsHint` | 없음 | 서비스가 열 포트. 준비 확인에 쓰고 첫 포트를 `DEVBOX_PORT`로 넘긴다 |
| `readyTimeoutSec` | `60` | 준비를 기다리는 최대 초. 넘으면 준비되지 않은 것으로 끝난다 |

`[[schedule]]`의 키:

| 키 | 기본값 | 뜻 |
|---|---|---|
| `id` | 필수 | 예약 id. 빠지면 파일을 읽지 못한다 |
| `task` | 필수 | 실행할 작업 id |
| `cron` | 필수 | 5칸 또는 초를 더한 6칸 cron. `@daily` 같은 별칭과 `+`는 받지 않는다 |
| `tz` | WSL 시간대 | `"Asia/Seoul"` 같은 IANA 시간대 |
| `missed` | `"skip"` | 놓친 시각 처리: `"skip"` 또는 `"runOnce"`(`"run_once"`·`"run-once"`도 받는다) |

snake_case 별칭(`restart_sec`·`start_limit`·`interval_sec`·`ports_hint`·`ready_timeout_sec`)도 받는다.

```toml
# .devbox/devbox.toml
[agent]
copy = [".env.local"]
setup = ["pnpm install --frozen-lockfile"]
test = "pnpm test"
teardown = []

[launch]
services = ["web"]
terminals = [{ name = "shell" }, { name = "logs", command = "tail -f log/dev.log" }]
agents = [{ tool = "claude", worktree = true, prompt = "README를 읽고 할 일을 정리해 줘" }]

[[task]]
id = "build"
command = "pnpm build"

[[task]]
id = "report"
command = "pnpm report"
dependsOn = ["build"]
env = { REPORT_DIR = "out" }

[[service]]
id = "web"
command = "pnpm dev"
restart = "onFailure"
portsHint = [5173]

[[schedule]]
id = "nightly-report"
task = "report"
cron = "0 3 * * *"
tz = "Asia/Seoul"
missed = "runOnce"
```

## Windows settings.json

Windows 앱이 쓰는 파일이다. 앱은 같은 폴더의 임시 파일에 쓴 뒤 바꿔 넣는다. 형식이 맞지 않으면 앱은 이 파일을 덮어쓰지 않고 설정 저장을 거절한다.

| 키 | 기본값 | 뜻 |
|---|---|---|
| `distro` | 없음 | devbox가 쓸 WSL 배포판 |
| `autostart_asked` | `false` | Windows 시작 시 실행 여부를 물었는지 |
| `autostart` | `true` | Windows 시작 시 실행 |
| `tray_notice_shown` | `false` | 트레이 안내를 보였는지 |
| `global_shortcuts.palette` | 없음(`Ctrl+Alt+Space`) | 명령 팔레트 전역 단축키 |
| `global_shortcuts.capture` | 없음(`Ctrl+Alt+N`) | 빠른 캡처 전역 단축키 |
| `global_shortcuts.bottom_terminal` | 없음 | 하단 터미널 전역 단축키. 없으면 쓰지 않는다 |
| `activity.consent` | `false` | 활동 기록 동의 |
| `activity.paused` | `false` | 활동 기록 일시중지 |

괄호 안 값은 키가 없을 때 쓰는 단축키다. 전역 단축키에는 Ctrl·Alt·Shift 가운데 하나가 있어야 한다. 화면에서는 설정 → 단축키에서 바꾼다.

```json
{
  "distro": "Ubuntu",
  "autostart_asked": true,
  "autostart": true,
  "tray_notice_shown": true,
  "global_shortcuts": {
    "capture": "Ctrl+Alt+N",
    "palette": "Ctrl+Alt+Space",
    "bottom_terminal": null
  },
  "activity": { "consent": false, "paused": false }
}
```
