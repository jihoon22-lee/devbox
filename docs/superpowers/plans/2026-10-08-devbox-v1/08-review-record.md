# 검토 기록

- 대상: [01-design](01-design.md) 초안(2026-10-07)
- 방식: 서로 다른 관점의 리뷰어 다섯 명이 같은 초안을 병렬로 검토했고, 주 작성자가 소견마다 수용·수정·기각을 정해 설계에 반영했다.
  - 저장소 AGENTS.md는 하위 에이전트로 GPT-6 Astra/Sol을 지정한다. 이 문서를 만든 Claude Code 세션에서는 그 모델을 쓸 수 없어 Claude 하위 에이전트로 수행했다.
- 리뷰어와 관점:
  1. 플랫폼 기술(WSL·systemd·Tauri·외부 도구의 실제 동작)
  2. 기능 범위(v0.9.0 기능 대비 누락·제외 판단)
  3. 화면·사용성
  4. 단순성·실행 가능성·일관성(반대 의견 포함)
  5. 프로토콜·데이터 모델·상태 관리
- 한계: 리뷰어는 문서와 기존 코드를 읽고 웹 자료를 확인했다. 플랫폼 리뷰어의 측정 외에는 실행 검증이 아니다. 실제 동작은 S0 구조 확인 실험(02-s0 T0)에서 확인한다.

## 판정 표기

- **수용:** 소견대로 설계를 바꿨다.
- **수정 수용:** 방향은 받아들이되 다른 형태로 반영했다.
- **기각:** 반영하지 않았다(이유를 적음).

## 1. 플랫폼 기술

리뷰어가 WSL 안에서 조회와 소규모 측정을 직접 했다. 설정은 바꾸지 않았다.

| ID | 소견 | 판정 | 반영 위치 |
|---|---|---|---|
| PL-1 | 배포판은 연결된 `wsl.exe`가 없으면 `instanceIdleTimeout`(기본 15초), VM은 `vmIdleTimeout`(기본 60초) 뒤 멈추고 systemd 서비스는 사용 중으로 치지 않는다(WSL 이슈 #13291). 앱이 연결을 유지하면 유지된다 | 수용 | §4.1 유휴 종료 대응, §14 R1, doctor 점검 항목 |
| PL-2 | `wsl.exe -u root`는 비밀번호 없이 동작. `--exec`에도 `XDG_RUNTIME_DIR`·`DBUS_SESSION_BUS_ADDRESS`가 있다. WSL이 로그인 세션을 만들어 linger 없이도 user manager가 뜬다 | 수용 | §4.1 linger는 보험, bridge가 `XDG_RUNTIME_DIR` 계산 |
| PL-3 | D-Bus로는 `StandardOutputFileToAppend`를 써야 하고, `CPUAccounting`은 systemd 259에서 무시되며, 정상 종료 unit은 바로 정리돼 결과를 놓치기 쉽다 | 수정 수용 | SE-3로 `systemd-run` CLI를 택했고, 출력은 실행 래퍼가 직접 쓴다. `ExecStopPost`로 결과 기록, CPU는 `CPUUsageNSec` |
| PL-4 | tmux는 `Type=exec` + `-D`(포그라운드 서버)가 낫다. pane 명령은 PATH가 필요하다. scope로 옮긴 에이전트는 tmux unit을 멈춰도 죽지 않는다 | 수용 | §4.3 tmux unit, 에이전트 scope 명시 정지 |
| PL-5 | Claude hook은 `--settings`로 주입 가능, stdin에 `session_id`·`notification_type` 등. Codex `notify`는 `agent-turn-complete`만 알리고 입력 대기 신호가 없으며, Codex hook은 신뢰 승인 전에는 건너뛴다 | 수용 | §9.2 hook 주입(사용자 설정 미변경), Codex 입력 대기는 tmux 출력 감지 기본. FS-15와 SE-14의 엇갈림을 "실행 인자 주입"으로 정리 |
| PL-6 | `@codemirror/lsp-client` 6.3.0의 `Transport`는 메시지 단위 문자열, `language-data` 6.5.2는 동적 로드 | 수용 | §9 lsp 메시지 단위 중계 |
| PL-7 | Tauri 알림 플러그인의 Actions는 모바일 전용이라 데스크톱 클릭 이벤트가 없다. Windows 알림은 설치본에서만 동작 | 수용 | §6.1 WinRT 토스트 + protocol activation + deep-link, §14 R12 |
| PL-8 | 브리지 실측: 64B 왕복 p50 0.16ms·p99 0.25ms, 4KB p50 0.17ms, 처리량 77–93 MiB/s, 첫 응답 180–290ms(VM 실행 중), 8MiB 무작위 바이트 왕복 일치. 한계: VM 부팅 상태·데몬 구간 미측정 | 수용 | §5.4, §14 R3 |
| PL-9 | GitHub 러너의 systemd user 세션은 linger + `XDG_RUNTIME_DIR`로 가능하다는 사례가 있으나 미측정, 컨테이너 job은 불가 | 수용 | §11 L2(S0a에서 확인) |
| PL-10 | RENAME_EXCHANGE·inotify 동작(tmpfs에서 측정, ext4는 문서 근거). `/mnt/c`의 Windows 쪽 변경은 inotify가 감지 못함 | 수용 | §9 lsp·documents |
| PL-11 | `\\wsl.localhost` 쓰기는 9p 경유라 실행 비트·중단 처리·Defender 지연 문제. stdin 방식은 8MiB 바이너리 왕복 일치 | 수용 | §4.1 바이너리 배치 |
| PL-위험1 | systemd user manager PATH에 `~/.local/bin`·`~/.cargo/bin`이 없어 실행 unit·tmux에서 사용자 도구를 못 찾음(실측) | 수용 | §4.1 setup의 사용자 환경 잡기, §14 R13 |
| PL-위험2 | transient unit 결과 유실 | 수용 | PL-3과 같음 |
| PL-위험3 | L2 테스트와 실제 데몬의 unit·tmux 이름 충돌, unit은 manager의 HOME으로 실행 | 수용 | SE-11 인스턴스 분리 + 테스트 slice, `HOME`·`XDG_*` 명시 |
| PL-위험4 | `wsl.exe` 자체 오류는 UTF-16LE 현지화 문장이라 프레임과 섞일 수 있음 | 수용 | §5.4 브리지 표지 바이트열, Job Object |
| PL-위험5 | WSL은 Store 자동 업데이트·`wsl --shutdown`·앱 사망 15초 뒤에도 멈춘다 | 수용 | §4.1, §9.2 `session_lost` |
| PL-위험6 | Codex hook 신뢰 승인 때문에 입력 대기 감지가 조용히 빠질 수 있음 | 수용 | PL-5와 같음 |
| PL-위험7 | `wsl.exe --exec`로 띄운 프로세스는 `/init.scope`라 R2의 "브리지 자식 데몬" 대체 경로는 cgroup 측정이 안 되고 브리지와 함께 죽는다 | 수용 | 그 대체 경로를 설계에서 뺌 |
| PL-위험8 | musl 기본 할당자는 tokio 멀티스레드에서 느림 | 수용 | §4.2 mimalloc |
| PL-위험9 | transient 서비스는 TTY가 없어 버퍼링·색 꺼짐. inotify instances 한도 128 | 수용 | §4.3 `PYTHONUNBUFFERED`·`FORCE_COLOR`, §14 R14 |

## 2. 기능 범위

| ID | 소견 | 판정 | 반영 위치 |
|---|---|---|---|
| FS-1 | Docker 컨테이너 목록·시작·중지·재시작, 포트의 컨테이너 표시가 빠짐(v0.9.0 `terminal-engine` docker_ps·docker_action) | 수용 | §2.1 실행 |
| FS-2 | 로그 소스(파일 tail, Docker·Podman 컨테이너 로그)가 빠짐(`logs-engine/core/sources.rs`) | 수용 | §2.1 실행 로그 |
| FS-3 | 로그 북마크·저장된 보기 | 수용(제외 후보) | §2.6 X-10 |
| FS-4 | 프로젝트 파일 미리보기(Markdown·Mermaid·SVG) — 에이전트가 만든 계획·보고서 읽기 | 수용 | §2.3 |
| FS-5 | 편집기·로그 선택을 변환 도구로 보내기, API 응답을 노트에 붙이기 | 수용 | §2.4 |
| FS-6 | 브랜치·worktree 안전 정리(`CleanupPanel`) | 수용 | §2.1 에이전트 정리, §2.3 Git |
| FS-7 | 포트를 점유한 프로세스 종료 | 수용 | §2.1 포트 |
| FS-8 | 터미널 빠른 호출 전역 단축키 | 수용(선택) | §2.1 터미널 |
| FS-9 | 셸 연동(현재 위치 추적)·벨 표시 | 수용(최소 범위) | §2.1 터미널 |
| FS-10 | clone으로 프로젝트 추가(두 번째 PC) | 수용 | §2.1 프로젝트 |
| FS-11 | 예약 실패·재시작 한도 초과 알림 | 수용 | §2.1 실행 |
| FS-12 | multipart·바이너리 응답·세션 변수·JSONPath를 이식 목록에 명시 | 수용 | §2.4 |
| FS-13 | X-6 이유 오류: 화면 분할이 칸마다 별도 tmux 세션이면 `synchronize-panes`로 대체 불가 | 수용 | X-6 이유를 "사용 빈도 낮음"으로 |
| FS-14 | X-8 보완: `cmd.exe`는 `\\wsl.localhost`를 작업 폴더로 못 쓰고 WSL `node_modules`·`target`은 Linux용. Windows 대상 작업에는 Windows 사본 경로와 실행 전 동기화가 필요 | 수용 | §2.1 실행, §4.3, §7 `devbox.toml` 예 |
| FS-15 | 에이전트 필수 보강: 작업 준비(setup·copy, 기준 브랜치 선택, worktree 없이 실행), 세션 ID 기록과 재개, 지시·토큰·커밋 기록 고정(Claude Code transcript 30일 삭제), 기준 반영(rebase)과 뒤처짐 표시, 병합·폐기 뒤 정리, hook 설치 규칙(사용자 설정에 한 번, 기존 hook 보존·연쇄, `DEVBOX_TASK_ID` 없으면 무시), 브랜치·위치 규칙, 실행할 worktree 선택 | 수용 | §2.1 에이전트, §9 agents, §9.1 |
| FS-16 | 에이전트 선택 보강: 포트 분리(`DEVBOX_PORT`부터 10개), 도구 프로필, 지시문 템플릿(`.devbox/prompts/*.md`), 노트에서 작업 만들기, 같은 지시 재시도, teardown | 수정 수용 | 도구 프로필·지시문 템플릿·teardown·포트 분리는 S1 범위. "노트에서 작업 만들기"·"재시도 비교"는 S3·S1 후반 선택 과제 |
| FS-17 | 과한 것: 달러 비용·한도 대시보드, 동시 실행 상한·대기열, transcript 전체 뷰어 | 수용(넣지 않음) | §2.1 비고 |
| FS-18 | 두 PC: 개인 API 컬렉션은 `~/.config/devbox/api/` 파일로, 설정은 공유값 `config.toml`과 PC별 `config.local.toml`로 분리. 앱에서 만든 작업은 "프로젝트 파일에 저장"을 먼저 제안. 비밀 이름은 공유하고 값은 PC별. 일일 기록 요약은 PC 이름 소제목 아래 | 수용 | §7 |
| FS-19 | 중복: 코드 검색 두 곳, 진단 세 곳, 에이전트 테스트 실행 vs runtime, `[launch]`의 claude 터미널 vs 에이전트, 에이전트 diff vs 소스 화면 | 수용 | §2.3·§2.5·§8: search crate 하나에 진입점 둘, 진단 화면 하나, runtime 재사용, launch의 에이전트는 worktree 없는 에이전트 작업, diff·병합·충돌 컴포넌트 공유 |
| FS-20 | 문서 오류: 변환 도구 22종 → 21종, API Workspace 묶음 참조 X-10 → X-9 | 수용 | §2.4 |

## 3. 화면·사용성

| ID | 소견 | 판정 | 반영 위치 |
|---|---|---|---|
| UX-H1 | 4열 보드는 1180–1440px 폭에서 열이 100–140px로 좁아 카드 정보가 들어가지 않는다. 시스템이 상태를 바꾸므로 칸반이 맞지 않는다. '실패'·'준비 중'·'닫힘' 상태가 정의되지 않았다 | 수용 | §8.2 에이전트: 긴급순 목록 + 상세, 격자 보기. 상태 6개 고정(준비 중·실행 중·입력 대기·실패·검토 대기·닫힘) |
| UX-H2 | Ctrl+K·P·B·N·D·O가 셸 readline·tmux·zellij·Claude Code·CodeMirror 키와 충돌한다. 한글 IME에서는 `event.key`가 자모로 온다. WebView2 가속키(Ctrl+P 인쇄, F5 새로고침, Ctrl+± 확대)가 남아 있다 | 수용 | §8.5 단축키 정책과 기본값 표: 포커스 범위 규칙, `event.code` 매칭, 레지스트리, WebView2 가속키 끔 |
| UX-H3 | tmux `mouse on`이면 스크롤백·복사가 tmux 복사 모드로 넘어가 xterm 검색·링크·클립보드가 깨진다 | 수용 | §4.3 devbox 전용 tmux 설정(prefix None, unbind -a, mouse off, status off, 대체 화면 끔), 화면 분할 칸 하나 = tmux 세션 하나, 재연결 시 `capture-pane` 선채움, 격자는 read-only·ignore-size attach |
| UX-H4 | 첫 실행·장애 화면(WSL 미설치, 배포판 여러 개·WSL1, systemd 꺼짐, linger·setup 실패, 재배치 중, 데몬 끊김, 업데이트 중, 키 분실)과 Claude Code·Codex hook 설치·점검 단계가 없다. SC1에 전제가 빠졌다 | 수용 | §6.5 상태 화면 표, §2.1 hook 설치·점검, SC1 전제 |
| UX-M1 | 홈은 "지금 볼 것"이 에이전트 목록·상태 표시줄·알림과 겹치는 중간 화면이다 | 수용 | 홈 섹션 제거. 시작 화면은 마지막 섹션(처음은 에이전트). 프로젝트 카드는 확장된 프로젝트 전환기로, 알림은 알림 센터로 옮김. 활동 막대 6개 + 배지 |
| UX-M2 | 전역 프로젝트 전환과 여러 프로젝트에 걸친 에이전트의 범위가 정의되지 않았다 | 수용 | §8.1 범위 규칙: 에이전트·알림·상태 표시줄은 전체(필터 칩), 터미널·실행·소스·노트는 현재 프로젝트. 에이전트 검토는 전역 맥락을 바꾸지 않음 |
| UX-M3 | 하단 패널 '출력' 미정의, 로그·터미널 중복, 소스의 파일 트리 자리 없음, 통합 검색·활동·에이전트 테스트 결과 위치 없음 | 수용 | 하단 패널 = 터미널·로그·문제. 소스 목록 [파일 \| 변경 \| 브랜치]. 통합 검색 = Ctrl+Shift+F 본문 탭. 활동 = 노트 목록 그룹. 에이전트 상세에 '실행' 탭 |
| UX-M4 | IME 조합 중 Ctrl+Enter 무시 규칙 때문에 첫 제출이 먹지 않는다. xterm 한글 조합이 불안정하다 | 수용 | 제출 단축키는 `compositionend` 뒤 실행. 에이전트 지시는 별도 작성란 + bracketed paste |
| UX-M5 | Cascadia Mono에 한글이 없어 셀 폭이 어긋난다. 행·컨트롤 높이 토큰이 없다 | 수용 | 코드 글꼴 Cascadia Mono → D2Coding(포함), xterm unicode11, 높이 토큰 |
| UX-M6 | 확인·되돌리기 판단을 화면마다 하면 다시 갈린다. 결과를 모르는 변경의 처리가 없다 | 수정 수용 | `rpc!` 선언에 `policy`(reversible·confirm·none)를 넣어 생성. 오류 위치 규칙. 변경 요청은 `requestId`로 데몬이 10분간 결과를 기억해 다시 눌러도 안전(재시작 시에는 "결과 확인 필요") |
| UX-M7 | 연결이 끊겼을 때 전부 읽기 전용이면 업데이트 때마다 노트 작성이 막힌다 | 수용 | 문서 편집은 계속 허용, 저장은 "연결되면 저장"(화면 쪽 임시 초안 보관) |
| UX-M8 | 방해 금지 모드에서는 토스트가 안 보인다 | 수용 | 작업 표시줄 오버레이 배지 + FlashWindow, 토스트 tag 교체·묶음, 첫 창 닫기 때 트레이 안내 |
| UX-L1 | 빠진 컴포넌트·상태 | 수용 | §8.3 컴포넌트 목록 보강, stale·forced-colors, 색 외 모양 구분 |
| UX-L2 | 제목 표시줄의 배포판 상시 표시·'●' 미정의 | 수용 | 배포판은 툴팁, '●'은 데몬 연결 상태로 정의 |
| UX-L3 | 팔레트에서 클립보드 값으로 변환 도구 실행. '개발 환경'과 '진단' 이름 구분 | 수용 | §8.2 도구, 이름: 도구 섹션 "개발 환경 점검", 설정 "앱 진단" |
| UX-U1 | 기본 창 크기·하단 패널 기본값이 없어 편집 영역이 다시 60% 미만이 될 수 있다 | 수정 수용 | 기본 창 = 작업 영역의 80%(최대 1600×1000, 최소 1180×760). 소스·노트는 하단 패널 기본 닫힘, 본문 65% 이상 |
| UX-U2 | 금지어 검사가 없다 | 수용 | CI에 금지어 검사(v0.7 앱 이름, 내부 용어, ⌘) |
| UX-U4 | 뒤로/앞으로 미정의 | 수용 | Alt+←/→, 마우스 4·5번 버튼 |
| UX-U5 | 좁은 창의 응답 위치 | 수용 | 1100px 미만이면 응답을 요청 아래로 |
## 4. 단순성·실행 가능성·일관성

| ID | 소견 | 판정 | 반영 위치 |
|---|---|---|---|
| SE-1 | DPAPI 키 전달은 유지하되 `secrets_locked`는 공통 오류 하나로 끝내고, 예약 전용 안내는 뺀다. 키를 잃으면 "비밀 다시 입력"만. 키 파일이 NSIS 설치 폴더(`%LOCALAPPDATA%\Devbox`)와 겹친다 | 수용 | §6.3: 키는 `%LOCALAPPDATA%\<identifier>`(앱 데이터 폴더) |
| SE-2 | dev-gateway 유지 | 수용 | §5.6 |
| SE-3 | D-Bus `StartTransientUnit`의 `ExecStart a(sasb)` 직렬화는 틀리기 쉽다. `systemd-run` CLI를 쓰고 활성 실행만 조회 | 수용 | §4.3: `systemd-run --user`. 종료는 실행 래퍼가 알리고, 데몬 시작 때만 `systemctl --user show`로 맞춤 |
| SE-4 | tmux transient unit → setup이 쓰는 정적 unit 파일. `-L` 분리는 유지 | 수용 | §4.1·§4.3: 템플릿 unit `devbox-tmux@.service` |
| SE-5 | 스트림을 구독의 한 종류로 합치고 입력은 Request로. 바이너리 프레임 태그는 유지 | 수용 | §5.1 |
| SE-6 | 활동 재시도 큐를 연결이 끊긴 동안 앱 메모리 1,000건으로 축소. DB 오류는 표시만 | 수용 | §9.1-5 |
| SE-7 | 마이그레이션 백업은 `VACUUM INTO` 1개. FTS 같은 파생 데이터는 `index.db`로 분리해 백업·마이그레이션에서 빼고 다시 만든다 | 수용 | §4.4, §7 |
| SE-8 | 분리 디버그 정보 90일 보관 제거 | 수용 | §4.5 |
| SE-9 | 무결성 확인은 하나만. self-check는 실행 + 내장 Linux 바이너리 버전 일치만 | 수용 | §10 |
| SE-10 | 새 ADR 7개 → 1개 + 옛 ADR 상태 갱신 | 수용 | §13 |
| SE-11 | 인스턴스 분리 누락: 임시 HOME으로는 systemd user manager·`/tmp/tmux-UID`가 분리되지 않아 L2·dev 데몬이 실사용 unit·tmux와 충돌 | 수용 | §4.1 `DEVBOX_INSTANCE`(prod·dev·test-<난수>)가 소켓·`tmux -L`·unit 인스턴스 이름·데이터 경로를 함께 정함 |
| SE-12 | `StandardOutput=append:`는 시각·스트림 구분이 없어 병합 보기가 안 되고, systemd가 연 파일은 잘라낼 수 없다 | 수용 | §4.3 `devbox exec -- <cmd>` 실행 래퍼(줄마다 시각·스트림, 회전, 종료 코드 보고) |
| SE-13 | 에이전트 상태 enum·전이표가 없다. Claude `Stop` hook은 턴 종료일 뿐 완료가 아니다 | 수용 | §9.2 에이전트 상태 기계 |
| SE-14 | worktree 위치 규칙과 쓰기 허용 범위 연결, hook을 사용자 전역 설정 대신 실행 인자로 주입 | 수정 수용 | §9.2: worktree 위치 규칙. hook은 실행 인자 주입(`claude --settings`, `codex -c`)을 기본으로 하고, S0a에서 동작을 확인한다. 안 되면 FS-15의 "사용자 설정에 한 번, 기존 hook 연쇄"로 대체 |
| SE-15 | S4가 너무 크다 → S4a(편집·LSP·Git)·S4b(API·웹훅·도구). API 순서: HTTP·GraphQL·가져오기 → WS·SSE → gRPC·MCP·OAuth | 수용 | §15: S4 편집·Git, S5 개발 도구, S6 마무리 |
| SE-16 | apt 등 sudo가 필요한 설치 명령은 터미널 세션에서 실행 | 수용 | §2.4, §9 tools·lsp |
| SE-17 | socket activation만으로는 예약이 돌지 않는다. WSL도 앱이 띄우기 전엔 꺼져 있다 | 수용 | §4.1: 데몬은 linger + `devbox@.service` enable로 user manager 시작 시 기동. 예약은 데몬이 돌 때만 동작하고, 놓친 예약은 설정에 따라 1회 보충 또는 건너뜀 |
| SE-18 | 재시작 정책 주체가 systemd와 데몬 두 곳 | 수용 | §4.3: systemd `Restart=`·`RestartSec`·`StartLimitBurst`만, 데몬은 표시만 |
| SE-19 | 업데이트 진행 상태 소유자 불일치(F09 조건) | 수용 | §9.1-6: Windows 앱이 소유하고 모든 창에 알림 |
| SE-20 | "작업"이 세 가지 뜻으로 쓰임 | 수용 | §8.6 용어: 작업(`[[task]]`)·서비스·예약·실행(한 번의 실행)·에이전트 작업·진행 작업(operation) |
| SE-21 | 에이전트 상태가 §1·§2.1·§8.2에서 다름 | 수용 | §9.2 enum 하나로 통일 |
| SE-22 | 외부 검토 번호 체계가 섞임 | 수용 | 출처 접두: `RV-`(본질 리뷰), `CV-`(review-cross-validation), `AR-`(astra-review), `AU-`(10-03 감사) |
| SE-23 | TCP 예외에 OAuth PKCE 콜백·R3 대체 경로가 빠짐 | 수용 | §5.1 |
| SE-24 | SC1 "새 Windows 계정"에는 WSL 배포판이 없음 | 수용 | SC1 문구 |
| SE-25 | 수동 측정이 완료 조건인데 L5는 게이트 아님 | 수용 | §11: 각 S 완료 조건의 수동 항목 10개 이하, 사용자 1회 확인 |
| SE-26 | SC2·SC3의 "홈"·"보드" 용어 | 수용 | "첫 화면", "에이전트 목록" |
| SE-27 | L3 "영향 범위만"은 옛 scope gate의 씨앗 | 수용 | §11: L3 전체를 매 PR 병렬 실행 |
| SE-28 | L4 주 1회 검증은 아무것도 막지 않는 소음 | 수정 수용 | Windows 빌드는 태그·수동 실행과, `app/src-tauri/**`·`crates/win/**`를 바꾼 PR(워크플로 `paths` 필터)에서만 |
| SE-29 | SC7에 X5를 직접 재는 지표 추가 | 수용 | SC7: `.github`+`scripts` 1,500줄 이하, 워크플로 3개 이하, flaky 테스트는 발견한 PR에서 고치거나 삭제(재시도 래퍼 금지) |
| SE-30 | Tauri 프레임 라우팅을 플랫폼 독립 crate로 빼 Linux에서 시험 | 수용 | §5.4: `crates/mux` |
| SE-31 | 이식 출처의 기존 테스트도 함께 옮긴다 | 수용 | §9 이식 표 |
| SE-32 | SC8을 S별 줄 수 예산(초과 시 단순화 검토 트리거)으로 | 수용 | SC8 |
| SE-33 | 데몬 유휴 자원 예산이 없다 | 수용 | SC9: 유휴 RSS 60MB·CPU 0.5% 이하 |
| SE-34 | 첫 PR에서 옛 AGENTS.md·CLAUDE.md·`.github`부터 교체 | 수용 | 02-s0 첫 과제 |
| SE-35 | S0이 너무 크다 → S0a spike(합격 기준 명시) + S0b 걷는 뼈대. 트레이·단축키·알림은 S1, 업데이트·설치기는 마지막, 디자인 시스템은 S1이 쓰는 컴포넌트만 | 수용 | §15, 02-s0 |
| SE-36 | R6 전환점: S2가 끝나면 에이전트·실행·터미널은 새 로컬 빌드로 옮김. 각 S 끝의 빌드가 단독으로 쓸 만해야 함 | 수용 | §14 R6, 00-roadmap |
| SE-37 | 반대 의견 수용: 같은 여정을 v0.9.0과 수치 비교, "그대로 이식/참고만" 구분, 회귀 조건 AR-D01(여러 파일 링크 rewrite 중단)·재시작 backoff 추가, v0.9.0 사용 중 회피 목록(CV-F01) | 수용 | S0a, §9 이식 표, §9.1, 00-roadmap 회피 목록 |
| SE-38 | 공존: v0.9.0과 전역 단축키(Ctrl+Alt+Space·Ctrl+Alt+N)가 같다 | 수용 | 개발 빌드 기본값을 Ctrl+Alt+Shift+Space·Ctrl+Alt+Shift+N으로, 등록 실패 표시 |
| SE-39 | 공존: 업데이트 자산 이름 `Devbox_{v}_x64-setup.exe`가 v0.9.0 업데이터와 겹치고, 새 개발 빌드가 v0.9.0을 업데이트로 제안할 수 있다 | 수용 | 자산 이름 `devbox-<v>-setup.exe`, 개발 빌드는 업데이트 확인 끔·버전 `1.0.0-dev`, 릴리스 노트에 수동 설치·v0.9.0 수동 제거 |
| SE-40 | 공존: 트레이·활동 수집이 둘씩 생김 | 수용 | 첫 실행에서 v0.9.0 Knowledge 활동 수집이 켜져 있으면 한쪽만 켜도록 안내 |
| SE-41 | 공존: v0.9.0도 `.devbox/project.json`을 씀 | 수용 | 새 앱은 `.devbox/devbox.toml`·`api/`만 읽고 다른 파일은 무시 |
| SE-42 | 개발 서버 5173은 사용자 Vite 기본 포트와 겹침 | 수용 | 화면 개발 서버 1450(strictPort), dev-gateway 1451 |

## 5. 프로토콜·데이터 모델·상태 관리

| ID | 소견 | 판정 | 반영 위치 |
|---|---|---|---|
| PD-H1 | 요청 ID에 창 번호를 붙이면 Tauri가 16MiB JSON을 해석·재직렬화해야 하고, 창을 닫으면 구독·PTY·lease가 남는다 | 수용 | §5.1 프레임 헤더 `u32 len \| u8 kind \| u32 channel`, kind 2 제어(`ChannelOpen`·`ChannelClose`·`StreamCredit`). 창 파괴·새로 고침 시 `ChannelClose`로 일괄 정리 |
| PD-H2 | 흐름 제어·역압 없음. `broadcast`는 느린 수신자 이벤트를 조용히 버리고, 큰 JSON 하나가 공유 파이프를 막는다 | 수용 | §5.1 스트림별 신용 창(256KiB), 채널별 큐 라운드로빈·JSON 우선, Hub는 연결을 기다리지 않음, 이벤트 큐가 넘치면 `Resync{topic}`, JSON 평소 1MiB 이하 |
| PD-H3 | 재연결 중 유실·순서 역전·늦은 변경 응답이 새 결과를 덮는 문제 | 수용 | §5.5 rev 규칙(R4–R7) |
| PD-H4 | 창마다 DocumentStore가 따로 생겨 같은 문서가 두 벌이 된다. lsp-client는 같은 파일 다중 뷰를 거부한다 | 수용 | §8.4 데몬 편집 lease(`documents.claim`), 다른 창은 읽기 전용 + "여기로 가져오기", 저장·저널은 lease·rev CAS |
| PD-H5 | `tag="code"`만으로는 `{code, detail}`이 나오지 않고 newtype 변형은 실행 중 실패. 접두사 자동 부여 불가 | 수용 | §5.3 `tag="code", content="detail"`, 변형별 rename을 `domain_error!`가 생성, `diagnosticId`는 Response 수준 |
| PD-H6 | DPAPI 키를 잃으면 비밀 전부 분실(WSL 이전, Windows 재설치, 계정 비밀번호 재설정, 제거 시 앱 데이터 삭제). DPAPI의 추가 이득은 "DB 파일만 유출" 방지뿐인데 그 대가로 unlock 상태가 생긴다. WSL 키 파일(0600) + 복구 문자열 + `key_id` 제안 | 수정 수용 | DPAPI 유지는 사용자의 이전 결정(ADR 0016)이라 유지한다. 대신: 봉인된 키 blob은 WSL 데이터 폴더에 두고, 데몬이 시작할 때 interop PowerShell로 직접 푼다(unlock RPC·앱 의존 제거, interop 불가 시에만 앱이 대신 풂). 첫 설정에서 복구 문자열을 안내하고, `key_id`로 키 불일치를 감지해 "복구 문자열 입력" 또는 "비밀 다시 입력"을 제공한다. 키 파일 방식으로 단순화할지는 00-roadmap의 "결정 대기"에 남긴다. → 2026-10-08 Q1에서 DPAPI로 확정 |
| PD-H7 | tmux 마우스 켬과 화면 리뷰 충돌, attach 클라이언트의 대체 화면 진입으로 스크롤백이 막힘, 입력은 스트림이어야 함, read-only·ignore-size는 tmux 3.2 이상, 격자는 CSS 축소. control mode보다 PTY attach 유지 권고 | 수용 | §4.3 tmux 설정·하한 3.2, 입력 스트림, 격자 CSS 축소, PTY attach 유지 |
| PD-M1 | 메서드 종류 선언이 없어 "조회만 재시도" 판단 근거가 없다 | 수용 | §5.2 `query`·`mutation`·`subscription`·`stream` |
| PD-M2 | 부분 성공·결과 불명 표현 | 수용 | §5.3 `Ok{value, warnings}`, `truncated`, 클라이언트 전용 `connection_lost`(결과 불명) vs `daemon_restarting`(적용 안 됨), 생성형 변경은 `clientRequestId` 멱등, 변경의 `cancelled`도 결과 불명 |
| PD-M3 | 긴 색인 쓰기가 단일 writer를 점유해 SC3을 위협. 분리 기준 | 수용 | §4.4 `devbox.db`(핵심)·`index.db`(파생, 백업 제외)·큰 본문은 파일, DB당 writer 하나, 도메인별 마이그레이션 버전, 커밋 뒤 이벤트 발행 |
| PD-M4 | LSP 중계 세부(v1 서버당 세션 1개, 진단 엿보기, URI 권위, UTF-16, 열리지 않은 파일 편집, 재시작 백오프, 유휴 기준) | 수용 | §9 lsp |
| PD-M5 | 저장 세부(mode 보존, symlink 대상, CRLF·BOM, 이진·비 UTF-8·크기 초과는 읽기 전용, 자기 저장 echo, 끊김 중 편집) | 수용 | §9.1-2 |
| PD-M6 | 노트·코드 공통 모델은 타당. `DocumentPolicy`로 차이 분리, 문서별 `EditorState` 보관, rename 대상이 dirty면 버퍼에 적용 또는 거부 | 수용 | §8.4 |
| PD-M7 | 큰 정수·ID·해시는 문자열, 시간은 `*_ms` | 수용 | §5.2 |
| PD-L1–L4 | 프레임 길이 정의·길이 0 거부·잘못된 프레임은 연결 종료, 재배치 중 `daemon_restarting`, ts-rs·TypeVisitor 재사용(specta 2는 RC, typeshare는 serde 반영 약함), 백업은 `devbox.db`만 | 수용 | §5.1·§5.2·§4.4 |

## 6. 리뷰어 사이에서 엇갈린 판단

| 주제 | 의견 | 결정 |
|---|---|---|
| hook 설치 위치 | FS-15: 사용자 설정에 한 번 설치하고 기존 hook 연쇄 / SE-14: 실행 인자로 주입 | 실행 인자 주입(PL-5가 Claude `--settings`·Codex `-c` 동작 근거 제시). 사용자 설정 파일은 건드리지 않는다 |
| 비밀 키 | SE-1: DPAPI 유지·축소 / PD-H6: DPAPI를 버리고 WSL 키 파일 | DPAPI 유지(사용자의 이전 결정, ADR 0016). 데몬이 interop으로 직접 풀어 unlock 상태를 없애고, 복구 문자열·`key_id`로 분실에 대비. 키 파일 방식으로 단순화할지는 사용자 결정 대기로 남김(00-roadmap) → 2026-10-08 Q1에서 DPAPI로 확정 |
| systemd 사용 방식 | 초안: D-Bus transient unit / SE-3: `systemd-run` CLI / PL-3: D-Bus 속성 세부 | `systemd-run` CLI + 실행 래퍼 + `ExecStopPost`. D-Bus 직접 호출은 쓰지 않는다 |
| tmux 분할 | 초안: tmux 내부 분할 가정(X-6) / UX-H3·PD-H7: 화면 분할 칸마다 세션 | 칸마다 세션. X-6 이유를 "사용 빈도 낮음"으로 수정 |
| 홈 화면 | 초안: 홈 섹션 / UX-M1: 제거 | 제거. 프로젝트 카드는 전환기로 |

## 7. 사용자 결정 (2026-10-08)

사용자가 권장안을 모두 채택했다. 내용은 [00-roadmap §5](00-roadmap.md)와 [01-design §0](01-design.md)의 D13–D16에 반영했다.

| 항목 | 결정 |
|---|---|
| Q1 비밀 키 | DPAPI 봉인(0600 키 파일 방식은 만들지 않음) |
| Q2 제외 후보 X-1–X-10 | 모두 제외 |
| Q3 앱 identifier | `io.github.jihoon22lee.devbox` |
| Q4 노트 vault 기본 위치 | `~/notes` |
| 실행 방식 | subagent-driven-development |

다른 세션이 이 계획을 추가로 검토한다. 그 결과는 이 문서에 새 절로 덧붙인다. → §8

## 8. 통합 검토 (2026-10-08) — 세 계획을 하나로

- 대상: 재구축 계획(이 폴더의 전신 `2026-10-07-devbox-rebuild/`), 그리고 같은 날 다른 세션이 만든 두 계획
  - CV(`2026-10-07-review-cross-validation/`): v0.9.0 리뷰 10건의 정적 교차 검증과 개발 방향
  - AR(`2026-10-07-astra-review/`): 리뷰 16건 + D01의 교차 검증, 격리 복사본에서 결함 RED 24건 실행 재현, 보정 과제 Q01–Q10, 제품 제안 U01–U07
  - 세 폴더는 이 폴더로 통합한 뒤 지웠다(§9.3). 결론·결함·입력값은 이 문서와 00-roadmap §4·§7에 있다.
- 기준: 사용자가 다시 밝힌 본질. Windows·WSL에서 AI 에이전트로 개발할 때 **에이전트 세션 관리·터미널·개발 도구를 한곳에 모아**, 각각 높은 품질·성능과 아주 쉬운 사용성을 제공한다.
- 방식: 세 계획의 문서를 모두 읽고, 계획이 기대는 사실을 `main@1b84aa9d` 코드와 이 PC 환경에서 다시 확인했다. 제품 코드 실행 검증은 하지 않았다. 아래 측정값은 조회 명령(`wc -l`, `systemctl`, `gh api`, `du`)의 결과다.

### 8.1 세 계획 비교

| | CV | AR | 재구축(이 폴더의 전신) |
|---|---|---|---|
| 성격 | 리뷰 소견의 정적 교차 검증 + 개선 방향 | 교차 검증 + 실행 재현(RED 24) + 보정·제품 과제 | 본질 리뷰 → WSL 데몬 + 얇은 Tauri 앱으로 다시 짓기, S0a–S6 |
| 권고 | 현 구조 유지 → 결함 수정 → 상태 책임 분리 → UX | 같음(정합성 보정 → 수명 보강 → 흐름 개선) | 다시 짓되, 확정 결함은 새 구조의 회귀 조건으로 |
| 강점 | 결함의 근본 원인 분류(미리보기·적용 시간차, 캡처 수명, 화면·작업 수명, 내부 데이터 분류, 실패 뒤 재시도) | 실제로 재현한 결함과 입력값, 기존 보호를 인정하는 좁은 범위 | 사용 빈도(A ≫ B = C ≫ E > D = F = G)에 맞춘 구조, tmux·systemd·hook으로 세션 유지·상태 감지, 검증 비용 상한 |
| 약점 | 실행 검증 없음 | 결함이 대부분 사용 빈도가 낮은 영역에 있어 본질을 움직이지 못함 | AR 결함 일부 누락, 첫 PR 머지를 막는 보호 규칙, 터미널 성능 기준 없음, 확인되지 않은 수치 |

### 8.2 방향 판단: 재구축을 뼈대로 하고 CV·AR을 흡수한다

CV·AR의 "현 구조 유지" 권고는 네 전제(기존 데이터 보존, Windows·WSL 대칭, 네 제품 경계, 현 검증 정책) 위에 섰다. 사용자는 D2·D7·D8·D9로 네 전제를 모두 바꿨다. 바뀐 전제에서 다시 보면 다음과 같다.

1. **본질의 중심이 v0.9.0에 없다.** 앱을 닫아도 유지되는 에이전트 세션, hook 기반 상태 감지, 입력 대기 알림, 격자 보기, 재개는 결함을 고쳐서 생기는 기능이 아니다.
2. **확정 결함은 본질 밖에 몰려 있다.** CV·AR의 고유 결함 약 20건 중 에이전트·터미널은 0건, 실행은 3건(AR-F02·F04·F15)이다. 나머지는 편집·Git·API·변환·기록·업데이트다. 이것을 먼저 고쳐도 사용 빈도 1위 영역은 그대로다.
3. **측정한 구조 비용.**
   - 설치·제품 연결·Windows 백그라운드 agent·WSL helper·Control Center 전용 코드가 약 7.6만 줄이다.
   - 각 엔진 안의 Windows·WSL 이중 경로 파일이 약 5만 줄이다.
   - 둘을 합치면 전체 약 49.7만 줄(Rust·TS, 테스트 포함)의 약 25%다. 파일 단위로 센 하한이며, 파일 안의 `cfg(windows)` 분기는 세지 않았다.
   - `.github` + `scripts`는 5.6만 줄, 워크플로는 6개다(새 설계 목표 1,500줄·3개).
   - 01-design §0.1과 ADR 0017 초안의 "코드의 약 40%"는 이 측정으로 확인되지 않아 측정값으로 고쳤다(IR-13). 판단은 바뀌지 않는다. 근거는 비율이 아니라 1·2와 사용자 결정이다.

**결정:** 이 폴더의 재구축 계획을 단일 최종 계획의 뼈대로 삼는다. CV·AR은 네 가지로 흡수한다.
- 결함 → 새 구조의 회귀 조건과 시험 위치: [00-roadmap §7](00-roadmap.md) 추적표, [01-design §9.1](01-design.md)
- 근본 원인 → 설계 전체의 정합성 규칙: 01-design §8.6
- 실행 재현 입력값 → 해당 과제의 시험 입력: 00-roadmap §7
- 재구축 동안 v0.9.0을 쓸 때의 회피 목록 보강: 00-roadmap §4

### 8.3 소견과 반영

| ID | 소견 | 판정 | 반영 위치 |
|---|---|---|---|
| IR-1 | **(실행 차단)** main 보호 규칙의 필수 검사는 옛 job 이름 셋이다(`Frontend (pnpm)`·`Rust (Cargo workspace)`·`Rust (Windows)`, 2026-10-08 `gh api` 조회). 새 CI의 job은 `rust`·`frontend`·`e2e`라 첫 PR(A)부터 머지가 막힌다. Windows 워크플로는 `paths` 필터라 필수로 두면 대부분의 PR이 영원히 기다린다 | 수용 | 03 Task 1: 집계 job `ci-ok`(`if: always()`, 모든 job 결과 확인)를 만들고 필수 검사를 `ci-ok` 하나로 바꾸는 단계. 03b Task 21: `ci-ok.needs` 갱신. 00 §3 |
| IR-2 | AR 결함 중 회귀 조건에 없던 것: AR-F01(승인한 Git 상태와 실행 상태 불일치), F02(서비스 health·Never 정책·유예), F04·F15(생성 성공 뒤 조회 실패로 중복 생성, 상세 캐시 고정), F06·F07·F08(API 초안 소실·결과 출처), F09(정규식 위치 단위), F10(빈 제목 색인), F12(늦은 조회가 수집 제어 표시를 덮음) | 수용 | 01 §9.1-12–17 신설, 00 §7 추적표, 각 S 과제의 시험 |
| IR-3 | `DocumentStore` 표가 Conflict→[디스크 것]·Recovering→[버림]·Clean 외부 변경에서 저널을 지우는지 정하지 않았다. 그대로 지으면 AR-F13(다시 읽은 뒤 복구본이 다음 시작에 다시 나옴)이 새 구조에서 재현된다. 같은 폴더 임시 파일과 vault `.trash/`의 색인 제외도 빠졌다(CV-F06 계열) | 수용 | 01 §8.4 표·§9 `documents`, 06 Task 2·3·4·7 |
| IR-4 | 서비스 준비(readiness)·health 정의가 없다. v0.9.0의 health 실패 재시작이 AR-F02 세 결함의 원천이었다 | 수정 수용 | health 실패로 재시작하지 않는다(재시작은 systemd가 프로세스 종료 때만). 준비 상태는 `ports_hint` 포트의 수신 관찰로 표시하고 서비스 의존 순서에만 쓴다. 01 §2.1·§4.3·§9.1-17, 05 Task 2·4·8 |
| IR-5 | 재시작 간격이 고정(`RestartSec`)이라 빠른 반복 종료에 backoff가 없다(AR-F14의 정책 질문). 이 PC의 systemd 259는 `RestartSteps`·`RestartMaxDelaySec`(254 이상)를 지원한다 | 수용 | 1초 → 30초 5단계, 수동 재시작은 한도 초기화. 01 §4.3·§9.1-10, 05 Task 4 |
| IR-6 | 터미널 성능 기준이 없다. §0.1은 "터미널 입력 지연을 v0.9.0과 수치로 비교"한다고 했지만 S0a는 브리지 echo만 잰다 | 수용 | 01 §1 SC10, 02 Task 4 측정과 결과 표 행, 04b Task 24 |
| IR-7 | E2E가 개발 서버와 같은 1450 포트(strictPort)를 써서, 사용자가 `pnpm dev`를 켜 둔 동안 E2E가 실패한다(이 PC에서 반복된 포트 충돌과 같은 유형) | 수용 | 03b Task 21: E2E는 화면 1460·gateway 1461 |
| IR-8 | 과제마다 worktree를 새로 만들면 공유 target-dir에 경로별 incremental 결과물이 쌓인다(2026-09에 590 GiB, 2026-10-08 현재 61 GiB) | 수용 | 00 §3 자원: 각 S 완료 때와 150 GiB 초과 때 `scripts/sweep.sh` |
| IR-9 | 이 PC의 `systemctl --user is-system-running`은 실패한 snap scope 때문에 `degraded`다. doctor·setup이 이를 실패로 보면 첫 실행이 막힌다 | 수용 | `running`·`degraded`를 정상으로 본다. 03 Task 15 Review Focus, 02 Task 8 |
| IR-10 | S0a 판정이 바뀌면 01-design만 고치게 되어 있어, 코드 수준으로 적힌 03·03b·04·04b가 옛 가정으로 남는다 | 수용 | 02 Task 8 Step 2 |
| IR-11 | 확인 대화상자가 "사용자가 본 상태"와 "실행할 상태"를 묶지 않는다(AR-F01의 일반형). 병합·폐기·push·pull·프로세스 종료가 모두 해당한다 | 수용 | 01 §5.2: `confirm = always` 메서드는 본 상태의 `expect`(rev·OID)를 받고, 다르면 `<domain>.state_changed`로 거절. 01 §8.6 |
| IR-12 | v0.9.0 회피 목록에 AR의 P1(Git push 대상 불일치, Never 정책 무시, 중복 생성)이 없다. 재구축 동안 v0.9.0을 몇 달 더 쓴다 | 수용 | 00 §4 |
| IR-13 | 01 §0.1·ADR 0017 초안의 "코드의 약 40%"는 측정으로 확인되지 않는다 | 수용 | §8.2의 측정값으로 고침(01 §0.1, 02 Task 8 ADR 초안) |
| IR-14 | v0.9.0의 긴급 수정 경로가 적혀 있지 않다 | 수정 수용 | 계획하지 않는다(버전 규칙). 데이터 손상급 결함이 새로 나오면 `v0.9.0` 태그에서 수정 브랜치를 만드는 길이 있다는 것만 00 §4에 적고, 실제로 할지는 그때 사용자에게 묻는다 |
| — | AR U01–U07(제품 흐름 제안) | 대부분 이미 반영 | U02 프로젝트 여정 = 전환기·시작 구성·문제→편집기(§8). U04 "저장 / 새 요청으로 저장" = 07 S5-3에 추가. U06 Control Center = 폐기(§2.7). U07 최소 창은 960×600을 유지한다(AR의 720×480은 따르지 않음. 개인 데스크톱용이고 1100px·760px 미만 자동 접기로 대응) |
| — | CV §3의 B·C·D 단계(상태 책임 분리·UX·실측 기반 교체) | 흡수 | 새 구조가 처음부터 그 형태(데몬 소유 상태·rev 규칙·DocumentStore·진행 작업 ID)로 짓는다 |

### 8.4 CV·AR 문서의 처리

- 두 계획의 결론은 이 폴더(08 §8, 00 §4·§7, 01 §8.6·§9.1)에 흡수했다.
- AR `evidence/review-tests.patch`는 v0.9.0 코드에 대한 시험이라 새 구조에 그대로 쓸 수 없다. 시험 입력값은 00 §7에 옮겼다. 원본은 v0.9.0 태그의 코드와 함께만 의미가 있다.
- 통합 뒤 사용자 지시로 세 원본 폴더를 지웠다(§9.3).

## 9. 검수 (2026-10-08, 2차) — 누가 이어받아도 실행할 수 있게

- 계기: 사용자가 "큰 작업이라 세션이 왔다갔다 할 수 있으니, 누가 해도 잘 작업할 수 있도록 꼼꼼히 검수"를 요청했다. 이어서 통합본 확정 뒤 원본 세 계획을 지우라고 했다.
- 방식: 계획 문서끼리의 이름·번호·파일 순서를 스크립트로 대조하고(만들기 전에 고치는 파일, Consumes가 가리키는 이름, 과제마다 커밋 여부, 남은 자리표시), 계획 속 셸 스크립트를 `bash -n`·`shellcheck`로 검사하고, 이 PC의 도구·버전·WSL·Windows 상태를 조회했다. 제품 코드는 실행하지 않았다.

### 9.1 소견과 반영

| ID | 소견 | 반영 |
|---|---|---|
| K-1 | 진행 상태를 한곳에서 볼 원장이 없고(상태 줄이 문서마다 흩어짐), 세션이 끊겼을 때 이어받는 절차가 없다 | `PROGRESS.md` 신설. 00 §3.1 인계 절차(시작: 원장·worktree·원격 브랜치·PR 대조, 끝: 커밋·원장·push·보고). 커밋 안 된 작업은 버리지 않고 patch로 보존 |
| K-2 | 계획 폴더가 저장소에 없어(추적 안 됨) 디스크 사고·`git clean`·다른 PC 세션에서 잃거나 못 본다. 원본 세 폴더를 지우면 유일한 사본이 된다 | 00 §2 **P0**(2026-10-08 완료): 사용자 요청대로 PR·CI 없이 `docs/v1-plan` 브랜치에 커밋·push했다(main은 PR 필수 보호라 직접 커밋 불가, 현 CI는 push에서 돌지 않음). PR A가 이 브랜치에서 갈라져 함께 main에 들어간다. S0a 결과도 이 브랜치에 직접 커밋. PR A 뒤 문서만 바꾼 PR은 새 CI의 `changes` job이 무거운 job을 건너뛴다 |
| K-3 | 실행 세션이 혼자 정하면 안 되는 것이 정해져 있지 않다 | 00 §3.2 "먼저 물을 것" 표, 혼자 정해도 되는 것 |
| K-4 | 사전 준비가 없다. 이 PC에 `cargo-sweep`이 없고, Windows 쪽에는 Rust·MSVC 빌드 도구·pnpm이 없는데 S0b 사용자 확인이 Windows 로컬 빌드(`win-build.ps1`)를 전제했다 | 00 §3.3 사전 준비 표. `scripts/dogfood.sh`(CI `windows.yml` 산출물을 받아 설치)를 기본 경로로 하고, S0b·S1·S2·S3 사용자 확인을 이 경로로 바꿨다. S0b Task 24는 PR을 먼저 열어 CI가 설치 파일을 만들게 한 뒤 확인·머지하도록 순서를 바꿨다 |
| K-5 | 검토 하위 에이전트의 기준과 막혔을 때의 처리가 없다 | 00 §3.4 검토 기준, §3.5 막혔을 때 |
| K-6 | 설계 원장과 코드 수준 계획이 어긋난다: 설계는 `rpc!` 매크로·생성 파일 두 개·메서드별 훅, 계획 코드는 `method!`·`topic!`·`gen/rpc.ts` 한 파일·공용 훅 | 01 §4.2·§5.2·§5.5·§8.6·§9를 계획 코드에 맞춤 |
| K-7 | 설계가 `Result<T, String>`을 전면 금지했는데 계획 코드 9곳이 쓴다(검토자가 되돌려 보낼 것) | 01 §5.3에 허용 범위를 명시(crate 안 helper, setup·doctor 단계, Windows 앱) |
| K-8 | 1차 통합 때 넣은 `StateChanged { head_oid, … }`가 `domain_error!`의 "세부 필드는 한 단어 소문자" 규칙을 어긴다 | `{ head, base, unmerged }`로 고침(04 Task 9, 04b 시험, 01 §5.2) |
| K-9 | 아직 없는 파일을 "Modify"하라는 과제가 있다: `crates/projects/src/config.rs`(04 Task 8), `crates/core/src/systemd.rs`(05 Task 4), `notifications/native.ts`·`daemon/system.rs`(04b), `Consent.tsx`·`src-tauri/src/activity.rs`(06 Task 9). S0b Task 13의 자리 파일·`SettingsPage.tsx`는 Files 목록에 없다 | Create/Modify를 바로잡고 목록에 넣음 |
| K-10 | 번호 오류: S4-1이 없는 S4-10을 가리키고, S5-9가 PR 묶음 표에 없다 | S4-2–S4-9, `chore/s5-wrap` 추가 |
| K-11 | 코드 블록 첫 줄 `# <경로>` 표시를 그대로 복사하면 셸 스크립트의 shebang이 둘째 줄이 된다 | 00 §3 "계획 문서의 코드 읽는 법" |
| K-12 | `scripts/sweep.sh`가 `cargo sweep`에 target 폴더를 넘기고(프로젝트 폴더를 받는 명령), 누적의 주원인인 incremental을 따로 다루지 않는다 | 저장소에서 `cargo sweep`, 3일 넘은 incremental 삭제, 다른 cargo가 돌면 중단. shellcheck 통과 |
| K-13 | 이 PC에는 `docker-desktop` 배포판이 함께 있어, "배포판 여러 개면 고르기"가 첫 실행마다 뜬다(SC1 위협) | 도구 내부용 배포판을 세지도 보이지도 않음(01 §6.5, 03b Task 23 `is_internal`과 시험) |
| K-14 | 에이전트 포트 묶음을 열린 작업끼리만 비교해, 다른 프로그램이 쓰는 포트와 겹칠 수 있다 | 묶음 안에 수신 중인 포트가 있으면 건너뜀(04 Task 8) |
| K-15 | 이 PC의 wsl-resource-guard는 사용자 scope 프로세스(에이전트가 여기서 돈다)를 화면에서 끝낼 수 있고, 커널 OOM도 있다. 새 설계는 이를 일반 실패로 보인다 | 종료 코드 128+신호 → `failed(killed:<신호>)`, "외부에서 종료됨" + [재개](01 §9.2, 04 Task 7, 04b Task 17) |
| K-16 | 시험 데몬은 임시 HOME으로 돌아 Git 신원이 없다. 데몬의 squash 병합·rebase 커밋이 로컬·CI 모두에서 실패한다. CI 러너에도 신원이 없다 | 임시 HOME에 `.gitconfig`(04 Task 8, 04b Task 19 E2E), CI `rust` job에 Git 신원과 tmux(04 Task 3) |
| K-17 | PR A의 "옛 파일 전부 삭제"가 P0로 저장소에 들어간 계획 폴더까지 지운다 | 삭제 제외 목록에 이 폴더, 복사 단계를 확인 단계로(03 Task 1) |
| K-18 | 남겨 두는 `.gitignore`의 `logs` 패턴이 어느 깊이의 `logs` 폴더든 무시한다 | `/logs/`로 바꿈(03 Task 1) |
| K-19 | 완료 과제들이 로드맵의 없는 "상태" 칸을 고치라고 한다 | `PROGRESS.md`로 바꿈(03b·04b·05·06·07) |
| K-20 | PR A 전에는 저장소 `AGENTS.md`가 v0.9.0용(하위 에이전트 모델 지정·무거운 검증)이라 실행 규칙과 충돌한다. 하위 에이전트 방식도 특정 하네스에 묶여 있다 | 00 §3: P0·S0a·PR A 동안은 이 계획 규칙이 우선(사용자 승인), 하위 에이전트는 하네스가 주는 것 |
| K-21 | CV·AR의 화면 제안 중 자리가 없던 것: 섹션별 오류 경계, rebase의 ours/theirs 표기, API 실행기 순서·생산 관계 미리보기와 변수 출처, 배율 150·200%·키보드만 쓰기 확인 | 01 §8.6, 03b Task 19, 07 S4-6·S5-3·S6-6 |

### 9.2 확인한 사실 (2026-10-08, 이 PC)

- 계획이 고정한 버전은 모두 실제로 있다: Rust 1.98.1(+musl 대상 설치됨), ts-rs 12.0.1, tokio 1.53, tauri 2.12, rusqlite 0.32, portable-pty 0.9, windows 0.62, Vite 7·Vitest 4·TS 5.8 범위(최신은 Vite 8·Vitest 5·TS 7이지만 계획은 v0.9.0에서 검증된 판을 쓴다).
- WSL: systemd 259(`RestartSteps` 지원), tmux 3.6, claude 2.1.288, codex 0.161.0, Node 24.18, pnpm 9.15.9, `gh` 인증됨, 배포판 `Ubuntu`(기본) + `docker-desktop`, UID 1000, linger 꺼짐, user manager PATH에 `~/.local/bin` 없음.
- main 보호 규칙 필수 검사: `Frontend (pnpm)`·`Rust (Cargo workspace)`·`Rust (Windows)`(IR-1).
- 이 PC에서 수신 중인 포트(3000·5432·8000–8002·8080·8765 등)는 계획의 1450·1451·1460·1461·20000번대와 겹치지 않는다.

### 9.3 원본 세 계획의 삭제

통합본(이 폴더)이 원본 세 폴더의 내용을 모두 담는지 대조한 뒤 지웠다.
- 재구축 계획 `2026-10-07-devbox-rebuild/`: 이 폴더가 그 문서 전체의 사본에 §8·§9의 수정을 더한 것이다.
- CV `2026-10-07-review-cross-validation/`: 결함 F01–F10 → 00 §7, 회피 → 00 §4, 근본 원인 → 01 §8.6, 화면 제안 → K-21, 방향 판단 → 08 §8.2.
- AR `2026-10-07-astra-review/`: 결함 F01–F16·D01과 실행 재현 입력값 → 00 §7, 회피 → 00 §4, Q01–Q10 → 01 §9.1, U01–U07 → 08 §8.3·K-21. 실행 증거(`review-tests.patch` 등)는 v0.9.0 코드 대상이라 옮기지 않았다.
