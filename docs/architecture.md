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
│ packages/*  React 공용       │  tokens, editor, diff-view
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
- 신규 `crates/window-state`와 `packages/context-menu`
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
  crates/launch     ◄── repo-manager, workbench
  crates/catalog    ◄── devbox-manager (후속에서 launch·capability 메뉴 소비자 확대)
```

## 앱별 데이터 흐름

```
port-manager:    React → invoke → commands → process crate → OS netstat
wsl-desktop:     React → invoke → commands → wsl crate → wsl.exe (wsl-dashboard 흡수)
                   └ distro·docker 패널 (gitStatus는 Workbench로 이관 완료)
life-log:        tray/poller(상시) → sessionizer → SQLite → commands → React
                   (activity-timeline 흡수. crates/integration으로 snapshot을 자동 발견하며
                   외부 DB 직접 조회 없음)
everything-plus:  indexer/watcher → filesystem crate → search crate(FTS5) → React
knowledge-base:   fs_store → filesystem/search crate → React(CodeMirror)
api-playground:   React → commands → reqwest → HTTP
code-pad:         React(CodeMirror) → commands → LSP stdio 서버, filesystem/markdown crate → React
run-manager:      React → commands → scheduler → platform 실행 어댑터(Windows Job Object/WSL) → SQLite
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

## 보안 경계

각 앱이 다루는 외부 입력과 그 방어선:

| 방어선 | 위치 | 무엇을 막는가 |
|---|---|---|
| `ammonia` HTML 살균 | `crates/markdown` `sanitize()` | 마크다운 HTML의 `<script>` 제거, `javascript:` URI 차단 |
| mermaid `securityLevel: "strict"` | code-pad `PreviewPane`, knowledge-base `MarkdownPreview` | 다이어그램 HTML의 XSS |
| CSP (`csp` 정책) | 각 앱 `tauri.conf.json` | DOM injection 시에도 임의 `invoke`/네트워크 접근 차단 |

`csp: null` + `core:default` 조합은 DOM injection이 성립하면 곧바로 `invoke`에 닿게 만든다.
앱들이 임의 로컬 파일(code-pad, knowledge-base, everything-plus)과 임의 원격 응답
(api-playground)을 다루므로 명시적 CSP 정책을 둔다. (상세: `docs/product-opportunities.md` §7.5)

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
