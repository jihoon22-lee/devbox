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
│ apps/*   독립 Tauri 앱 (.exe) │  13개
├──────────────────────────────┤
│ packages/*  React 공용       │  tokens, editor, diff-view, context-menu
├──────────────────────────────┤
│ crates/*    Rust 공용        │  filesystem, markdown, process, wsl,
│                              │  search, integration, secrets, git, launch,
│                              │  applink, catalog
├──────────────────────────────┤
│ 공통 인프라: Cargo workspace, │
│ pnpm workspace, git 모노레포,  │
│ apps/catalog.json (앱 단일 원본)│
└──────────────────────────────┘
```

위 그림은 v0.5.0 개발 중인 현재 구조다. 기존 동작을 유지하면서 다음 계획 요소를
순차적으로 추가한다. 구현 전인 항목은 현재 앱/크레이트 수에 포함하지 않는다.

- 신규 독립 앱 `devbox-launcher`, `log-lens` — 목표 15개 앱
- 구현된 순수 `crates/catalog` — catalog v1/v2 type·revision freshness·runtime/build-time
  fallback·capability filter. runtime file I/O는 후속 Manager 기능이 담당한다.
- 신규 `crates/logs` — Log Lens가 두 번째 소비자가 되는 시점의 순수 log parsing
- 구현된 `packages/context-menu` — 위치·keyboard navigation·focus restore·submenu·separator·
  disabled/danger 표현만 소유한다. Port Manager, Developer Toolbox, Everything+, Knowledge, Code Pad,
  Run Manager, Devbox Manager, Workbench, Webhook Lab, Repo Manager, API Playground, WSL Desktop, Life Log의
  기존 13개 앱에 기능 단위로 적용됐다. 신규 앱은 처음부터 적용한다.
- 신규 `crates/window-state`
- `crates/applink` protocol v2 one-time handoff

상세: [`v0.5.0 네이티브 우선 계획`](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)

## 크레이트 의존 관계

```
  crates/filesystem ◄── api-playground, code-pad, developer-toolbox, devbox-manager,
                       everything-plus, knowledge-base, life-log, port-manager,
                       repo-manager, run-manager, wsl-desktop
  crates/applink    ◄── code-pad, repo-manager, wsl-desktop, workbench
  crates/markdown   ◄── knowledge-base, code-pad
  crates/process    ◄── port-manager, run-manager
  crates/wsl        ◄── wsl-desktop, run-manager, workbench, repo-manager
  crates/search     ◄── everything-plus, knowledge-base
  crates/integration◄── run-manager, workbench, knowledge-base, life-log 등 snapshot 계약·자동 발견
  crates/secrets    ◄── api-playground, run-manager (DPAPI)
  crates/git        ◄── devbox-manager, life-log, repo-manager, workbench
  crates/launch     ◄── everything-plus, knowledge-base, repo-manager, workbench
  crates/catalog    ◄── devbox-manager (후속에서 launch·capability 메뉴 소비자 확대)
```

## 앱별 데이터 흐름

```
port-manager:    React → invoke → commands → process crate → OS netstat
wsl-desktop:     React(xterm + pane/tab context-menu) → invoke → commands → wsl crate → wsl.exe
                   (wsl-dashboard 흡수; split/close/tab action은 exact ID와 확인 경계 사용)
                   └ distro·docker 패널 (`docker ps --format` 5필드 원문 → compact summary/detail,
                      gitStatus는 Workbench로 이관 완료)
life-log:        tray/poller(상시) → sessionizer → SQLite → commands → React(date context-menu)
                   (activity-timeline 흡수. crates/integration으로 snapshot을 자동 발견하며
                   Knowledge activity/v1을 검증·집계하고 외부 DB 직접 조회 없음)
everything-plus:  indexer/watcher → filesystem crate → search crate(FTS5) → React
knowledge-base:   fs_store → filesystem/search crate → React(CodeMirror + context-menu)
                   ├ Markdown 원문 → wikilink parser → SQLite 재생성 가능 key/link index
                   │  → autocomplete·unresolved decoration·backlink source position
                   ├ canonical tree entry → opener 또는 catalog/launch crate → 설치된 대상 앱
                   └ path/body-free activity/v1 snapshot → Life Log Data Sources
api-playground:   React(context-menu + History/Collection v2) → commands(secrets sanitizer)
                   → reqwest → HTTP
code-pad:         React(CodeMirror + tab/editor context-menu, bounded workspace Quick Open tree)
                   → commands → LSP stdio 서버, snapshot-checked sibling rename/delete,
                   filesystem/markdown crate → React
run-manager:      React(context-menu + bounded log export) → commands → scheduler
                   → platform 실행 어댑터(Windows Job Object/WSL) → SQLite + app-owned 회전 로그
devbox-manager:   React → commands → catalog/manifest → GitHub release asset
workbench:        React → commands → ProjectProfile/read-only health + 다른 앱 실행 (CLI argument,
                   v0.4.0에서는 argv 수신 부재로 미동작했으나, v0.4.1에서 crates/applink와
                   single-instance pending-open 수신을 Code Pad/WSL Desktop/Workbench에 구현.
                   v0.4.1은 이 핫픽스를 포함한 안정판으로 배포됐다. 남은 Windows packaged-runtime
                   acceptance는 [issue #176](https://github.com/jihoon22-lee/devbox/issues/176)에서
                   post-release로 계속 관리한다.
                   ./superpowers/specs/2026-08-17-app-interop-design.md)
webhook-lab:      inbound HTTP → core/server → history·rule·fixture → React
repo-manager:     React → commands → git crate(wsl) → repository/worktree 탐색·생성
```

API Playground의 History·Collection context menu는 v2에 저장되고 backend sanitizer read-back을
통과한 `PersistedHistoryRequest`만 복제·이름 변경·삭제·마스킹 cURL 복사의 입력으로 사용한다.
History의 선택적 표시 이름은 기존 v2 wire shape에 하위 호환되며 이름까지 sanitizer가 검사한다.
context action은 현재 editor의 raw request나 unsealed environment secret을 읽지 않는다. 삭제는
확인 전 storage를 변경하지 않고, 복제는 마스킹 request를 깊은 복사한 뒤 전체 store를 다시
sanitize/read-back 한다. Collection import/export와 앱 간 protocol은 각 후속 기능 경계에 남긴다.

API Playground response viewer의 일반 DTO는 마스킹된 headers와 값이 제거된 Cookie projection만
포함한다. 원문 response headers는 `Serialize`/`Debug`를 구현하지 않은 app-managed vault가 현재
요청 1건에 대해서만 최대 100개·64 KiB로 보관한다. 새 요청을 시작하면 이전 entry를 즉시
폐기하며 raw name/value 버퍼는 drop 때 zeroize한다. 이후 monotonically 증가하는 opaque string
ID를 발급하며, 늦게 끝난 과거 요청은 current ID와
일치하지 않아 raw entry를 저장하지 못한다. 상한 초과·비텍스트 header는 masked DTO에 안전한
표시만 남기고 raw copy 전체를 fail-closed한다. 확인된 Headers/Set-Cookie copy command만 ID로
원문을 조회하며 반환 문자열은 clipboard write 외 storage·history·log에 전달하지 않는다.

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

## 보안 경계

각 앱이 다루는 외부 입력과 그 방어선:

| 방어선 | 위치 | 무엇을 막는가 |
|---|---|---|
| `ammonia` HTML 살균 | `crates/markdown` `sanitize()` | 마크다운 HTML의 `<script>` 제거, `javascript:` URI 차단 |
| mermaid `securityLevel: "strict"` | code-pad `PreviewPane`, knowledge-base `MarkdownPreview` | 다이어그램 HTML의 XSS |
| CSP (`csp` 정책) | 각 앱 `tauri.conf.json` | DOM injection 시에도 임의 `invoke`/네트워크 접근 차단 |
| Clipboard 최소 권한 | Developer Toolbox·Knowledge·Code Pad `clipboard-manager:allow-read-text` | 명시적 Paste 이외의 image/write/clear IPC와 background clipboard 수집 차단 |

`csp: null` + `core:default` 조합은 DOM injection이 성립하면 곧바로 `invoke`에 닿게 만든다.
앱들이 임의 로컬 파일(code-pad, knowledge-base, everything-plus)과 임의 원격 응답
(api-playground)을 다루므로 명시적 CSP 정책을 둔다. (상세: `docs/product-opportunities.md` §7.5)

Developer Toolbox, Knowledge, Code Pad는 input/editor context menu에서 사용자가 붙여넣기를 선택한 순간에만
system clipboard의 plain text를 읽는다. 읽은 값은 현재 controlled input 또는 CodeMirror
selection에만 삽입하며 log, snapshot, settings에 기록하지 않는다. Copy는 기존 WebView clipboard
write 경로를 쓰고, Toolbox 결과 파일 저장은 사용자가 누른 항목에서 생성한 local text
download로만 수행한다.

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

Webhook Lab의 history/rule context menu도 열기 전에 대상의 opaque ID를 선택한다. 일반 history
DTO, 마스킹 복사, 헤더 복사는 Authorization·Cookie·API key 값을 마스킹하며, 원본 헤더를 가진
내부 entry는 Serialize/Debug를 구현하지 않고 process memory에만 최대 200건 유지한다. 요청별
보관 헤더는 100개·총 64K자, body는 256K자로 제한한다. raw copy command는 사용자가 별도 경고를
확인한 뒤에만 호출하고 반환값은 일회성 clipboard write 이외에는 저장·기록하지 않는다. 개별
history/rule 삭제와 전체 history 비우기는 기존 버튼을 포함해 확인을 요구하며, clear 뒤에도
프로세스 안의 history ID를 재사용하지 않는다. 별도 issue인 example curl과 `api-request/v1`
handoff가 준비되기 전에는 해당 메뉴 항목을 fail-closed로 비활성화한다.

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
identity를 다시 확인한다. portable 제거 전에는 app-owned tree 전체를 제한된 깊이·항목 수로 순회해
symlink, Windows reparse point, 특수 파일을 거부한 뒤 해당 app tree만 삭제한다. 별도 app-local user
data는 이 경계 밖에 있어 보존된다. registry를 먼저 원자 갱신하고 제거가 실패하면 원래 registry를
복원한다. installer lifecycle과 custom install root 이동·제거는 실제 소유 manifest가 추가되는 P2
기능 전까지 경로를 추측하지 않는다.

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

### CSP 기준선

13개 앱 전부 다음 최소 기준선을 쓴다 (PR 17 + 신규 앱 반영).

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
capability target만 반환한다. locator가 없거나 손상된 v0.4.x 환경에 한해서만 기존 고정
Manager root를 read-only fallback으로 읽으며, 유효한 locator 뒤의 manifest/path 오류는
fail-closed 처리한다. `crates/applink`는 argv 계약만 담당해 `launch`와의 순환 의존을
피한다.

고정된 공용 metadata 경계는 다음과 같다.

| 파일 | 소유자 | 소비자 | freshness/안전 조건 |
|---|---|---|---|
| `%LOCALAPPDATA%\devbox\catalog.json` | Devbox Manager | `crates/catalog`, Manager, 메뉴 소비 앱 | 유효한 v2이며 build-time `catalogRevision` 이상일 때만 runtime 우선 |
| `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json` | Devbox Manager | `crates/launch` | 양수 `registryRevision`, catalog provenance, canonical root/manifest |
| `<manager-root>\registry.json` | Devbox Manager | Manager, `crates/launch` | app/version/mode와 exact portable executable layout 일치 |

실제 custom root 이동·제거 UI는 이 locator 계약의 후속 기능이며, locator에는 root 자체나
설치 목록을 복제하지 않고 app-owned manifest 위치만 둔다.

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

`apps/catalog.json` 변경은 CI scope에서 양쪽 게이트(frontend/rust)를 켠다.

## 통합 앱 (Workbench)

`apps/workbench`는 기존 앱의 UI를 복제하는 통합 앱이 아니라, 프로젝트를 기준으로
여러 앱·서비스를 조정하고 상태를 요약하는 **orchestration 셸**이다. 기존 `crates/`·
`packages/`를 재사용하며, 결과물은 **독립 앱 13개**(workbench 포함) 구조다.
상세: `docs/product-opportunities.md` §15.2, `docs/superpowers/specs/2026-08-14-workbench-design.md`

## 신규 앱 설계 문서

- `docs/superpowers/specs/2026-08-14-workbench-design.md` — Workbench (orchestration 셸)
- `docs/superpowers/specs/2026-08-14-webhook-lab-design.md` — Webhook Lab (로컬 웹훅 서버)
- `docs/superpowers/specs/2026-08-14-repo-manager-design.md` — Repo Manager (git worktree)
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` — v0.5.0 Devbox Launcher·Log Lens,
  기존 13개 앱 강화, handoff와 native-first 범위

상세 규약: [CONVENTIONS.md](../CONVENTIONS.md)
