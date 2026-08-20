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
│                              │  search, integration, secrets, git, launch, applink
├──────────────────────────────┤
│ 공통 인프라: Cargo workspace, │
│ pnpm workspace, git 모노레포,  │
│ apps/catalog.json (앱 단일 원본)│
└──────────────────────────────┘
```

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
  crates/integration◄── run-manager, workbench, knowledge-base 등 snapshot 계약
  crates/secrets    ◄── api-playground, run-manager (DPAPI)
  crates/git        ◄── devbox-manager, life-log, repo-manager, workbench
  crates/launch     ◄── repo-manager, workbench
```

## 앱별 데이터 흐름

```
port-manager:    React → invoke → commands → process crate → OS netstat
wsl-desktop:     React → invoke → commands → wsl crate → wsl.exe (wsl-dashboard 흡수)
                   └ distro·docker 패널 (gitStatus는 Workbench로 이관 완료)
life-log:        tray/poller(상시) → sessionizer → SQLite → commands → React
                   (activity-timeline 흡수. 외부 DB 직접 조회 없음 → integration snapshot 계약)
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

> **예외 (알려진 위반)**: workbench의 `absorb_life_log_projects`(`commands/workspace.rs`)가
> life-log의 `data.db`를 직접 SQLite로 읽는다 — 현재 이 정책의 유일한 예외다.
> life-log를 producer로 만들어 해소할 예정: `./superpowers/specs/2026-08-17-app-interop-design.md` §4.1

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

상세 규약: [CONVENTIONS.md](../CONVENTIONS.md)
