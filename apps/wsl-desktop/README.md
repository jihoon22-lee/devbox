# wsl-desktop — WSL Desktop

앱 안에 내장된 임베디드 WSL 터미널. Windows Terminal처럼 탭·분할로 여러 WSL 세션을 관리한다.
산출물: `WSLDesktop.exe` (`apps/wsl-desktop`).

## 주요 기능

- **임베디드 터미널** — xterm.js + PTY(ConPTY), WSL 배포판 선택·지정 경로로 열기
- **탭 + 분할** — 탭 안에 격자/가로/세로 분할, 팬 사이 구분선을 끌어 크기 조절(더블 클릭 또는
  `Home`으로 균등 복원, 방향키로 미세 조절), 활성 팬만 탭 전체에 보이는 확대 토글, 드래그로
  탭 이동·재배치, 단축키 전환. 팬 구성·순서·레이아웃이 바뀌면 이전 비율은 균등 분할로
  무효화되고, 임시 확대만 켰다 끌 때는 그 전 비율을 그대로 복원한다.
  탭 바는 `tablist`/`tab` 의미를 가지며 좌우·Home·End로 이동하고 활성 탭을 항상 시야 안으로
  스크롤한다. 가운데 클릭과 `Delete`로 닫고 더블 클릭으로 이름을 바꾼다. 눈에 보이는 ✕는
  마우스 전용 보조 수단이라 접근성 트리에서 감추며, 키보드·보조기술은 컨텍스트 메뉴·
  `Delete`·`Ctrl+Shift+W`로 같은 동작에 도달한다.
- **팬 이동** — `Alt+방향키`는 목록을 순환하지 않고 현재 레이아웃에서 실제로 그 방향에 있는
  팬으로만 이동한다. 격자 가장자리에서는 움직이지 않는다.
- **클립보드** — 선택 자동 복사(기본 켬, 설정 저장), `Ctrl+Shift+C/V`, 가운데 버튼
  붙여넣기. `Ctrl+C`는 선택 유무와 무관하게 항상 셸의 SIGINT로 남고, 개행이 든
  붙여넣기는 내용 대신 줄 수만 표시해 확인한다. 단일 paste는 최대 1,000,000자다.
- **앱 내장 확인·입력 창** — 모든 확인과 이름 입력은 native `confirm`/`prompt` 대신 앱 안의
  대화상자를 쓴다. 테마·`Esc` 취소·IME·focus 복원(연 곳으로 정확히 되돌림)을 앱의 다른
  대화상자와 공유하고, 요청이 겹치면 순서대로 하나씩 묻는다. 대화상자가 열린 동안 뒤쪽 앱·
  터미널 단축키는 실행되지 않는다. 실행 직전 최종 문자열은 monospace 블록으로 그대로 보여 준다.
- **렌더러** — 공식 `@xterm/addon-webgl`을 별도 chunk로 나중에 불러와 대량 출력과 전체 화면
  TUI를 GPU로 그린다. chunk 로딩·컨텍스트 생성 실패나 이후 컨텍스트 손실에서는 조용히 DOM
  렌더러로 남으며 터미널·PTY 연결·스크롤백은 영향을 받지 않는다.
- **버퍼 명령** — 스크롤백 비우기, 맨 아래로 이동, 전체 선택을 팬 메뉴와 명령 팔레트에서
  실행한다. 벨 문자는 소리 대신 팬 머리글 배지로 알린다.
- **검색·메타데이터·링크** — 팬별 스크롤백 검색(`Ctrl+Shift+F`, Enter/Shift+Enter,
  대/소문자·단어 단위·정규식 옵션과 버퍼 전체 일치 강조),
  OSC 0/2 제목과 OSC 7 현재 cwd, OSC 8·일반 HTTP(S) 링크를 지원한다. 자동 탭 제목은
  활성 팬을 따르지만 사용자가 바꾼 탭 이름은 OSC가 덮어쓰지 않는다. 링크는 scheme과
  자격 증명을 검사하고 host 확인 뒤 기본 브라우저에서 연다. host 확인에서 "이 창에서는 이
  host를 다시 묻지 않기"를 고르면 그 창이 살아 있는 동안만 같은 host를 다시 묻지 않으며,
  이 선택은 저장하지 않는다. 검색어는 최대 512자다.
- **팬·탭 컨텍스트 메뉴** — 팬 복사·붙여넣기·검색·세로/가로 분할·cwd 복사·확인 후
  닫기, 탭 닫기·다른 탭 닫기·이름 변경·레이아웃 전환. 우클릭과
  `Shift+F10`/Menu 키를 지원하고 닫힌 뒤 실제 터미널로 focus를 복원한다. 복사와 cwd
  복사는 우클릭한 exact 팬에 selection 또는 유효한 OSC 7 값이 있을 때만 활성화된다.
- **글꼴·스크롤백·resize** — 글꼴 크기 `Ctrl++/-/0`과 툴바 조절값을 저장하며 xterm을
  재마운트하지 않는다. ConPTY wrap 보정, resize ack 후 commit·실패 재시도, hidden pane/최소
  크기 보호를 적용한다.
- **설정** — 툴바 `설정`과 명령 팔레트에서 연다. 팬 하나 닫기 확인 여부, 시작할 때 터미널
  하나 열기, 세션 유지 방식, 선택 시 자동 복사, 글꼴 크기, 글꼴(고정 목록), 색 테마(어두움/
  밝음/고대비), 커서 모양·깜박임, 스크롤백 줄 수(1,000~100,000, 기본 10,000)를
  `localStorage`에 저장한다. 툴바에는 자주 쓰는 조작만 남기고 나머지는 이 창으로 모았다. 글꼴은 임의 문자열을 받지
  않고 앱이 가진 목록에서만 고르며, 값이 손상되면 그 필드만 기본값으로 되돌린다. 변경은
  xterm 옵션만 갱신하므로 스크롤백과 PTY 연결이 유지된다.
- **시작 상태** — 복원할 레이아웃이 없고 배포판 조회에 성공했으면 기본 배포판 터미널을
  하나 연다(설정으로 끌 수 있음). 수집이 실패한 상태에서는 열지 않는다. 마지막으로 선택한
  세션 유지 방식과 사이드 패널 열림 상태도 복원하며, 저장된 유지 방식을 현재 배포판이
  제공하지 않으면 조용히 native로 되돌린다. 등록된 배포판이 0개인 정상 snapshot에서도 이름을
  추측해 시작하지 않고 터미널 생성 UI를 비활성화한다.
- **팬 상태 표시** — 팬 머리글에 실제로 시작된 유지 방식(native가 아닐 때)과 기존 세션에
  다시 붙었을 때의 `재연결됨` 배지를 보여 준다. 요청한 방식이 backend에서 native로 낮춰졌는지
  화면에서 바로 확인할 수 있다.
- **워크스페이스·프로필** — stable pane key로 마지막 탭/팬/distro/cwd/layout/시작 명령을
  복원하고, 현재 구성을 이름 있는 터미널 프로필로 저장한다. `OpenTarget::Profile` cold/hot
  요청은 같은 전환 경로를 사용한다. 시작 명령은 실행 전에 최종 문자열을 확인하고 새 세션에
  한 번만 보낸다.
- **Launcher profile snapshot producer (#487)** — 검증된 터미널 프로필의 opaque ID·안전한
  label·고정 detail·`{id}` target payload만 `wsl-desktop/v1/profiles.json` named view로
  발행한다. distro·cwd·시작 명령·pane 구성·경로·secret은 snapshot에 넣지 않으며, primary
  profile store의 유효한 읽기 또는 저장/삭제 성공 뒤 publication은 best-effort다. 손상된 profile store는 빈 값으로
  덮어쓰지 않고 mutation을 거부한다(읽기 명령은 빈 목록을 반환할 수 있지만 기존 바이트와
  last-good named snapshot은 보존한다).
- **명령 팔레트** — `Ctrl+Shift+P`에서 탭 열기·닫기·이름 변경·레이아웃 전환, 팬 분할·닫기·
  검색·복사·붙여넣기·cwd 복사, 배포판별 터미널 열기, snapshot 새로 고침, 프로필 저장·전환,
  설정과 단축키 안내를 키보드로 실행한다. 같은 동작에 단축키가 있으면 항목 옆에 함께 보여
  준다.
- **단축키 안내** — 툴바 `단축키`와 팔레트에서 연다. 목록은 실제 matcher가 쓰는 표에서
  나오며, 회귀 테스트가 표에 적힌 조합을 matcher에 그대로 넣어 확인하므로 안내와 동작이
  어긋나지 않는다.
- **동시 입력(broadcast)** — 기본 OFF. 활성 탭의 팬을 최소 2개, 최대 32개까지 직접 선택하고 대상 수를
  확인해야 켤 수 있다. 켜져 있는 동안 대상 팬은 머리글 배지와 테두리로 표시되고 접근성
  이름에도 `동시 입력 대상`이 붙어, 입력이 어디로 가는지 팔레트가 아니라 터미널 격자에서
  바로 보인다. 여러 줄 붙여넣기와 위험 명령 Enter는 대상 수와 실행 위험을 다시
  확인한다. 셸 redirection(`<`, `>`, `<<`, `>>`)은 공백이 없어도 위험 명령으로 분류한다.
  취소한 위험 명령은 다음 Enter에서도 재확인한다. 확인 창이 열려 있는 동안 들어온 입력은
  버리지 않고 도착 순서대로 최대 256 chunk까지 대기시켰다가 확인 뒤 같은 순서로 보낸다.
  단, 확인 중 대상·순서 또는 무장 상태가 바뀌면 그 확인과 대기 입력은 이전 대상에 보내지 않고
  폐기한다.
  resource/session snapshot 수집이 실패했거나 마지막 정상 snapshot이 TTL을 넘겼거나 대상 팬
  구성이 바뀌면 동시 입력은 자동으로 OFF/fail-closed가 된다. 새 collection이 진행 중이라는
  사실만으로는 끄지 않는다 — snapshot은 distro별 개수만 담고 대상 세션의 정체성은 담지
  않으며 backend가 보유하지 않은 세션의 broadcast를 스스로 거부하므로, 진행 중이라는 이유로
  끄는 것은 안전을 더하지 않고 TTL 주기마다 사용자가 켜 둔 상태만 되돌린다. 일반 팬의 단일
  입력·PTY I/O는 어느 경우에도 계속 사용할 수 있다.
- **선택적 프로세스 유지** — native workspace는 외부 도구 없이 완전하게 동작한다. 이미
  설치된 tmux/zellij만 감지해 stable `wsld-*` 세션에 opt-in attach하며, 설치·download하지
  않고 부재/감지 실패 시 native로 폴백한다. 감지는 login shell이나 rc 파일을 실행하지 않고
  선택 distro 사용자의 `HOME`·`PATH`만 제한적으로 조회해 `~/.local/bin`, `~/.cargo/bin`과
  고정 system bin 후보를 검사한다. 결과는 사용 가능·없음·확인 오류를 구분하며 실제 절대
  실행 경로는 renderer, 로그, 프로필에 노출하지 않는다.
- **상태 패널** — WSL 배포판과 선택 distro의 Docker 컨테이너를 표시한다. 260px의 좁은
  패널에서도 이름·정규화 상태·축약 port mapping을 먼저 보여 주고, 컨테이너를 펼치면 Docker가
  반환한 ID·image·status·ports 원문을 확인하고 start/stop/restart할 수 있다. Docker가 없으면
  설치 안내만 표시하며 engine 설치·설정·리소스 관리는 수행하지 않는다. 같은 dashboard
  snapshot에서 distro별 CPU 사용률·memory used/total·disk used/total과 active terminal 수를
  함께 표시하고, 마지막 정상 결과·stale 상태를 명시한다.
- **runtime snapshot producer** — Workbench와 Life Log가 별도 앱 전환 없이 현재 상태를
  사용할 수 있도록, 이미 실행 중인 distro만 `wsl-desktop/runtime/v1` read-only snapshot으로
  발행한다. distro별 실행 중 terminal 수와 Docker availability, bounded container 목록,
  검증된 published port mapping을 제공하며 stopped distro를 조회 때문에 시작하지 않는다.
- **Log Lens handoff producer** — 선택한 distro 카드에서 사용자가 확인한 경우에만 WSL file 또는
  journal source를 `log-source/v1` one-time envelope으로 보낸다. file payload는 `sourceType`,
  검증된 distro와 절대 WSL 경로(`wslPath`)만, journal payload는 `sourceType`, distro와 제한된
  unit만 포함한다. AppLink argv에는 opaque handoff kind/id만 들어가며, shell·arbitrary WSL
  command·환경변수·credential·로그 원문·clipboard fallback은 사용하지 않는다. `wslPath`는
  10분 TTL의 one-time pending envelope에만 잠시 존재하며 DB/localStorage/saved view/clipboard와
  AppLink argv에는 복사하지 않는다. Log Lens에서 preview를 확인하고 `읽기 전용 source 추가`를
  눌러야 claim이 ack되고 fixed adapter가 시작된다.
- publish와 launch는 producer single-flight로 보호하며, 이미 진행 중인 handoff에는 고정
  `handoff-busy` 오류를 반환한다. Log Lens는 producer/source-family와 lease를 다시 검증하고,
  journal의 선택적 `unit`이 없는 경우도 포함해 동일한 receiver contract로 변환한다. launch
  실패 시에는 방금 만든 exact pending envelope을 안전하게 제거하며, raw payload·경로·로그
  원문은 오류에 노출하지 않는다. 이 범위는 WSL Desktop producer와 Log Lens claim/preview
  lifecycle에 한정된다. Run log를 읽는 Log Lens receiver adapter는 #473에서 완성되어
  v0.5.1 stable에 포함됐지만 v0.5.0 binary에는 없는 maintenance correction이다.
- **open path 핀·최근 경로** — 자주 쓰는 작업 경로 저장 (최근 12개)

## 기술

- `portable-pty` 기반 ConPTY (PTY resize, 탭 모델, 드래그와 앱/터미널 단축키)
- 공용 `packages/context-menu` — viewport 배치·keyboard navigation·focus 복원·submenu를
  공유하고, WSL 전용 항목·exact pane/tab 대상·danger 확인은 앱이 소유한다.
- 공식 xterm MIT addon(`addon-search`, `addon-web-links`, `addon-webgl`)과 Tauri clipboard
  plugin을 앱에 포함하므로 설치 뒤 검색·링크 감지·붙여넣기·GPU 렌더링은 network나 별도 외부
  도구 없이 동작한다. `addon-webgl`은 dynamic import로 분리해 초기 번들에 넣지 않는다.
  clipboard capability는 읽기 텍스트 하나만 허용한다.
- tmux/zellij 어댑터는 shell 문자열 조립 없이 exact argv만 사용한다. tmux UI option은 해당
  session에만 적용하고 zellij는 내장 `disable-status` layout과 frame/mouse off option을
  사용해 앱의 탭·팬 UI와 xterm selection을 유지한다. 세션 시작 때마다 실행 파일을 다시
  해석하고 version probe, 기존 세션 조회, 실제 PTY launch에 같은 검증된 절대 경로를 사용한다.
- 공용 크레이트 `crates/wsl` — 프로세스를 실행하지 않는 WSL 공용 프리미티브로, `wsl.exe` 실행
  argv(`--cd` 포함)·`wslpath` argv 조립, distro 이름 검증, WSL 출력 디코딩, Windows↔WSL 경로와
  canonical project key 정규화를 제공한다.
- Docker 목록은 기본 공백 table을 추측해 파싱하지 않고 `docker ps -a --no-trunc --format`으로
  ID/name/image/status/ports 다섯 필드만 요청한다. 요약용 상태·port만 frontend에서 파생하며 원문
  필드는 변경하거나 저장하지 않는다. Docker query 자체는 COMMAND·환경 변수·credential·resource
  summary를 조회하지 않는다.
- dashboard/runtime producer는 하나의 고정 collection에서 `wsl.exe -l -v`로 distro·state를
  확인하고, Running distro에만 다음 read-only argv를 순차 실행한다:
  `wsl.exe -d <validated-distro> -- docker ps -a --no-trunc --format
  '{{.ID}}\\t{{.Names}}\\t{{.Image}}\\t{{.Status}}\\t{{.Ports}}'`,
  `/proc/stat`, `/proc/meminfo`, `df -P -B1 -- /`. 모든 값은 별도 argv 요소로
  전달하며 shell/`bash -lc`/사용자 command/환경 확장·외부 설치를 사용하지 않는다. child stdin은
  닫고 stdout·stderr는 bounded reader와 child 5초 timeout, 전체 collection 30초 deadline으로 처리한다. stderr는 오류 분류에
  반영하지 않고 decode·log·IPC·snapshot에 노출하지 않는다. dashboard command와 60초
  background writer는 같은 collection lock·revision을 공유하고, 수집 실패 시 last-good을
  보존한다.
- WSL 기준선은 `wsl.exe --cd <cwd>`를 지원하는 최신 Microsoft Store WSL이다. 구형 inbox WSL은
  `wsl --update`로 먼저 업데이트하는 것을 권장한다. WSL2는 필요하면 `wsl --install` 후 재부팅하며,
  컨테이너 패널에는 선택 distro에서 실행 가능한 Docker CLI와 engine이 필요하다. devbox가 이를
  자동 download하거나 설치하지 않는다.

## 데이터

- 프로젝트·git 상태는 Workbench로 이관됨 (`com.devbox.workbench\project-profiles.json`)
- `localStorage`: cwd 핀·최근 경로 5개, selection 자동 복사 여부, 터미널 글꼴 크기, version 1
  설정(확인 동작·시작 동작·사이드 패널·유지 방식·글꼴 id·테마·커서·스크롤백), version 1
  마지막 레이아웃. 팬 크기 비율과 확대 상태는 창이 살아 있는 동안만 유지하며 저장하지 않는다. 터미널 출력·selection·clipboard 내용과 runtime session id는 저장하지 않는다.
  링크 host의 "다시 묻지 않기" 선택은 process memory에만 두며 저장하지 않는다.
- Docker 컨테이너 목록과 detail 원문은 runtime memory에만 두며 localStorage나 profile에 저장하지
  않는다.
- 공용 integration snapshot은 `%LOCALAPPDATA%\\devbox\\integration\\wsl-desktop\\v1\\summary.json`
  runtime view와 `%LOCALAPPDATA%\\devbox\\integration\\wsl-desktop\\v1\\profiles.json`
  named profile view를 소유한다. profile view에는 opaque profile ID·안전한 label·고정 detail과
  `{id}` payload만 들어가며 distro·cwd·시작 명령·pane 구성·경로·secret은 포함하지 않는다.
  envelope의 `data.views.runtime`에는 다음처럼 공개에 필요한 최소 필드만 들어간다.

  ```json
  {
    "schemaVersion": 1,
    "producer": "wsl-desktop",
    "producerVersion": "0.3.3",
    "generatedAt": "2026-08-26T00:00:00Z",
    "data": {
      "views": {
        "runtime": {
          "schemaVersion": 1,
          "freshnessMs": 0,
          "entries": [{
            "id": "Ubuntu",
            "name": "Ubuntu",
            "state": "running",
            "terminalCount": 1,
            "dockerAvailability": "available",
            "containers": [{
              "id": "0123456789abcdef",
              "name": "api",
              "state": "running",
              "portMappings": [{ "published": 8080, "target": 80, "protocol": "tcp" }]
            }]
          }]
        }
      }
    }
  }
  ```

  distro `id`는 WSL에 별도 숫자 ID가 없으므로 검증된 등록 이름과 동일한 안정 key다. `state`는
  현재 producer가 조회한 실행 중 상태이며, container state는 `created`, `dead`, `exited`,
  `paused`, `removing`, `restarting`, `running`, `unknown` 중 하나다. Docker의 image, raw
  status/ports, command, labels, mounts, environment, volume/socket/path와 terminal session
  id, cwd, title, profile command는 snapshot에 들어가지 않는다.

  화면 전용 `dashboard_snapshot` IPC 응답은 위 integration view와 같은 collection generation을
  사용하며, 여기에만 distro `version`, `default`, stopped/running `state`, active
  `terminalCount`, Docker detail과 다음 numeric resource summary를 포함한다:

  ```json
  {
    "revision": 4,
    "capturedAtMs": 1725000000000,
    "staleAfterMs": 30000,
    "distros": [{
      "name": "Ubuntu",
      "version": 2,
      "default": true,
      "state": "Running",
      "terminalCount": 1,
      "dockerAvailability": "available",
      "resource": {
        "cpuPercent": 18,
        "memoryUsedBytes": 4194304,
        "memoryTotalBytes": 8388608,
        "diskUsedBytes": 10485760,
        "diskTotalBytes": 20971520
      }
    }]
  }
  ```

  이 IPC 응답은 process memory에만 존재하며 localStorage/profile/integration envelope에 저장하지
  않는다.
- producer bounds는 distro 64개·이름 128 bytes, distro당 container 256개·전체 512개,
  container ID 64 bytes hex·이름 256 bytes, container당 port mapping 32개·전체 1,024개,
  distro당 terminal 256개, Docker stdout 4MiB·line 16KiB·stderr 64KiB다. resource 명령별
  stdout는 64KiB이며, CPU는 연속해서 성공한 `/proc/stat` aggregate counter 두 개의 delta만
  0~100%로 표시한다. 첫 표본·counter reset은 거짓 0% 대신 `null`/`—`로 표시하고, distro가
  중지되거나 제거되면 이전 표본을 폐기한다. memory/disk byte는 JavaScript safe integer와 checked
  arithmetic 상한을 따른다. 최종 envelope는 공용 `crates/integration`의 10MiB 상한도 통과해야 한다.
- snapshot은 완성된 envelope만 `crates/integration::write_atomic`으로 교체한다. WSL/Docker
  timeout(5초), child I/O/출력 상한, malformed row/identity/privacy 검증 실패는 빈 결과나
  부분 결과로 덮어쓰지 않고 직전 last-good 파일을 보존한다. Docker exit 127은 `missing`,
  기타 non-zero 종료는 `error`, 성공한 빈 출력은 `available` + 빈 목록으로 구분한다.
- 앱 시작은 60초 주기 writer를 만들고, renderer도 snapshot TTL(정상 30초, 5~60초 clamp)에
  맞춰 single-flight refresh한다. dashboard의 성공적인 refresh와 terminal
  start/close/reader cleanup은 250ms debounce trigger를 공유한다. producer당 단일 worker와
  dashboard command의 collection lock이 동시 trigger/수동 refresh를 합치며, 수집은 distro별
  순차 실행으로 고정한다. 화면은 snapshot revision 하나의 distro/resource/Docker/terminal
  결과만 사용한다. Docker mutation과 broadcast는 마지막 정상 snapshot이 TTL 안에 있고 수집이
  실패하지 않았을 때 사용할 수 있으며, error·만료·Docker/workspace action 진행 중에는
  fail-closed한다. 진행 중인 collection은 그 자체로 조작을 막지 않는다.
  snapshot 갱신 실패가 terminal I/O나 기존 read-only Docker panel display를 막지는 않는다.
- `app_local_data_dir/terminal-profiles.json`: version 1 이름 있는 터미널 프로필. atomic replace,
  탭 16개·팬 32개·한 줄 시작 명령 4,096자 제한, 참조 무결성·안전한 절대 cwd·명백한 평문
  credential 검증을 적용한다.
- profile store가 corrupt/invalid이면 기존 바이트를 보존하고 profile mutation을 실패시킨다.
  missing일 때만 빈 store를 시작하며, 유효한 저장/삭제 뒤 named profile snapshot을 best-effort로
  다시 발행한다.

## 개발

- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: [`docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`](../../docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md)
