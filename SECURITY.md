# 보안 정책

## 범위

devbox는 한 사람이 자기 Windows 11 PC와 WSL에서 쓰는 개인 개발 도구다. 보안 범위는 [ADR 0016 개인용 보안 범위](docs/adr/0016-personal-security-scope.md)를 따른다.

- 유지하는 것: 개인정보 fail-closed, 링크 거부, DPAPI, 로그 비노출, peer 검증, OAuth 봉인.
- 이 범위에서 뺀 것: 업데이트 서명, Authenticode, pipe DACL 강화, CSP 추가, multipart grant, 복구 저널 암호화, 네이티브 확인 대화상자.

## 취약점 신고

GitHub 비공개 취약점 신고(Private vulnerability reporting)를 쓴다. 저장소의 Security 탭에서 "Report a vulnerability"를 누른다. 신고용 이메일 주소는 없다.

## 기본으로 꺼진 것

| 항목 | 기본값 | 켜는 방법 | 코드 위치 |
|---|---|---|---|
| 활동 기록 수집 | 꺼짐 | 설정 → 활동에서 동의 | `app/src-tauri/src/core/settings.rs`(`activity.consent` 기본 `false`), `app/src-tauri/src/core/activity.rs`(동의했고 일시중지가 아닐 때만 수집 hook을 돌린다) |
| MCP 쓰기 도구 | 꺼짐 | `config.toml`의 `[mcp] write = true` | `crates/core/src/config.rs`(`McpConfig::write` 기본 `false`), `crates/cli/src/mcp/tools.rs`(`list(write)`가 꺼져 있으면 쓰기 도구를 목록에 넣지 않는다), `crates/cli/src/mcp/tools.rs`·`runs.rs`·`notes.rs`(목록에 없는 쓰기 도구를 불러도 거절한다) |
| 웹훅 LAN 수신 | 꺼짐(127.0.0.1에서만 받음) | 웹훅 리스너의 "LAN에서 받기" | `app/src/features/tools/webhooks/ListenerForm.tsx`(체크 기본 해제), `crates/webhooks/src/address.rs`(`bind_address`: LAN이 아니면 127.0.0.1) |

- MCP 쓰기 도구는 `devbox_agent_send`·`devbox_run_task`·`devbox_note_add`다. `devbox mcp`는 시작할 때 설정을 한 번 읽는다.
- 쓰기 도구를 켜면 `devbox mcp`에 연결한 MCP 클라이언트는 어느 것이든 아래 일을 할 수 있다. devbox는 어느 에이전트가 불렀는지 확인하지 않는다.
  - `devbox_agent_send`: 부른 에이전트 자신의 작업이 아니어도 어느 에이전트 작업에든 지시를 보낸다.
  - `devbox_run_task`: 프로젝트의 작업이나 서비스를 시작한다. `.devbox/devbox.toml`의 정의는 승인한 내용으로만 실행하고, 앱에서 "이 PC에만" 저장한 정의는 승인 없이 실행한다.
  - `devbox_note_add`: 받은 노트함(`inbox`)이나 오늘의 일일 기록에 글을 덧붙인다.
- `devbox_agent_send`는 앞의 공백·줄바꿈을 뺀 글이 `!`나 `/`로 시작하면 어느 도구의 작업이든 보내지 않는다. 도구가 그 글을 셸 명령이나 도구 명령으로 실행할 수 있어서다(Codex·Claude Code·opencode·pi의 `!`, 각 도구의 `/` 명령. 붙여 넣은 `!`가 opencode를 셸 모드로 바꾸는지는 확인하지 못했다). 앱에서 보내는 그런 글은 그대로 넘긴다.
  - 사용자가 입력창에 `!` 초안을 남겨 두면 보낸 글이 그 초안 뒤에 붙어 함께 셸 명령이 될 수 있다. devbox는 Codex·opencode·Claude Code의 셸 모드를 화면으로 알아보고 보내지 않지만, pi는 셸 모드에서 입력창 테두리 색만 바뀌어 알아보지 못한다.
  - 도구가 붙여 넣기 모드(bracketed paste)를 켜지 않은 상태에서는 보낸 글의 줄바꿈이 Enter가 되어 줄마다 제출되므로, 여러 줄 글의 뒷줄이 `!`나 `/`로 시작하면 위 거절에도 불구하고 명령으로 실행된다(devbox는 줄마다 검사하지 않고 tmux는 이 상태를 알려 주지 않는다. 도구의 일반 입력 상태에서는 글 전체가 한 번에 붙는다).
- 에이전트 작업에 보내는 글에 탭·줄바꿈 말고 다른 제어 문자(C0·DEL·C1)가 있으면 앱에서 보내든 MCP로 보내든 보내지 않는다. 에이전트의 터미널에서 제어 문자는 키 입력이 되기 때문이다(Ctrl-U는 입력창을 지우고, 붙여 넣기 끝 표시 `ESC [201~` 뒤의 글은 직접 친 글이 된다). [에이전트에게 실패 로그 보내기]는 실행 로그의 색 코드 같은 제어 순서를 빼고 남은 제어 문자를 `�`로 바꾼 일반 글로 보낸다.
- "LAN에서 받기"를 켜면 0.0.0.0에서 받는다. Windows나 다른 PC에서 받는 데 필요한 `netsh interface portproxy` 명령은 화면에 안내만 하고 실행하지 않는다.

## 활동 기록

- 동의하기 전에는 수집하지 않는다. 일시중지하면 진행 중인 기록을 그 시각에 끝내고, 다시 켤 때까지 들어오는 관찰을 버린다. 동의를 거두면 아직 보내지 않은 앱 이름·제목을 지운다.
- 모으는 것은 Windows 앞 창의 앱 이름과 창 제목, 그리고 그 시간이다. 제목은 [개인정보 규칙](docs/config.md)을 적용한 뒤에만 앱 밖으로 나간다. 규칙을 읽거나 컴파일하지 못하면 고칠 때까지 활동을 기록하지 않는다. 제외할 앱 목록(`excluded_processes`)도 그 규칙에 들어 있어 그 앱들을 가려낼 수 없기 때문이다.
- 저장 위치는 WSL의 `~/.local/share/devbox/prod/devbox.db`다. 활동 기록을 처리하는 코드는 앱 이름·제목을 로그에 쓰지 않는다.
- 활동 기록을 외부로 보내는 기능은 없다. Windows 앱은 WSL 쪽 devbox에만 보낸다.
- 설정 → 활동의 "활동 기록 내보내기"는 고른 기간의 기록(시작·끝·앱 이름·제목·프로젝트)을 `~/.local/share/devbox/prod/exports/activity-<시작일>-<끝일>.csv`(또는 `.json`)에 암호화하지 않고 쓴다. 파일 권한은 사용자 전용(0600)이고, 같은 기간을 다시 내보내면 그 파일을 바꾼다. devbox는 이 파일을 지우지 않으므로 필요 없으면 직접 지운다.

## 비밀

- 비밀 값은 `devbox.db`에 XChaCha20-Poly1305로 암호화해 저장한다. 키는 무작위 32바이트이고, Windows DPAPI(현재 사용자)로 봉인한 키 파일만 `~/.local/share/devbox/prod/keys/data.key.dpapi`에 둔다. 평문 키 파일은 만들지 않는다.
- 복구 문자열은 이 키를 Base32로 적고 4글자씩 `-`로 이은 것이다. 설정 → 비밀의 [복구 문자열 보기]로 보고 안전한 곳에 보관한다. 키를 풀 수 없을 때 [복구 문자열 입력]이나 `devbox secrets import-key`(표준 입력)로 되살린다.
- 복구 문자열은 비밀 키 그 자체다. `devbox.db`와 함께 있으면 비밀 값을 풀 수 있으므로 남에게 보이지 않는다.
- 프로젝트 파일과 요청 파일에는 값 대신 비밀 참조(`{{secret:이름}}`)만 둔다.
- 실행 로그(로그 파일과 그것을 읽는 앱 화면·`logs.search`)와 MCP `devbox_logs` 응답에서는 그 실행에 넘긴 비밀 값(`secrets`·`{{secret:이름}}`)의 원문과, 따옴표·역슬래시·제어 문자를 이스케이프한 JSON·Rust 디버그 표기를 가린다. 그 밖의 방식으로 바꾼 값(base64·URL 인코딩, `<`·`é` 같은 문자까지 이스케이프하는 JSON 등)은 가리지 못한다. 이 판 전에 저장된 실행 로그는 다시 가리지 않는다.

## 언어 서버

코드 파일을 열면 devbox가 그 프로젝트 폴더에서 언어 서버(rust-analyzer·typescript-language-server 등)를 자동으로 띄운다. 이것은 `.devbox/devbox.toml` 실행 승인과 상관없다. 언어 서버와 그 서버가 부르는 도구(cargo 등)는 저장소의 설정과 코드(빌드 스크립트·proc macro 등)를 실행할 수 있다. 그래서 언어 서버가 설치돼 있으면 파일을 여는 것만으로 저장소 안의 코드가 실행될 수 있다. 믿지 않는 저장소는 프로젝트로 등록하지 않는다. 1.0.0은 자동 시작을 유지한다(사용자 결정 2026-11-06).

## 지원 번들

- 설정 → 앱 진단의 [지원 번들]은 이 PC에 ZIP 파일(`~/.local/share/devbox/prod/support/`)을 만들 뿐 어디에도 보내지 않는다.
- 저장된 비밀 값(원문·JSON 이스케이프·디버그 표기)과 홈·노트 경로를 가린다. 설치본에서의 가림 확인은 한 번 시도했지만 노트 경로가 번들 입력에 들어가지 않아 판정하지 못했다(INCONCLUSIVE). 그래서 NOT_RUN으로 남아 있다.
- 비밀 저장소가 잠겨 있으면 가릴 값을 알 수 없으므로 로그와 crash 기록을 넣지 않는다(`logs_omitted: "secrets_locked"`). 노트가 꺼져 있으면 노트 경로를 알 수 없으므로 역시 넣지 않는다(`logs_omitted: "notes_disabled"`).

## 설치 파일과 업데이트

- 설치 파일 `devbox-<버전>-setup.exe`에는 코드 서명(Authenticode)이 없고, 앱 업데이트도 서명을 쓰지 않는다(ADR 0016).
- 앱 업데이트는 받은 파일의 크기와 SHA-256을 GitHub Release 자산의 값과 비교한 뒤에만 설치 준비를 마친다. 직접 받은 설치 파일은 PowerShell `Get-FileHash`로 SHA-256을 확인한다.
- 서명이 없는 설치 파일에 Windows가 보이는 경고와 그 문구는 아직 확인하지 않았다([진행 원장](docs/superpowers/plans/2026-10-08-devbox-v1/PROGRESS.md) §4).

## WSL 쪽 연결

- 앱(`wsl.exe`로 실행한 `devbox bridge`를 거쳐)·CLI·MCP는 WSL 쪽 devbox의 Unix 소켓으로 연결한다. 소켓은 사용자 전용(`SocketMode=0600`)이다.
- 에이전트 작업은 로그인 셸 환경 가운데 허용 목록에 있는 변수만 받는다(`crates/cli/src/setup.rs`의 `ALLOWED`).
