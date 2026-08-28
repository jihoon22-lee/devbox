# Architecture

devbox는 **모노레포 + 다중 독립 앱** 구조를 취한다.

## 핵심 원칙

1. **하나의 저장소, 여러 독립 앱** — 각 앱은 독립적으로 실행되고 독립적으로 `.exe`를 만든다.
   모노레포는 개발 코드 관리 방식일 뿐 앱을 합치는 방식이 아니다.
2. **공통 코드는 실제 필요해졌을 때만 추출** — 첫 앱은 앱 안에 코드를 두고,
   두 번째 앱에서 같은 코드가 필요해지는 순간 `crates/`·`packages/`로 옮긴다.
3. **WSL에서 개발, Windows에서 빌드** — 순수 로직은 WSL에서 테스트하고,
   Tauri 앱 실행/배포는 Windows 툴체인으로 한다.

## 레이어

```
┌──────────────────────────────┐
│ apps/*   독립 Tauri 앱 (.exe) │  15개
├──────────────────────────────┤
│ packages/*  React 공용       │  tokens, editor, diff-view, context-menu
├──────────────────────────────┤
│ crates/*    Rust 공용        │  filesystem, markdown, process, wsl,
│                              │  search, integration, secrets, git, launch,
│                              │  applink, catalog, window-state,
│                              │  window-state-tauri
├──────────────────────────────┤
│ 공통 인프라: Cargo workspace, │
│ pnpm workspace, git 모노레포,  │
│ apps/catalog.json (앱 단일 원본)│
└──────────────────────────────┘
```

위 그림은 v0.5.0 개발 중인 현재 구조다. 기존 동작을 유지하면서 계획 요소를
순차적으로 추가한다. 구현 전인 항목은 현재 앱/크레이트 수에 포함하지 않는다.

- 신규 독립 앱 `devbox-launcher`·`log-lens` bootstrap 구현 — 현재 15개 앱. Log Lens의
  Run/WSL producer handoff는 후속 integration PR이다.
- 구현된 순수 `crates/catalog` — catalog v1/v2 type·revision freshness·runtime/build-time
  fallback·capability filter. runtime file I/O는 후속 Manager 기능이 담당한다.
- 신규 `crates/logs` — Log Lens가 두 번째 소비자가 되는 시점의 순수 log parsing
- 구현된 `packages/context-menu` — 위치·keyboard navigation·focus restore·submenu·separator·
  disabled/danger 표현만 소유한다. Port Manager, Developer Toolbox, Everything+, Knowledge, Code Pad,
  Run Manager, Devbox Manager, Workbench, Webhook Lab, Repo Manager, API Playground, WSL Desktop, Life Log의
  기존 13개 앱에 기능 단위로 적용됐다. 신규 앱은 처음부터 적용한다.
- 구현된 순수 `crates/window-state` — bounds/maximized/monitor identity/scale을 bounded
  JSON으로 보존하고 monitor·DPI 변화 시 visible titlebar가 남도록 복원 geometry를 계산한다.
  파일 I/O와 Tauri/Windows monitor adapter는 소비 앱이 소유하며 transient window는 제외한다.
- 구현된 `crates/applink` protocol v2 one-time handoff — argv에는 kind와 opaque 128-bit id만
  전달하고, bounded payload는 공용 data root 아래에서 atomic claim/ack/restore와 60초 lease로
  한 번만 소비한다. producer/consumer UI는 각 integration PR이 소유한다.

상세: [`v0.5.0 네이티브 우선 계획`](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)

## 크레이트 의존 관계

```
  crates/filesystem ◄── applink, api-playground, code-pad, developer-toolbox, devbox-manager,
                       everything-plus, knowledge-base, life-log, port-manager,
                       repo-manager, run-manager, wsl-desktop, devbox-launcher, window-state-tauri
  crates/applink    ◄── code-pad, knowledge-base, life-log, repo-manager, wsl-desktop, workbench,
                       run-manager, devbox-manager, devbox-launcher
  crates/markdown   ◄── knowledge-base, code-pad
  crates/process    ◄── port-manager, run-manager
  crates/wsl        ◄── port-manager, wsl-desktop, run-manager, workbench, repo-manager
  crates/search     ◄── everything-plus, knowledge-base
  crates/integration◄── run-manager, workbench, knowledge-base, life-log, devbox-launcher 등 snapshot 계약·자동 발견
  crates/secrets    ◄── api-playground, run-manager (DPAPI)
  crates/git        ◄── devbox-manager, life-log, repo-manager, workbench
  crates/launch     ◄── everything-plus, knowledge-base, life-log, repo-manager, workbench, devbox-launcher
  crates/catalog    ◄── devbox-manager, devbox-launcher
  crates/window-state ◄── window-state-tauri
  crates/window-state-tauri ◄── 14개 persistent 앱의 `main` window (#323–#336)
  apps/log-lens     ◄── wsl (fixed WSL argv validation; parser remains app-local until a second consumer)
```

### `crates/window-state` 선행 계약

`WindowState`의 v1 JSON은 physical-pixel `bounds`, 저장 당시의
`monitorId`·`monitorWorkArea`, `scaleFactor`, `maximized`를 포함한다. 문서는 16 KiB,
monitor identity는 256 UTF-8 bytes, scale factor는 0.5–8.0, 좌표·dimension은 bounded
geometry로 제한하고 unknown field와 future schema를 거부한다. `encode_state`/`decode_state`는
파일을 읽거나 쓰지 않으며, 각 앱은 `crates/filesystem` 등의 atomic storage 경계와 앱별
설정 경로를 계속 소유한다.

`restore_window`는 현재 monitor identity를 우선 매칭하고, 제거된 경우 primary → 첫 유효
monitor로 fallback한다. 저장 work area와 현재 scale factor를 사용해 relative position과
size를 physical pixels로 변환한 뒤, titlebar의 최소 가시 영역을 남기도록 clamp한다.
손상·과대 문서는 `restore_from_bytes`가 고정 오류 없이 기본 bounds로 fail-closed하며,
Launcher/dialog/splash 같은 transient window는 이 계약의 소비 대상이 아니다.

### `window-state-tauri` cross-app adapter (#323–#336)

`crates/window-state-tauri`는 Tauri의 physical `outer_position`/`inner_size`와 monitor
snapshot을 위 순수 계약에 연결하고, 앱별 `app_local_data_dir/window-state-v1.json`에
`crates/filesystem::atomic_write`로 저장한다. `main` label만 처리하므로 dialogs, file
pickers, splash, child windows는 state를 만들지 않는다. moved/resized/DPI 이벤트는 단일
bounded debounce slot을 사용하고 close/programmatic exit는 flush한다.

현재 #323–#336의 14개 persistent 앱이 모두 manifest dependency, setup restore,
window-event save를 갖는다. Life Log와 Run Manager는 저장을 hide/prevent-close보다 먼저
수행하고, Life Log·Run Manager·Code Pad의 programmatic exit은 `app.exit` 직전 explicit
save를 유지한다. Log Lens도 persistent `main`만 같은 adapter를 사용한다. 현재 picker와
export는 별도 Tauri window가 아닌 native/browser dialog지만, label filter가 향후 추가될
transient picker/export/preview window도 state 파일에서 배제한다.

15번째 앱인 Devbox Launcher의 palette는 transient이므로 adapter를 설치하지 않는다.
통합 선행 관계는 `#322 window-state 계약 → #321 Log Lens bootstrap → #323–#336 wiring`으로
완료됐고, #323–#336은 같은 monitor/DPI 회귀 행렬을 공유하는 하나의 사용자-visible 기능
묶음으로 적용한다. monitor 제거/DPI/해상도 축소/virtual-origin 변경,
corrupt·oversized/future schema, atomic write 실패 보존은 pure fixture와 packaged W4
smoke에서 공통 회귀 행렬로 검증한다.

## 앱별 데이터 흐름

```
port-manager:    React → invoke → bounded commands → native netstat / wsl.exe
                   ├ Windows process handle (PID + creation FILETIME) → identity re-check → terminate
                   ├ WSL ss + /proc stat/cmdline (distro + PID + start tick) → identity re-check → SIGTERM
                   ├ Docker published port → validated WSL Desktop stop handoff (never process kill)
                   ├ bounded single-flight refresh → stable snapshot → identity-based
                      new/closed/changed diff; failed polls retain rows/favorites and lock kill
                   └ app-local strict preferences (interval, pinned, port/process identities)
                      → atomic JSON replace (no path/command/secret fields)
wsl-desktop:     React(xterm + pane/tab context-menu) → invoke → commands → wsl crate → wsl.exe
                   (wsl-dashboard 흡수; split/close/tab action은 exact ID와 확인 경계 사용)
                   └ distro·docker 패널 (`docker ps --format` 5필드 원문 → compact summary/detail,
                      gitStatus는 Workbench로 이관 완료)
life-log:        tray/poller(상시) → sessionizer → SQLite → bounded digest/export commands → React(date context-menu)
                   (activity-timeline 흡수. digest는 authoritative local civil-day boundaries,
                   shared privacy/snapshot rules와 safe Git collector를 사용하고 DB progress
                   cancellation + single-flight guard를 native 작업까지 전달한다. day 화면은
                   digest 한 번의 Git 결과에서 summary/chart를 파생하며 legacy get_day Git을
                   함께 호출하지 않는다. crates/integration으로 snapshot을 자동 발견하며
                   Knowledge activity/v1을 검증·집계하고 외부 DB 직접 조회 없음)
                   └ explicit digest → applink handoff store → launch_open → Knowledge preview
everything-plus:  validated roots → filesystem crate → bounded text extractor →
                   search crate(FTS5 + content metadata) → React
knowledge-base:   fs_store → filesystem/search crate → React(CodeMirror + context-menu)
                   ├ Markdown 원문 → wikilink parser → SQLite 재생성 가능 key/link index
                   │  → autocomplete·unresolved decoration·backlink source position
                   ├ explicit image paste/drop → bounded browser bytes → save_image_asset
                   │  → vault/assets/<sha256>.<safe-ext> + note-relative Markdown node
                   ├ canonical tree entry → opener 또는 catalog/launch crate → 설치된 대상 앱
                   └ path/body-free activity/v1 snapshot → Life Log Data Sources
                   └ applink handoff claim → draft preview → exclusive Journal note/index save → ack
                      (검증·파일·index 실패는 restore, 만료는 새 digest 재생성)
api-playground:   React(context-menu + History/Collection v2 + transfer/search/binary response projection)
                   → commands(secrets sanitizer + bounded cancellation + native file dialogs)
                   → reqwest → HTTP
                   └ GraphQL GET/POST query·variables·operationName은 기존 native HTTP 경계를
                      재사용하며 persisted query/introspection/subscription은 별도 제공하지 않음
code-pad:         React(CodeMirror + tab/editor context-menu, bounded workspace Quick Open tree)
                   → commands → LSP stdio 서버, snapshot-checked sibling rename/delete,
                   filesystem/markdown crate → React
run-manager:      React(context-menu + bounded log export/search) → commands → scheduler
                   → platform 실행 어댑터(Windows Job Object/WSL) → SQLite + app-owned 회전 로그
devbox-manager:   React → commands → catalog/manifest/install-root preview → GitHub release asset
                   └ custom root apply: canonical empty directory → app-owned registry → versioned locator
workbench:        React → commands → ProjectProfile/read-only health + 다른 앱 실행 (CLI argument,
                   v0.4.0에서는 argv 수신 부재로 미동작했으나, v0.4.1에서 crates/applink와
                   single-instance pending-open 수신을 Code Pad/WSL Desktop/Workbench에 구현.
                   v0.4.1은 이 핫픽스를 포함한 안정판으로 배포됐다. 남은 Windows packaged-runtime
                   acceptance는 [issue #176](https://github.com/jihoon22-lee/devbox/issues/176)에서
                   post-release로 계속 관리한다.
                   ./superpowers/specs/2026-08-17-app-interop-design.md)
webhook-lab:      inbound HTTP → core/server → history·rule·fixture → React
repo-manager:     React → commands → git crate → repository/worktree 탐색·생성
                   ├ read-only history/detail/diff → run_bounded → bounded parser → React
                   ├ selected stage/unstage + explicit commit → run_mutating → Git index/commit
                   ├ Git safety preflight → run_bounded → porcelain-v2/marker parser → React
                   └ remote status/preflight → bounded parser → default-remote fetch/
                      FF-only pull/exact branch push → run_mutating_with_cancel → configured Git remote
devbox-launcher:  transient React → bounded catalog/optional snapshot index → revalidated AppLink
                   ├ catalog app 검색과 profile/repo/query/task target 재검증
                   ├ missing target → Devbox Manager install handoff
                   ├ source path가 없거나 손상되면 해당 source만 격리
                   └ selected/clipboard text는 명시적 preview 동안에만 표시
log-lens:         React → bounded commands → app-local parser/ring → local file or fixed WSL/container adapter
```

Repo Manager의 Git history·diff(#316)는 선택된 canonical repository에서만 실행되는 native
read-only 흐름이다. repo_history, repo_commit_detail, repo_diff는 hexadecimal object ID와
고정된 argv만 허용하고 공용 crates/git::run_bounded로 stdin/stderr·timeout·stdout 상한을
강제한다. parser는 NUL metadata, repository-relative path, text/binary marker를 검증하며
history/detail/diff와 원문을 storage·telemetry·remote network로 복제하지 않는다. History는
기본 50개(최대 100개), detail 128KiB, 전체 diff 2MiB·파일당 patch 512KiB·최대 256파일로
제한한다. `%aI` authored timestamp는 calendar/date-time과 `Z` 또는 `±14:00` offset까지
strict ISO로 확인하며, 잘못된 날짜·시간·offset·hex ID는 fixed error로 폐기한다.

selected stage/unstage·explicit commit(#317)은 별도 mutable flow다. status는 bounded NUL
porcelain parser를 거친 DTO로만 표시하고, backend가 재검증한 repository-relative path를
`--literal-pathspecs`와 `--` 뒤에 전달해 선택한 파일만 index에 반영한다. rename 선택은
new/old path를 함께 검증해 전달하고, unborn repository unstage는 `git rm --cached`로
worktree를 보존한다. Commit은 `operationId`를 가진 명시적 확인을 통과한 뒤 현재 index만
대상으로 하며 unstaged 파일을 자동 추가하지 않는다. 공용 crates/git::run_mutating은 Git
config와 credential helper 해석을 그대로 두되 stdin/stderr를 닫고 timeout/stdout 상한을
적용하며 credential·raw path·message·stderr를 devbox가 저장하거나 반환하지 않는다.

Git 상태 사전 검사(#319)는 선택 repository에 대해 고정된 porcelain-v2 branch status와
rev-parse --git-path의 rebase/merge marker만 읽는다. dirty, detached, upstream 없음,
ahead/behind·diverged와 진행 중인 rebase/merge를 deterministic issue ID로 분류하며,
malformed/overflow·권한·busy·unmount·marker race는 고정 오류로 닫힌다. repository/index/ref/
remote/credential를 변경하지 않고 force push/reset/clean 또는 automatic recovery도 제공하지 않는다.
Frontend는 scan·preflight·panel 요청마다 mounted/request-sequence guard를 적용해 stale
응답을 폐기하며, failure는 raw path·Git stderr·remote URL·credential 없는 fixed error만
보인다. Preflight의 `safe`는 read-only snapshot의 known blocker가 없다는 의미일 뿐
mutation authority가 아니다.

remote sync(#318)는 remote/refspec 없이 `fetch --no-tags`를 호출해 Git의 기본 선택 규칙
(현재 branch에 configured remote, 없으면 `origin` fallback)를 사용하며 `--all`은 금지한다.
Pull은 `--ff-only --no-rebase`, push는 native가 읽어 검증한 configured remote와 upstream
destination에 대한 `HEAD:refs/heads/<destination>`만 사용한다. Pull/push는 clean·attached·
upstream·non-diverged 상태를 요구하고 push는 behind도 차단하며, 모든 remote action은
in-progress merge/rebase를 차단한다. Commit/pull/push는 UI 확인창을 거친다(검토 snapshot이
바뀌면 무효화); fetch는 read-only working tree 경계 때문에 확인 없이 시작할 수 있다.

Local·remote operation ID는 bounded opaque `operationId`로 첫 async await 전에 등록되고,
`repo_local_cancel({request:{operationId}})`/`repo_remote_cancel({request:{operationId}})`가
path 재검증 없이 해당 child를 취소한다. local mutation, remote mutation, `create_worktree`는
표시 경로가 아니라 `git --git-common-dir`의 common Git directory filesystem identity로
single-flight lock을 공유한다. linked worktree도 같은 common identity를 사용하므로 서로
동시 진입할 수 없다. Unix identity는 `dev/inode`, Windows identity는 native handle의
`volume serial/file index`이며, worktree/common directory와 worktree-create target parent는
mutation 직전에 다시 확인한다. cancellation/timeout은 Unix process group 또는 Windows
kill-on-close Job Object로 Git hook·credential helper·SSH/transport descendant까지 종료하고,
root Git이 먼저 끝나도 process tree와 bounded stdout reader를 회수한다.

`crates/git` child 환경에서는 `GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`,
`GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_CEILING_DIRECTORIES`,
`GIT_DISCOVERY_ACROSS_FILESYSTEM`, `GIT_PREFIX`, `GIT_QUARANTINE_PATH` 같은 repository-selection
override만 제거한다. Git config 및 credential/SSH/askpass 환경은 유지해 사용자의 credential
helper가 동작하도록 하며 devbox가 credential을 읽거나 저장하지 않는다. UI의 busy,
unmount/request-sequence와 confirmation focus trap/initial-cancel-focus/Escape/trigger-focus
restore는 duplicate·stale·우발적 mutation을 차단한다.

Port Manager의 listener row는 source별 display metadata와 kill precondition을 분리한다. Windows
row의 command line/path는 읽기 전용·bounded·credential-redacted projection이며, Windows creation
FILETIME identity는 JavaScript 정밀도 손실을 막기 위해 decimal string으로 wire한다. process control
command는 이를 받지 않고 endpoint와 creation FILETIME만 받는다. WSL row는 distro를 argv로
검증하고 ss 결과와 proc start tick을 함께 보존한다. kill command는 새 snapshot에서 endpoint와
identity가 모두 일치할 때만 고정된 Windows handle 또는 wsl.exe kill argv를 사용한다. Docker
published port는 container identity를 별도로 표시하고 process kill 대신 WSL Desktop stop
handoff descriptor를 반환한다. WSL detail은 distro/PID별 bounded cache로 재사용하며 snapshot의
모든 child 명령은 하나의 15초 deadline과 2 MiB stdout 상한, Windows kill-on-close Job Object를
공유해 timeout·output failure·root exit 뒤 WSL/container descendant가 남지 않게 한다. P3-04는 이 snapshot
producer를 재사용해 1–60초 bounded auto-refresh와 manual pause를 제공한다. refresh promise는
single-flight라 native child poll을 중첩하지 않으며, 첫 성공 결과는 baseline으로만 삼는다. 이후
성공 결과는 exact identity+endpoint match를 먼저 예약한 뒤 남은 strong identity move만
fallback matching해 new/closed/changed를 만들고, 실패 결과는
stable rows, baseline, favorites를 덮어쓰지 않는다. 실패 중에는 #285 kill/handoff 버튼도
fail-closed로 잠긴다. 성공한 kill은 pre-kill in-flight poll을 기다린 뒤 fresh snapshot을 한 번
더 요청한다. 포트 favorite와 process identity favorite, pinned filter는 app-local
data 아래 strict `port-manager-preferences-v1.json`으로만 저장한다. 문서는 64 KiB와 종류별
256개 상한, 1–60초 interval, endpoint/identity bounds, unknown-field rejection을 적용하고
`crates/filesystem::atomic_write`로 완전한 JSON만 교체한다. command line/path/credential은
favorite DTO에 존재하지 않는다. source/provenance는 Windows, WSL+distro, container+
engine/distro/ID로 화면과 저장 identity를 일치시킨다.

API Playground의 History·Collection context menu는 v2에 저장되고 backend sanitizer read-back을
통과한 `PersistedHistoryRequest`만 복제·이름 변경·삭제·마스킹 cURL 복사의 입력으로 사용한다.
History의 선택적 표시 이름은 기존 v2 wire shape에 하위 호환되며 이름까지 sanitizer가 검사한다.
context action은 현재 editor의 raw request나 unsealed environment secret을 읽지 않는다. 삭제는
확인 전 storage를 변경하지 않고, 복제는 마스킹 request를 깊은 복사한 뒤 전체 store를 다시
sanitize/read-back 한다.

API Playground의 Collection/Environment transfer는 앱 내부에 versioned JSON 문서를 만들고 읽는
오프라인 우선 기능이다. `devbox.api-playground.collection-export`와
`devbox.api-playground.environment-export`의 `schema_version: 1`만 허용하며 문서 1 MiB,
Collection 256건, Environment 64건, 환경별 변수 256건과 field byte 상한을 frontend parser와
native file command에서 함께 적용한다. Collection request는 기존 persistence sanitizer와
read-back을 다시 거치고, 가져오기는 기존 항목을 덮어쓰지 않고 새 opaque ID로 append한다.
데스크톱 file picker가 선택한 regular file만 읽고 native atomic write로 내보내며, browser는 명시적
file input/download만 사용한다. Environment secret은 DPAPI blob·평문을 export하지 않고
`${NAME}` reference와 `secret: true`만 남긴다. 민감한 key 또는 token-shaped value를
`secret: false`로 위조한 문서는 거부하고, 가져온 secret은 빈 placeholder로 표시되어 재입력을
요구한다. transfer 중에는 request send/save/delete를 잠그며 parser·dialog·write 오류는 fixed
message로 닫는다.

History search/filter는 safe display name·method·정화된 URL·status만 대상으로 하며 query는
최대 128자다. header/Cookie/auth/body와 environment secret은 검색 색인·정렬·결과 DTO에 들어가지
않는다. filter는 기존 v2 History 순서를 보존하고 success(200–399)/error/전체 상태를 별도
계산하므로 새로운 persistence schema나 network protocol을 만들지 않는다.

API Playground response viewer의 일반 DTO는 마스킹된 headers와 값이 제거된 Cookie projection만
포함한다. 원문 response headers는 `Serialize`/`Debug`를 구현하지 않은 app-managed vault가 현재
요청 1건에 대해서만 최대 100개·64 KiB로 보관한다. 새 요청을 시작하면 이전 entry를 즉시
폐기하며 raw name/value 버퍼는 drop 때 zeroize한다. 이후 monotonically 증가하는 opaque string
ID를 발급하며, 늦게 끝난 과거 요청은 current ID와
일치하지 않아 raw entry를 저장하지 못한다. 상한 초과·비텍스트 header는 masked DTO에 안전한
표시만 남기고 raw copy 전체를 fail-closed한다. 확인된 Headers/Set-Cookie copy command만 ID로
원문을 조회하며 반환 문자열은 clipboard write 외 storage·history·log에 전달하지 않는다.

Binary response는 response `Content-Type`과 strict UTF-8/제어문자 판별 뒤 text와 분리한 projection으로
다룬다. ordinary response는 최대 16 MiB, GraphQL response는 최대 4 MiB만 읽고, UI에는 media
type·size와 최대 4 KiB hex/UTF-8 preview만 보낸다. raw bytes는 current opaque response ID와
연결된 process-memory `Zeroizing` buffer에만 남고 History·Collection·localStorage·log·event
DTO에는 저장하지 않는다. binary secret 또는 token-shaped content가 확인되면 hex/text preview를
redact하며, invalid UTF-8은 lossy text로 승격하지 않는다. native save는 명시적 사용자 선택,
regular destination 검증과 atomic write를 모두 거치고 stale response ID·취소·오류는 fixed
failure로 닫는다. browser preview는 저장하지 않고 projection만 표시한다. 이 기능은 protocol별
streaming, arbitrary execution, 자동 download/clipboard fallback을 추가하지 않는다.

Knowledge Base의 `doc_link_keys`와 `wikilinks`는 Markdown 파일이 원본인 재생성 가능 SQLite 보조
인덱스다. 하나의 Rust parser가 앱 저장·watcher·편집 중 분석·preview를 함께 담당하며 frontmatter,
fenced/inline code와 escape된 opener를 제외한다. path stem·filename·frontmatter title key가 정확히
한 문서에만 대응할 때만 resolved다. 중복 key는 ambiguous, 부재 key는 missing으로 남고 새 문서가
인덱스되면 source 재작성 없이 현재 key 집합으로 다시 해석된다. schema v1 최초 실행은 root 내부의
canonical `.md` 원문만 최대 10 MiB로 읽어 정확한 line/UTF-16 column을 복구한 뒤 marker를 기록한다.
UI가 raw `[[target]]`을 경로로 열지 않으며 editor/preview/backlink가 받은 indexed 상대 경로도 실제
열기 직전에 canonical root·확장자·크기 검증을 다시 통과한다.

Knowledge의 이름 변경은 기존 즉시 `rename_file` IPC를 노출하지 않는다. preview command가 root
경로 목록, 모든 Markdown, 이동 subtree 내용을 10,000항목·64 MiB 경계 안에서 SHA-256으로 묶고,
깨질 때만 canonical 새 target으로 바꾼 link diff와 opaque one-shot plan ID를 반환한다. 제목 등
기존 key가 이동 뒤에도 유일하면 source를 쓰지 않고 `[[target|alias]]`의 alias는 그대로 보존한다.
frontend에는 root-relative path와 최대 1,024바이트의 영향 link syntax만 보내며 전체 원문·절대
경로는 plan vault 밖으로 내보내지 않는다. 적용 command는 현재 root와 destination을 canonical
검증하고 동일 스냅샷을 재계산한 뒤에만 파일별 atomic replace → filesystem rename → SQLite
FTS/link transaction을 실행한다. 실패 시 역순 rollback과 생성한 빈 parent 정리를 시도하며 rollback
자체 실패는 성공으로 숨기지 않고 수동 확인 오류로 올린다. 따라서 계약은 OS 전역 다중 파일
원자성이 아니라 실행 중 오류에 대한 conflict-checked all-or-rollback이다. apply 도중 process/OS
강제 종료를 복구할 persistent journal은 이 기능 범위가 아니다. watcher는 같은 DB mutex 뒤에서 후속
event를 처리하므로 command transaction과 인덱스를 동시에 수정하지 않는다.

Knowledge의 이미지 자산은 노트 파일과 분리된 root-level `assets/` 저장소를 사용한다. 명시적인
clipboard image paste 또는 단일 image drop만 bounded bytes를 frontend에서 native
`save_image_asset`으로 보낸다. native는 PNG/JPEG/GIF/WebP magic과 header dimension을 확인하고
2 MiB, 한 변 16,384px, 총 64M pixel 경계를 적용한다. 입력 MIME과 original filename은 저장
계약에 참여하지 않으며, 파일명은 SHA-256 lowercase content hash와 고정 확장자로만 생성한다.
`assets/`와 기존 note 경로는 canonical root 안에서 다시 검증하고 symlink/reparse·absolute/
drive/UNC·control/traversal 경로를 거부한다.

저장은 temp file의 bounded write/flush/sync 뒤 no-overwrite atomic publication(Unix hard-link와
parent directory `fsync`, Windows non-replacing `MoveFileExW`의 `MOVEFILE_WRITE_THROUGH`)으로
노출한다.
동일 hash의 동일 bytes만 idempotent reuse하고, collision·partial write·storage 오류는 기존
파일을 덮어쓰지 않는 고정 오류로 종료한다. native는 note를 직접 수정하지 않는다. 성공 응답은
현재 note directory에서 계산한 `../assets/...` Markdown node와 opaque `reused` boolean만
반환하며, frontend가 현재 editor document와 note identity를 재검증한 뒤 in-memory draft에
삽입한다. 사용자가 Save를 선택하기 전에는 note 원문/검색 인덱스를 변경하지 않는다. 응답이
늦게 도착한 동안 문서가 바뀌거나 note가 전환·unmount되면 asset node를 삽입하지 않는다.
preview loader도 nested relative destination을 안전하게 normalize한 뒤 canonical existing
entry로만 읽고, 이미지 원문·경로를 snapshot/telemetry/external service로 보내지 않는다.

## 앱 간 데이터 교환

상대 앱의 `app_local_data_dir`을 직접 읽지 않는다. producer가
`%LOCALAPPDATA%\devbox\integration\<app-id>\v<n>\`에 privacy-safe snapshot을 원자적으로
기록하고 consumer는 읽기만 한다. (상세: `docs/product-opportunities.md` §10.1)

`crates/integration::discover()`는 각 version의 `summary.json`을 자동 발견한다. 한 producer의
손상·과대·unsafe-link snapshot은 다른 producer를 막지 않으며, Life Log의 Data Sources는
발견 결과와 격리된 안전한 오류를 동적으로 표시한다. 여러 kind는 `data.views`에 모아 파일
전체를 한 번만 교체하고 각 view가 `schemaVersion`, `freshnessMs`, `entries`를 소유한다.
공용 경계는 10MiB 상한과 producer/version/path/timestamp 검증을 적용하고
Authorization·Cookie·credential·raw environment 계열 필드를 거부한다.

v0.5.0에서는 지속 상태는 snapshot, 일회성 작업 전달은 applink protocol v2 handoff로
구분한다. API request, Knowledge draft, log source처럼 argv에 안전하게 넣을 수 없는 payload는
128-bit opaque id만 argv에 전달하고 공용 root의 TTL·크기 제한 payload를 한 번 소비한다.
devbox가 양쪽 앱을 제어하면 clipboard·임시 export 파일 전달은 명시적 fallback으로만 둔다.

2026-08-27 #307+#315 그룹 작업은 이 경계를 두 개의 명시적 앱 간 흐름에 적용한다.
Webhook Lab은
backend가 읽은 masked history/fixture만 `api-request/v1` payload로 만들고, catalog에서
설치된 `api-playground` capability를 확인한 뒤 producer/consumer ID가 있는 envelope를
공용 store에 기록한다. 실행 argv에는 kind와 opaque ID만 들어간다. API Playground는 cold/hot
single-instance 경로에서 ID를 claim하고 적용 전 preview를 표시하며, 적용은 ack/delete,
취소는 restore한다. TTL·size·target/kind/schema·lease와 URL/header/body privacy 검증을
양쪽 경계에 두고, 미설치·실행 실패·만료·손상·중복 소비는 fixed error로 격리하며 clipboard
fallback을 사용하지 않는다.

Life Log의 `Send to Knowledge`는 이 일회성 경계를 `knowledge-draft/v1`에 적용한다. native
digest를 다시 검증한 producer가 aggregate-only summary, 결정론적 Markdown body, 고정 tags와
source provenance만 10분 TTL envelope에 기록하고 `launch_open`에는 target kind와 opaque id만
전달한다. Knowledge는 source/target/kind/schema, body/title/tags/summary bounds와 privacy
marker를 다시 검증한 뒤 process-local claim slot에서 저장 전 preview를 제공한다. 사용자가
`Save draft`를 확정하면 `Journal/YYYY-MM-DD-life-log-<period>[-n].md`를 exclusive create하고
검색 index를 갱신한 다음 ack/delete하며, cancel·validation·file/index 실패는 claim을 restore한다.
만료·손상·미설치/실행 race는 raw payload·path·OS 오류 없이 고정 안내로 격리하고 Life Log의
새 digest 재생성으로 회복한다. preview lease는 30초마다 갱신하지만 envelope TTL은 연장하지
않으며, 어느 앱도 상대 앱의 DB나 원문 노트를 직접 읽지 않는다.

Knowledge handoff의 preview/save는 명시적으로 설정된 vault만 대상으로 하며, default-root
초기화나 Journal 자동 생성으로 외부 요청을 승격하지 않는다. preview가 보관한 canonical root와
filesystem object identity를 save·파일 publication 직전에 재검증하고, symlink/reparse ancestor,
root 교체, Journal 종류 변경은 fixed error와 claim restore로 격리한다. 파일은 완전히 flush한
temporary sibling을 no-replace primitive로 publish하고, 그 entry identity가 일치할 때만 index
실패 cleanup을 허용한다. watcher도 같은 vault identity 경계와 regular UTF-8 document/10 MiB 제한을
사용하며 이벤트당 path 수·길이와 raw event queue·pending path를 각각 bounded하게 유지한다. frontend modal은 title/body
UTF-8 byte budget과 explicit Save/Cancel, stale/expiry/unmount guard, focus trap/restore를
제공한다.

Log Lens는 local file/directory, fixed WSL `cat`/`journalctl`, fixed Docker/Podman `logs`
adapter만 읽는다. Source path, command, environment, credential은 source identity나 error로
반향하지 않으며, parser는 app-local core에 남겨 `crates/logs` 조기 추출을 피한다. 100,000 line
또는 64MiB process-memory ring과 16KiB line cap을 함께 적용하고, rotation/truncate는 file
identity와 size cursor로 재시작한다. operation ID/generation guard와 cancellation은 stale
callback·unmount·single-flight 결과를 폐기한다. saved view는 source 설정과 filter만 memory에
보관하며 raw log는 저장하지 않는다. export/copy는 사용자가 누른 현재 selection에 한해서만
수행한다. Run Manager의 `log-source/v1` receiver는 이 bootstrap에서 identity만 검증하며,
producer claim/ack와 WSL/Run 실연결은 후속 integration PR의 책임이다.

Workbench는 Life Log의 app-local DB와 settings schema를 알지 않는다. 시작 시
`life-log/projects/v1`을 producer/schema/freshness 기준으로 검증하고 안전한 절대 경로만
ProjectProfile로 흡수한다. 파일 없음은 no-op이며 손상·schema mismatch·unsafe entry는 기존
프로필을 바꾸지 않는 fail-closed fallback이다.

Knowledge Base는 `knowledge-base/activity/v1` view에 UTC 기준 오늘 작성·수정된 노트 수,
전체 최근 수정 시각, 최대 512개의 `note-<DB row id>` 불투명 식별자와 truncation 여부만
발행한다. 경로·제목·본문·tag는 생산 경계를 넘지 않는다. 앱 내부 CRUD뿐 아니라 watcher가
반영한 외부 편집도 원자 snapshot을 갱신하므로 Life Log는 Knowledge DB나 노트 파일을 직접
읽지 않는다. Life Log는 producer/envelope/view schema와 단일 entry, 식별자 형식·유일성·
개수 관계를 검증하고 UI에는 수와 시각만 전달한다. 구버전 flat v1은 롤링 업그레이드
fallback으로만 허용하며, 손상된 Knowledge snapshot은 다른 producer 발견과 집계를 막지 않는다.

Life Log `digest/v1`는 `export/v1`의 bounded producer를 입력 단계로 공유한다. native command는
실제 달력 월말(윤년 포함), 연속된 23/24/25시간 civil-day boundary, timezone 문자열과
exclusive epoch 범위를 DB/Git 조회 전에 검증한다. project setting은 raw bytes/count/path
bound를 적용한 뒤 safe absolute path를 identity 기준으로 중복 제거한다. 하나의 digest
operation만 DB progress handler와 순차 bounded Git child를 소유하며, 취소 시 같은 generation
token으로 SQLite와 child를 모두 중단하고 다음 operation은 이전 guard가 해제된 뒤 시작한다.
앱 filter는 sanitized session 집계에만 적용되고 Git은 전체 requested range를 유지하므로 UI와
Markdown에 두 scope를 함께 설명한다. native response의 Markdown은 120초 TTL server-owned
immutable handle로 저장되어 `save_digest`가 입력을 재계산하지 않고 화면과 동일한 bytes를
atomic write한다. 최종 write는 generation mutex로 cancellation과 선형화해 취소 직전 commit race를
막는다. handle·document·Markdown은 모두 bounded이고, stale provenance는 30일 상한을
넘으면 수치 없이 `snapshot_stale`로 격리한다. #306은 Knowledge handoff/저장(#307)을 호출하지
않는다.

Developer Toolbox Smart Workflows는 renderer-memory input/output과 metadata persistence를
분리한다. detection/pipeline은 static local transformer registry만 사용하고 URL·shell·network를
실행하지 않는다. app-local `smart-workflows.json`에는 version, tool/transformer/type/pipeline ID와
timestamp만 들어가며 native는 64 KiB bounded strict schema, final object identity, process-local
writer serialization과 atomic replace를 적용한다. malformed store는 자동 복구 write로 덮지 않는다.
브라우저 preview의 localStorage도 같은 metadata allowlist만 사용하고 원문 draft/result를 저장하지
않는다.

Toolbox→API Playground는 이 local pipeline의 stage가 아니라 별도 explicit action이다. 사용자가 현재
output을 `POST /` text/plain draft로 preview/edit/confirm하면 Toolbox native producer가
`api-request/v1` one-time envelope를 만들고 opaque ID만 launch argv에 넣는다. shared AppLink privacy
validator는 raw credential을 publish 전에 거부하고 launch 실패는 exact pending envelope를 revoke한다.
API Playground receiver는 Toolbox/Webhook source를 allowlist하고 claim/lease/restore/ack 후 editor만
갱신하며 자동 request send나 clipboard fallback을 수행하지 않는다.

## 보안 경계

각 앱이 다루는 외부 입력과 그 방어선:

| 방어선 | 위치 | 무엇을 막는가 |
|---|---|---|
| `ammonia` HTML 살균 | `crates/markdown` `sanitize()` | 마크다운 HTML의 `<script>` 제거, `javascript:` URI 차단 |
| mermaid `securityLevel: "strict"` | code-pad `PreviewPane`, knowledge-base `MarkdownPreview` | 다이어그램 HTML의 XSS |
| CSP (`csp` 정책) | 각 앱 `tauri.conf.json` | DOM injection 시에도 임의 `invoke`/네트워크 접근 차단 |
| Clipboard 최소 권한 | Developer Toolbox·Knowledge·Code Pad `clipboard-manager:allow-read-text`; Knowledge image는 명시적 browser Clipboard API read | 명시적 Paste 이외의 image/write/clear IPC와 background clipboard 수집 차단 |

`csp: null` + `core:default` 조합은 DOM injection이 성립하면 곧바로 `invoke`에 닿게 만든다.
앱들이 임의 로컬 파일(code-pad, knowledge-base, everything-plus)과 임의 원격 응답
(api-playground)을 다루므로 명시적 CSP 정책을 둔다. (상세: `docs/product-opportunities.md` §7.5)

Developer Toolbox, Knowledge, Code Pad는 input/editor context menu에서 사용자가 붙여넣기를 선택한 순간에만
system clipboard의 plain text를 읽는다. Knowledge image paste는 같은 명시적 action의
`navigator.clipboard.read()` 또는 browser paste event에서 image bytes를 한 번 읽으며,
clipboard-manager image/clear/write 권한을 추가하지 않는다. 읽은 값/bytes는 현재 controlled
input 또는 CodeMirror selection에만 연결되고 log, snapshot, settings, history, telemetry에
기록하지 않는다. Knowledge asset은 native vault 저장 후 generated Markdown node만 draft에
삽입한다. Copy는 기존 WebView clipboard write 경로를 쓰고, Toolbox 결과 파일 저장은 사용자가
누른 항목에서 생성한 local text download로만 수행한다.

Run Manager의 job/service/history context menu는 열기 전에 대상 행을 선택하고, 메뉴 action은
그 snapshot의 불투명 ID만 backend에 전달한다. 활성 실행 stop과 service stop, job/service delete는
메뉴와 기존 버튼 어느 쪽에서 실행해도 명시적 확인을 요구한다. 작업 active-run snapshot이 아직
정상 확인되지 않았거나 service가 `stopping`이면 파괴적·lifecycle 항목을 fail-closed로 비활성화한다.
service instance snapshot 자체가 없을 때도 stopped로 추정하지 않고 모든 lifecycle·delete 항목을
비활성화한다. `retry_waiting` 서비스의 stop은 같은 service lock 아래 예약된 backoff를 취소하고,
restart는 stopped 전이를 거쳐 새 generation을 claim한다.

실행 이력의 로그 저장은 사용자가 선택한 stdout 또는 stderr 한 스트림만 `tail_log`로 읽는다.
backend는 run ID에서 app-local `logs/runs` 경로를 다시 만들고 stored relative directory와 canonical
경계를 검증하므로 frontend는 filesystem path를 받거나 선택하지 않는다. cursor는 lossless decimal
string으로 유지하고 응답은 256KiB, 한 번의 저장은 현재 per-stream 보존 상한과 같은 50MiB로 제한한다.
cursor가 전진하지 않거나 보존 범위가 이동하거나 상한 뒤 데이터가 남으면 부분 저장임을 알린다.
download 파일명은 64자 이하의 sanitized opaque run ID와 stream만 포함해 command, cwd, 환경변수,
원래 log path가 이름이나 오류에 노출되지 않는다.

Run Manager의 #311 검색도 이 경계를 확장하지 않는다. `search_run_logs`는 기존 tail의
256KiB cursor chunk를 source당 4MiB·전체 8MiB까지만 읽고, chunk 사이에 scheduler에
양보한다. 결과는 literal 우선/명시적 linear-time regex, level·source·time 필터와
보존 snapshot의 stream·line metadata만 반환하며 log 원문·path·credential을 별도 payload로
복제하지 않는다. `log-source/v1`는 `run-manager:<opaque-run-id>:<stream>` identity를
검증하는 local contract로만 존재한다. request/source는 unknown field를 거부하고 timestamp는
JavaScript safe integer로 제한하며, 동기 filesystem metadata 복원과 bounded scan은 async
command executor 밖의 blocking worker에서 수행한다. Log Lens handoff/remote ingest/permanent
archive는 Log Lens bootstrap 뒤 별도 integration 범위다.

Webhook Lab의 history/rule context menu도 열기 전에 대상의 opaque ID를 선택한다. 일반 history
DTO, 마스킹 복사, 헤더 복사는 Authorization·Cookie·API key 값을 마스킹하며, 원본 헤더를 가진
내부 entry는 Serialize/Debug를 구현하지 않고 process memory에만 최대 200건 유지한다. 요청별
보관 헤더는 100개·총 64K자, body는 256K자로 제한한다. raw copy command는 사용자가 별도 경고를
확인한 뒤에만 호출하고 반환값은 일회성 clipboard write 이외에는 저장·기록하지 않는다. 개별
history/rule 삭제와 전체 history 비우기는 기존 버튼을 포함해 확인을 요구하며, clear 뒤에도
프로세스 안의 history ID를 재사용하지 않는다.

Captured fixture는 history의 masked snapshot만 opaque ID로 읽어
`app_local_data_dir()/fixtures.json`에 저장한다. fixture 자체도 method·origin-form target·header·
body·timestamp를 다시 검증하며, Authorization/Cookie/token/secret/password/auth 계열 값과
known credential marker를 `[REDACTED]`로 바꾸고 절대/unsafe path는 고정 marker로 대체한다.
schema v1은 fixture 200개·파일 8 MiB·bounded field limits를 적용하고, corrupt·oversized·link-backed
파일은 원본을 자동 복구하지 않고 fixed error로 중단한다. app-owned parent/file 검사,
raw-byte CAS, process-local write lock, atomic replace로 concurrent update와 partial write를
방지하며 timestamp 내림차순+ID tie-break로 목록을 결정적으로 만든다. fixture의 response-rule
초안은 로컬 editor draft다. #315 handoff는 이 masked fixture 경계에서만 payload를 만들고,
replay/sequence(#362)는 별도 범위다. example curl은 기존 bounded redaction contract를 따른다.

Devbox Manager의 app-row context menu도 메뉴를 열기 전에 catalog app ID로 대상 행을 선택하고,
설치/업데이트의 portable·setup 선택을 submenu로 보존한다. 실행·rollback·설치 폴더 열기·제거는
portable registry snapshot에서만 활성화하며 installer 상태나 확인되지 않은 상태에서는 fail-closed다.
제거는 danger 표시와 명시적 확인을 거친다. frontend의 installed/current DTO에는 registry의
`exe_path`를 포함하지 않고 action은 app ID만 backend에 전달한다.

일괄 설치/업데이트는 frontend의 catalog-derived checkbox selection을 `appId`와
`portable|installer` mode 목록으로만 전달한다. backend는 빈 목록, 32개 초과, 중복·잘못된 ID/mode와
manager-visible/non-self-managed가 아닌 target을 mutation 전에 거부한다. 유효한 batch는 release
manifest와 HTTP client를 한 번만 준비하고 입력 순서대로 한 앱씩 처리한다. 한 항목이 실패해도 다음
항목을 계속하며 public 결과는 app ID, mode, 성공 여부와 고정된 안전 메시지만 포함한다. 성공 결과는
유지하고 UI는 실패 결과만 선택한 채 exact mode로 다시 호출한다. batch 전체를 하나의 rollback
단위로 만들지 않는다. backend가 installed/available version을 strict SemVer로 다시 비교해 available이
더 큰 경우에만 변경하며, 같거나 더 최신 버전이 설치된 stale selection은 download 없는 성공 no-op다.

portable 항목은 검증된 version artifact를 준비한 뒤 current와 registry를 갱신한다. registry commit이
실패하면 이전 `current.json`을 복구하거나 최초 설치였다면 새 current를 제거하고 해당 항목만 실패로
표시한다. setup 항목은 durable registry 준비 뒤 검증된 installer를 spawn하고, spawn 실패 시 registry를
복구한다. setup 성공은 설치 완료가 아니라 마법사 실행 성공이므로 여러 항목 실행 전 UI 확인과
결과 설명을 제공한다. batch 도중 URL, digest, process와 filesystem의 원문 오류는 frontend DTO로
반사하지 않는다.

설치 경로 표시는 lifecycle DTO와 분리된 명시적 read-only `install_path(appId)` IPC만 사용한다.
backend는 selected runtime/build catalog revision과 install-root locator의 catalog provenance가
일치하는지 확인하고, locator가 가리키는 canonical root와 그 내부의 canonical source manifest를
검증한다. manifest 전체가 알려진 catalog app만 포함하고 중복·잘못된 version/mode가 없으며 모든
portable executable이 `<root>/apps/<app-id>/versions/<version>/<app-id>.exe`와 정확히 같을 때만
표시 DTO를 만든다. 또한 locator의 source manifest가 현재 Manager 설치 목록이 읽는 canonical manifest와
같아야 한다. 미래 custom-root locator와 아직 default-root인 Manager 상태가 섞인 과도기에는 다른 root의
같은 app ID를 표시하지 않고 실패한다. 원시 locator/manifest 값이나 검증 오류는 frontend에 반사하지 않는다.

portable DTO는 app ID, mode, canonical executable, canonical install root, canonical source manifest를
포함한다. installer manifest는 installer 실행 사실만 소유하고 마법사 완료 위치는 소유하지 않으므로
executable과 install root를 `null`로 반환하고 source manifest만 표시한다. UI는 이를 `읽기 전용`으로
표시하며 copy/open/edit action을 제공하지 않는다. command는 filesystem, registry, process를 변경하지
않고, 기존 installed/current 및 launch/remove DTO는 계속 path-free 상태를 유지한다.

Manager backend는 action마다 manager-visible/non-self-managed catalog target, bounded version component,
`<manager-root>/apps/<app-id>/versions/<version>/<app-id>.exe` 고정 layout과 registry executable의 canonical
identity를 다시 확인한다. `preview_remove_app`은 이 검증 결과와 bounded app-owned tree를
read-only로 반환하고, `remove_portable_app`은 preview의 root/catalog revision 및 manifest digest를
CAS로 다시 확인한 뒤 정확히 수집된 파일·디렉터리만 깊은 순서로 non-recursive 삭제한다. portable 제거
전에는 symlink, Windows reparse point, 특수 파일과 foreign entry를 거부하며, user data는 이 경계
밖에 있어 보존된다. registry를 먼저 atomic claim하고 실패·부분 제거 때 동일 digest일 경우에만 원래
manifest bytes를 복원한다. 이미 삭제된 exact final executable은 recovery parser에서만 missing으로
허용한다. installer lifecycle은 wizard 실행 사실만 기록하고 실제 설치 위치나 uninstaller를 추측하지
않으므로 제거를 지원하지 않는다.

Devbox Manager의 custom install root는 `preview_install_root`와 `apply_install_root` 두 단계로
분리된다. preview는 사용자 문자열을 native에서 trim/bounds/canonicalize하고, 기존 root·home·workspace·
환경변수 표기·symlink/reparse·비디렉터리·기존 항목·쓰기 권한·최소 128 MiB free-space를 검사하지만 파일을
생성·이동·삭제하지 않는다. active manifest가 비어 있지 않거나 `apps/`, partial, 기타 root artifact가
남아 있으면 `existing-install`로 종료한다. 후보는 이미 존재하는 canonical 빈 디렉터리여야 하므로
부모를 임의로 만들거나 사용자의 파일을 덮어쓰지 않는다.

적용은 preview의 양의 `registryRevision`을 CAS token으로 받아 경로·active manifest·artifact·free-space를
즉시 재검사한다. 성공할 때만 후보 안에 `apps/`와 빈 `registry.json`을 component별로 안전하게 만들고,
manifest는 기존 경로를 대체하지 않는 exclusive create+sync로 준비한다. locator publish 직전에는
candidate direct entries, empty apps와 exact manifest bytes를 다시 확인하고,
`%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`을 새 canonical root/manifest와
`catalogRevision` provenance로 atomic replace한다. locator write가 실패하면 이번 호출이 만든 빈
manifest/apps만 rollback하고 기존 root·설치 파일·사용자 data는 절대 이동하거나 삭제하지 않는다.
rollback 시 manifest는 여전히 exact regular `[]`일 때만 제거한다. 다른 writer가 내용을 바꿨으면
외부 변경을 보존하고 rollback 실패를 반환한다. root preflight/apply 중 frontend는 refresh,
doctor, app lifecycle과 batch selection/action을 같은 single-flight operation으로 잠근다.
metadata refresh/doctor도 read single-flight를 소유해 진행 중에는 root·app mutation을 막고,
mutation 소유자가 수행하는 후속 refresh만 명시적 internal 경로로 허용한다.
locator가 없을 때만 v0.4.x default root를 read-only fallback으로 보고, 손상된 locator나 valid locator
뒤의 manifest/path 오류는 fail-closed한다. 적용 후 Manager의 install/current/rollback/launch/path 조회는
locator의 active root를 사용하며, #309의 safe removal도 default/custom root에 동일하게 적용한다.
non-legacy Manager lifecycle은 locator catalog provenance가 선택 catalog revision과 같고 source
manifest의 모든 app ID가 현재 manager-visible/non-self-managed 대상일 때만 동작한다. startup
metadata sync가 실패한 stale custom locator/manifest를 command가 우회해 계속 사용하지 않는다.

startup partial cleanup은 active root 전체에서 이름이 `.partial`인 파일을 재귀 삭제하지 않는다.
catalog-visible Manager app과 strict version에서 계산한 exact portable download slot만 bounded scan으로
먼저 모두 수집하고, scan 전체가 안전할 때만 삭제한다. 따라서 사용자 sibling/nested partial은
보존되고 link/reparse·특수 파일·읽기 실패·과대 tree는 cleanup 전체를 fail-closed한다. startup
metadata sync는 custom locator의 active root/manifest도 mutation 전에 재검증한다. valid custom root는
선택 catalog revision으로 provenance와 registry revision만 전진시키며 path identity는 유지하고,
locator가 선택 revision보다 앞서거나 unsafe하면 downgrade·재작성하지 않는다.

Workbench의 profile-row context menu는 열기 전에 opaque profile ID로 대상 행을 선택한다. 현재 UI가
추적 중인 workspace run은 profile ID와 함께 유지하고 frontend reload 때 run/profile ID ownership만
복원한다. 기존 step detail, PID, 경로는 restore DTO에 포함하지 않는다. 다른
profile의 start와 active/starting profile 삭제를 fail-closed로 막는다. 단일 transition claim은
concurrent start와 start/delete race를 차단한다. Stop What I Started는 run ID와 profile ID를 함께 backend에 보내고 저장된 run
소유권이 일치할 때만 Workbench가 기록한 PID를 정리한다. stop과 profile delete는 메뉴와 기존 inline
button 양쪽에서 같은 명시적 확인을 거친다. profile delete는 저장 정의만 지우고 프로젝트 파일이나
기존 외부 resource를 삭제하지 않는다.

Workbench의 “다른 앱으로 열기” submenu는 `crates/launch::installed_targets`의 `path`/`workspace`
capability 교집합에서 생성하며 executable과 profile path를 frontend에 보내지 않는다. 사용자가 target을
선택하면 backend가 현재 `project-profiles.json`을 다시 읽고 bounded absolute Windows/UNC/POSIX 경로를
검증한다. workspace payload는 Windows project path가 있을 때만 노출하고, 동일 target이 path와
workspace를 모두 받으면 workspace를 우선한다. 경로 복사는 사용자가 항목을 누른 순간에만 같은 검증을
통과한 현재 path를 frontend로 반환해 system clipboard에 기록한다.

Workbench의 #312+#313 grouped Start Workspace 흐름은 다음과 같은 read-only observation →
explicit continue → execution-time revalidation 순서를 가진다.

1. 사용자가 Start를 누르면 `workspace_preflight`가 필수 앱 capability, WSL distro/실행 상태,
   Windows·WSL working directory, 예상 TCP port와 Run Manager snapshot dependency를 bounded
   probe한다. stopped distro를 확인만 하려고 시작하지 않고, probe 실패는 fixed status/detail로
   줄인다. 결과의 `ResourceProvenance`는 existing/notRunning과 Workbench-started를 구분한다.
2. UI modal은 warning을 검토 가능한 상태로 보여 주되 Continue를 명시적으로 요구하고, failure/
   unavailable에서는 Continue를 비활성화한다. profile selection, Escape/Cancel, unmount와 late
   result는 generation guard로 폐기하며 Continue 중 target/profile은 고정한다.
3. backend `start_workspace`는 modal 결과를 신뢰하지 않고 동일 preflight를 다시 실행한다. 실패하면
   `.env` source를 읽거나 child를 열지 않는다. 통과한 뒤에도 profile store와 project root/source
   identity를 child 직전에 재검증하고, #312의 metadata-only environment revision이 맞을 때만
   zeroizing ephemeral overlay를 전달한다.
4. 첫 child가 시작된 뒤 profile/source가 바뀌거나 두 번째 child의 environment provider가 실패하면
   `StartedPidGuard`가 Workbench가 이번 transition에서 만든 PID만 rollback한다. 성공한 run에는
   고정된 preflight/resource provenance만 남고 PID·경로·stderr·secret은 restore/IPC DTO에 없다.
   검토 상태가 그대로인 일반 child launch 실패는 성공한 child를 자동 종료하지 않고 고정된 단계
   실패를 포함한 partial run으로 게시한다. 사용자는 그 결과를 확인한 뒤 `Stop What I Started`로
   Workbench-owned process만 정리한다.

이 grouped PR은 사용자 흐름과 재검증 기반을 공유하지만 acceptance/rollback은 독립적으로 추적한다.
#313은 service 생성·수정·자동 복구를 소유하지 않으며 #312는 `.env` write/upload, global/cloud
environment store와 다른 앱 DB 변경을 하지 않는다.

### CSP 기준선

15개 앱 전부 다음 최소 기준선을 쓴다 (PR 17 + 신규 앱 반영).

```
default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline';
font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost
```

- `connect-src ipc: http://ipc.localhost` — Tauri v2 IPC 채널
- `style-src 'unsafe-inline'` — React 인라인 스타일과 mermaid가 삽입하는 SVG 스타일
- `img-src data:`·`font-src data:` — data URI 아이콘/폰트
- 앱별 예외가 필요해지면 그룹 단위로 최소한만 추가한다:
  - A(외부 콘텐츠 렌더): code-pad, knowledge-base — mermaid SVG
  - B(외부 응답 취급): api-playground, devbox-manager — 응답 텍스트/릴리스 메타데이터
  - C(로컬 데이터만): 그 외 — 기준선 그대로
- dev 모드 HMR(WebSocket)이 기준선과 충돌하면 dev/prod CSP를 분리하거나 `connect-src`에 dev 오리진을 추가한다

## 앱 카탈로그

`apps/catalog.json`이 앱 식별자의 단일 원본이다 — 배포 대상 목록이자 런타임 discovery의
단일 원본. 앱 ID·productName·bundle identifier·Cargo package·앱 디렉터리를 소유한다.
버전은 카탈로그가 소유하지 않는다(세 파일 `Cargo.toml`/`tauri.conf.json`/`package.json`이 원본).

카탈로그는 두 가지 소비자를 갖는다:
- **release workflow** — 빌드 대상 앱 목록을 카탈로그에서 읽는다 (하드코딩 배열 금지)
- **Devbox Manager** — 설치·업데이트 대상과 앱 표시 여부를 카탈로그에서 읽는다

현재 schema v2는 단조 증가하는 `catalogRevision`과 `accepts`/`produces`/`actions`를
소유한다. `crates/catalog`가 v1 하위 호환, v2 검증, build/runtime revision freshness
선택과 순수 capability filter를 담당한다. Devbox Manager는 시작과 설치 registry 변경 때
`%LOCALAPPDATA%\devbox\catalog.json` 및 versioned install-root locator를 원자적으로
동기화하고, 현재보다 낮은 revision으로 덮어쓰지 않는다. `crates/launch`는 locator가
가리키는 Manager 소유 manifest에서 canonical executable을 확인한 뒤 실제 설치된
capability target만 반환한다. locator가 없는 v0.4.x 환경에 한해서만 기존 고정
Manager root를 read-only fallback으로 읽으며, 손상된 locator 또는 유효한 locator 뒤의 manifest/path 오류는
fail-closed 처리한다. `crates/applink`는 argv 계약만 담당해 `launch`와의 순환 의존을
피한다.

legacy fallback은 locator 파일이 없는 경우에만 허용한다. locator 경로의 이미 존재하는
parent component가 symlink/reparse이거나 active portable record가 exact layout 밖을
가리키면 Manager와 launch consumer 모두 fail-closed하며, startup sync도 present corrupt
locator를 default metadata로 덮어쓰지 않는다.

고정된 공용 metadata 경계는 다음과 같다.

| 파일 | 소유자 | 소비자 | freshness/안전 조건 |
|---|---|---|---|
| `%LOCALAPPDATA%\devbox\catalog.json` | Devbox Manager | `crates/catalog`, Manager, 메뉴 소비 앱 | 유효한 v2이며 build-time `catalogRevision` 이상일 때만 runtime 우선 |
| `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json` | Devbox Manager | `crates/launch` | 양수 `registryRevision`, catalog provenance, canonical root/manifest |
| `<active-install-root>\registry.json` | Devbox Manager | Manager, `crates/launch` | app/version/mode와 exact portable executable layout 일치 |

custom root selector는 위 locator 계약을 소비하는 #308 범위에 포함되지만 “이동”은 빈 후보에
대한 pointer 전환만 뜻한다. 기존 설치가 있는 상태에서의 migration과 root reset은 수행하지 않으며,
binary removal은 #309의 manifest-CAS/exact-tree 경계를 통해서만 수행한다. user-data 삭제는 하지
않는다. locator에는 설치 목록을 복제하지 않고 app-owned manifest 위치만 둔다.
`registryRevision`·catalog provenance·path/manifest bounds와 canonical/symlink/reparse 검증 실패는
공용 consumer와 Manager 모두 동일하게 fail-closed해야 하며 raw path·OS error는 public DTO에 내보내지 않는다.

Repo Manager의 "다른 앱으로 열기"는 이 계약의 첫 동적 UI 소비자다. `path` capability와
실제 설치 executable이 모두 확인된 앱만 표시하고, 같은 앱이 `workspace`도 선언하면
repository에 더 구체적인 Workspace payload를 우선한다. 대상 앱 ID나 executable 경로를
프론트에 하드코딩하지 않으며, source app 자신은 메뉴에서 제외한다.

Repo Manager의 repository context menu는 열기 전에 canonical repository key로 정확한 카드를
선택한다. “다른 앱으로 열기” submenu만 위 catalog/installed capability 결과로 만들고 action 때
target ID와 현재 card path를 backend가 다시 검증한다. 경로 복사와 OS file manager 열기도 같은
absolute/traversal/existing `.git` 검증을 다시 거친다. raw path는 명시적 copy 결과에서만 새로
반환하고, opener/검증 상세 오류에는 거부된 path를 포함하지 않는다. worktree 생성 항목은 자동 Git
명령을 실행하지 않고 선택한 카드의 기존 입력으로 focus만 이동한다. 카드 내부 text input의 기본
context menu·IME는 가로채지 않는다. 실제 worktree/branch remove는 dirty/untracked/locked/main
차단과 preview를 소유한 #364 safe cleanup 전까지 메뉴에 넣지 않는다.

Everything+의 검색 결과 context menu도 같은 설치 경계를 사용한다. 앱 고유의 열기·Explorer
reveal·경로/파일명 복사와 달리 "다른 앱으로 열기" submenu만 `path` capability와 설치
manifest의 교집합에서 생성한다. frontend에는 app id와 표시 이름만 전달하고 executable은
노출하지 않는다. 실행 command는 전달받은 id가 현재 교집합에 남아 있는지 확인하고, 결과가
traversal 없는 기존 절대 파일인지 재검증한 뒤 `from=everything-plus`인 versioned Path
app-link를 만든다. locator/manifest가 없거나 대상이 제거되면 submenu는 비활성화되며 임의
fallback executable을 실행하지 않는다.

Knowledge Base는 catalog revision 2부터 `path`와 `query`를 수신한다. cold argv와 hot
single-instance relaunch를 모두 one-shot `PendingOpen`으로 모으고, frontend listener는 event
payload를 직접 적용하지 않고 pending slot을 pull한다. Path는 canonical Knowledge root 내부의
bounded Markdown 파일로 해석·읽기까지 backend에서 수행하고, Query는 bounded trim 뒤 기존
FTS 검색 상태로 연결한다. 잘못된 요청은 창을 유지한 채 raw 입력 없는 복구 가능한 오류로
표시한다.

Everything+는 catalog revision 3부터 `query`를 수신한다. Knowledge와 같은 listener-first
`PendingOpen` 경로로 cold/hot request를 한 번만 적용하고, 유효한 bounded Query는 name 모드와
non-regex 상태의 기존 검색 pipeline에 연결한다. invalid/unsupported request는 raw query를
반향하지 않는 오류로 표시하며 index, root, saved-query 상태는 변경하지 않는다.

Everything+의 content-enabled root는 explicit source/Markdown/plain-text allowlist를 거친
파일만 bounded extractor로 보낸다. extractor는 UTF-8과 UTF-16 LE/BE를 strict decode하고
파일 20 MiB·text 2,000,000 Unicode scalar characters·candidate 10초 processing budget을
공통 적용한다. `file_content`는 `content_status`, `extractor_version`, `truncated`,
`indexed_at`, `error_code`, `encoding`, `text_chars`를 소유하며, 실패 상태의 body는 빈
FTS row라 filename search를 가리지 않는다. sensitive filename은 읽기 전에 skip하고,
검색 snippet도 common credential/private-key와 provider token·AWS access key·JWT pattern을
redaction한 뒤 4,096자 cap을 통과해야 UI로 나간다. 파일명 검색은 regex prefilter의 기존
2,000개 상한을 보존하고 content 결과만 200개로 제한한다. full scan과 watcher incremental
path가 크기·mtime을 읽기 전후에 다시 확인하는 같은 extractor를 공유하며,
network/external tool/OCR/Office/semantic processing은 이 BASE 경계에 없다. PDF는
별도 `pdf-v1` format extractor가 MIT `lopdf`로 text object만 bounded offline 추출하며,
파일 20 MiB·decompressed page/object stream 16 MiB·parsed object 100,000개·page 10,000개와
text 2,000,000자·candidate 10초 상한을 적용한다. object/page 구조 상한 초과는
`content_status=extract_error`, `error_code=resource_limit`으로, image-only scan/encrypted/
corrupt PDF는 각각 `no_text`/`unsupported_encrypted`/`extract_error` metadata로 격리한다.
`meta.pdf_extractor_version`이 없거나 현재 `pdf-v1`과 다르면 첫 설치/버전 전환 PDF-only
reindex를 수행하고, 성공한 full/PDF scan 뒤 marker를 기록한다. PDF-only reindex 중 큐에
새 root/index 요청이 들어오면 다음 실행을 `All`로 승격해 요청을 놓치지 않는다. OCR·image·
format extraction은 이 경계에 포함하지 않는다.

legacy XLS는 별도 `xls-v1` extractor가 MIT pure-Rust `calamine::Xls`로 worksheet 셀 값만
bounded offline 추출한다. pure-Rust `cfb` preflight가 calamine의 eager range allocation 전에
Workbook stream의 구조와 sheet/dimension/record/formula/SST 상한을 fail-closed로 검증하고,
unique SST text와 `LabelSst` clone 확장량 및 256 MiB 추정 peak memory를 별도로 계산한다. 수식 재계산,
VBA/macro, image/style, 외부 resource는 사용하지 않는다. encrypted/corrupt/resource-limit XLS는
각각 `unsupported_encrypted`/`extract_error`/`resource_limit` 고정 코드로 격리한다.
`meta.xls_extractor_version`이 없거나 현재 `xls-v1`과 다르면 XLS-only reindex를 수행하고,
성공한 full/XLS scan 뒤 marker를 기록한다. 각 format-only worker의 queued restart는 `All`로
승격한다.

XLSX와 ODS는 각각 `xlsx-v1`/`ods-v1` extractor와 독립 완료 marker를 소유하지만 XLS와 같은
`FormatSet` reindex 상태 기계를 사용한다. startup은 누락/불일치 marker와 stale row를 format별로
합성하고, 성공한 full/선택-format scan만 해당 marker를 기록한다. partial-root/cancel/error pass는
marker를 기록하지 않으며, 새 root나 사용자 full-index 요청은 queued pass를 `All`로 승격한다.
clear와 candidate match는 bit set에 포함된 확장자만 대상으로 하므로 text/PDF/다른 spreadsheet
row가 보존된다.

두 modern spreadsheet 형식은 `calamine` 진입 전에 in-memory ZIP/XML admission을 거친다.
공통 ZIP envelope 검사는 EOCD/ZIP64 locator에서 선언 entry 수 4,096개 상한과 single-disk 구조를
먼저 확인해 central-directory metadata 대량 할당을 막은 뒤, `ZipArchive`의 실제 entry 수,
unsafe/중복 path, encryption, entry 32 MiB와 전체 uncompressed 64 MiB를 재검사한다. XLSX는
표준 package/workbook relationship root, external target/DTD 금지, XML depth 128/event 1,000,000,
shared string 1,000,000개·8,000,000자와 worksheet 좌표/논리 cell 4,000,000개를 검사하고,
calamine의 streaming cell API만 사용한다. ODS는 manifest encryption/DTD를 차단하고
`content.xml`의 sheet/row/column repeat와 non-empty value/formula clone 확장량을 계산한다.
calamine이 기존 row vector와 dense range vector를 동시에 보유하는 구간까지 반영해 Data/formula
slot을 각각 두 벌로 추정하며, expanded text 16,000,000자와 peak memory 256 MiB를 넘기면 parser
진입 전에 거부한다. 두 형식 모두 formula를 평가하지 않고 cached value만 text로 취급하며,
macro/image/style/external resource/network를 사용하지 않는다. OCR/semantic search는 계속
별도 후속 경계다.

DOCX는 spreadsheet 묶음과 별도의 `docx-v1` parser/rollback 경계를 가진다. Everything+에
이미 고정된 MIT `zip`과 `quick-xml`을 재사용하되 Office/LibreOffice나 sidecar를 설치·실행하지
않는다. raw EOCD/ZIP64 envelope에서 선언 entry 수를 먼저 제한하고, `ZipArchive` 생성 뒤 실제
entry 4,096개, case-insensitive duplicate/unsafe path, encrypted flag, entry 32 MiB와 전체
uncompressed 64 MiB를 다시 검사한다. canonical `[Content_Types].xml`, `_rels/.rels`,
`word/document.xml`만 main-document trust root로 인정하며 macro-enabled content type, external/
unsafe package target과 DTD는 거부한다. XML은 depth 128, event 1,000,000개, text Unicode
scalar와 raw attribute byte를 합산한 8,000,000 budget, relationship 4,096개 안에서 streaming
scan한다.

본문 계약은 `word/document.xml`의 `w:t`와 paragraph/tab/line-break를 FTS text로 정규화하는
것뿐이다. field instruction과 non-main part(header/footer/footnote/comment), image/style,
embedded object, macro는 읽거나 실행하지 않고 relationship target도 열지 않는다. output은 기존
2,000,000 Unicode scalar/10초 경계를 공유한다. OOXML CFB `EncryptedPackage`와 encrypted ZIP은
`unsupported_encrypted`, 빈 main text는 `no_text`, 손상/ZIP/XML/resource 실패는 raw path/parser
detail 없는 고정 코드와 빈 FTS body로 격리한다. `meta.docx_extractor_version` 누락/불일치 또는
stale row는 startup의 compact `FormatSet`에 DOCX bit를 더하고, 성공한 full/DOCX-only 전체-root
pass만 marker를 기록한다. DOCX-only clear/reindex는 text/PDF/XLS/XLSX/ODS row를 보존한다.

Repo Manager는 catalog revision 4부터 `path`를 수신한다. cold/hot request를 listener-first
`PendingOpen` 경로로 한 번만 소비하고 기존 scan 결과와 canonical identity가 같은 repository를
선택·focus한다. 목록에 없는 유효한 Git repository는 자동 등록하거나 Git 명령을 실행하지 않고
비지속 등록 초안으로 표시하며, 사용자가 명시적으로 선택할 때만 기존 read-only scan을 수행한다.
상대·traversal·누락·비-repository 경로는 raw path를 반향하지 않는 복구 가능한 오류가 된다.

Life Log는 catalog revision 5부터 `snapshot:life-log/projects/v1`을 생산한다. 등록된 안전한
절대 프로젝트 경로와 최근 7일의 마지막 활동 시각·세션 수·활동 시간만 `projects` view로
발행하고, 창 제목·앱명·원문 세션은 귀속 과정 밖으로 내보내지 않는다. 앱 시작, 프로젝트
설정 변경, 60초 주기 갱신은 같은 `life-log/v1/summary.json` 전체를 원자 교체한다.
Workbench는 이 view를 읽는 첫 consumer다. producer가 꺼진 동안에도 마지막 정상 snapshot은
사용하되 계산된 freshness를 유지하고, 전체 entry 검증을 마친 뒤에만 Workbench가 단독 소유한
`project-profiles.json`을 원자 교체한다. distro 정보가 없는 `/mnt` 밖 POSIX 경로에는 임의
distro를 붙이지 않는다.

Knowledge Base가 catalog에 선언한 `snapshot:knowledge-base/activity/v1`은 Life Log가 읽는
첫 consumer 계약이다. producer가 종료돼도 마지막 정상 snapshot과 계산된 view freshness를
사용한다. schema·payload 오류가 있으면 version/freshness 진단과 고정된 안전 오류를 Data
Sources에 표시하되, 불투명 note ID와 원문 입력은 frontend로 전달하지 않는다.

WSL Desktop은 catalog revision 6부터 `snapshot:wsl-desktop/runtime/v1`을 생산한다. producer는
`wsl-desktop/v1/summary.json` 하나에 `runtime` view를 원자적으로 발행하며, dashboard command와
background writer가 동일한 collection lock 아래 하나의 revision을 만든다. collection은
`wsl.exe -l -v`로 distro/state와 session의 distro별 terminal 수를 한 번 캡처하고, Running
distro에만 Docker와 numeric resource query를 순차 실행한다. 화면의 `DashboardSnapshot`은 이
generation의 distro, terminal count, Docker availability/container state 및 CPU 사용률·memory/
disk used/total을 함께 소유하고, runtime view는 기존 Workbench 호환 필드의 같은 generation을
원자적으로 발행한다. stopped distro는 resource/Docker query 때문에 시작하지 않는다.

공개되는 runtime 값은 validated container state/name/hex ID와 published `portMappings`이며,
bounded numeric resource summary는 화면 전용 `DashboardSnapshot` IPC에만 존재한다. WSL/Docker
path·command·env·credential·image·raw status/ports·terminal session identity는 snapshot과 오류에서
제외한다. `wsl.exe`, `docker`, `cat`,
`df` 호출은 fixed argv·5초 timeout·stdout/stderr bounds를 사용하며 shell/사용자 command/환경
확장·설치를 사용하지 않는다. child 5초·전체 collection 30초 deadline 또는 bounds를 넘은
malformed/partial 수집은 정상 빈 결과로 교체하지 않고 기존 last-good을 보존한다. dashboard
refresh는 single-flight로 중복 요청을 합치고 snapshot TTL마다 자동 재조회하며 UI는
`capturedAtMs + staleAfterMs`를 기준으로 loading/refreshing/fresh/stale/error를 표시한다.
stale/refreshing/error 동안 Docker mutation과 broadcast는 fail-closed하지만 단일 terminal PTY I/O는 계속 허용한다.
Workbench #281은 이 snapshot을 읽는 consumer이며 WSL Desktop producer PR에서는 Workbench 파일이나
Docker resource mutation을 수정하지 않는다.

Workbench는 이 runtime view의 첫 consumer다. 공용 discovery/read 경계 뒤에서 v1 payload 전체를
엄격하게 재검증하고 published TCP host port만 숫자순으로 묶으며, 같은 port의 distro/container/
target source는 안정적으로 정렬·중복 제거한다. 60초 producer cadence를 기준으로 2분 이하는 fresh,
2분 초과 15분 이하는 stale, 그 이후는 expired다. stale accept는 사용자 확인이 필요하고 expired,
missing, corrupt는 차단한다. accept 직전 snapshot을 다시 읽으며 결과는 현재 `ProfileDraft`에만
합쳐져 기존 port 순서를 보존한다. 이 경계는 profile store, WSL/Docker process, container와 Run
Manager service를 변경하지 않고 snapshot path·raw Docker detail·container ID를 frontend로 보내지
않는다.

`apps/catalog.json` 변경은 CI scope에서 양쪽 게이트(frontend/rust)를 켠다.

## 통합 앱 (Workbench)

`apps/workbench`는 기존 앱의 UI를 복제하는 통합 앱이 아니라, 프로젝트를 기준으로
여러 앱·서비스를 조정하고 상태를 요약하는 **orchestration 셸**이다. 기존 `crates/`·
`packages/`를 재사용하며, 결과물은 **독립 앱 15개**(workbench, Devbox Launcher, Log Lens 포함) 구조다.
상세: `docs/product-opportunities.md` §15.2, `docs/superpowers/specs/2026-08-14-workbench-design.md`

## 신규 앱 설계 문서

- `docs/superpowers/specs/2026-08-14-workbench-design.md` — Workbench (orchestration 셸)
- `docs/superpowers/specs/2026-08-14-webhook-lab-design.md` — Webhook Lab (로컬 웹훅 서버)
- `docs/superpowers/specs/2026-08-14-repo-manager-design.md` — Repo Manager (git worktree)
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` — v0.5.0 Devbox Launcher·Log Lens,
  기존 13개 앱 강화, handoff와 native-first 범위

상세 규약: [CONVENTIONS.md](../CONVENTIONS.md)
