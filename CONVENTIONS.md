# devbox — 공통 개발 규약

**현재 공개 v0.7.0 기준:** 15개 앱(port-manager, developer-toolbox, wsl-desktop, api-playground, everything-plus, knowledge-base,
life-log, devbox-manager, code-pad, run-manager, workbench, webhook-lab, repo-manager, devbox-launcher, log-lens)을 하나의 저장소에서 관리하되,
각각은 **독립적으로 실행되고 독립적으로 .exe가 만들어지는 Tauri 앱**이다. 소스 저장소와 공통 코드만 공유한다.

v0.8.0은 Workspace / API Studio / Knowledge / Control Center 네 제품으로 전환한다.
전환 중에는 v0.7 공개 topology를 유지하며, 신규 topology는 별도 검증한다.
구현 상태와 목표를 혼동하지 않는다. v0.8 PR 정책은 §8, 작업 도구 운영은 §11을 따른다.

```
devbox/
├─ apps/          # 각각 독립 Tauri 앱 (독립 .exe 생성)
├─ packages/      # 공용 React 패키지 (필요해지면 생성)
├─ crates/        # 공용 Rust 크레이트 (필요해지면 생성)
├─ docs/          # architecture / roadmap / projects
├─ Cargo.toml     # Cargo workspace
├─ pnpm-workspace.yaml
└─ package.json
```

> 공통화 원칙: **두 번 이상 실제로 필요해진 코드만** `packages/`·`crates/`로 추출한다.
> 처음부터 공용 패키지를 미리 만들지 않는다. 첫 앱(port-manager)은 앱 안에 코드를 두고,
> 두 번째 앱에서 같은 코드가 필요해질 때 그때 추출한다.

## 1. 개발 환경

| 항목 | 값 |
|---|---|
| 타깃 OS | Windows 10/11 (WebView2 내장) |
| 개발 OS | WSL2 Ubuntu + Windows (편집·로컬 검증은 WSL, 앱 실행·패키징은 Windows) |
| 소스 위치 | `/home/jihoon/projects/devbox/apps/<AppName>` (Windows: `\\wsl.localhost\Ubuntu\home\jihoon\projects\devbox\apps\<AppName>`) |
| 에디터 | 자유 (Rust-analyzer + ESLint + Prettier 권장) |
| 프론트 패키지 매니저 | **pnpm** (workspace) |
| Rust 빌드 | **Cargo workspace** (루트 `Cargo.toml`) |

### 빌드 원칙
- **개발(핫리로드)**: Windows PowerShell에서 `pnpm tauri dev` (각 앱 디렉터리에서)
- **배포 빌드**: Windows PowerShell에서 `pnpm tauri build`
- Windows toolchain은 `\\wsl.localhost\Ubuntu\home\jihoon\projects\devbox` UNC source를 사용한다.
  VHD의 물리적 E: 저장 위치를 `E:\projects` source path로 오인하지 않는다.
- WSL은 편집·git·React dev server와 frontend build/test/typecheck, Rust test/check/clippy/fmt에 사용한다.
  Rust 사용 전 새 셸에서 `source ~/.cargo/env`를 실행한다.
- 실제 앱 실행·배포 빌드는 **Windows**에서 수행한다. Windows에는 Rust MSVC 툴체인을 설치한다
  (`winget install Rustlang.Rustup`). WSL 컴파일 통과를 Windows 실행 PASS로 취급하지 않는다.
- 크로스 컴파일(`cargo-xwin`)은 공식 지원하나 비권장 → 일상 빌드는 Windows 툴체인 고정

## 2. 저장소 구조

```
devbox/
├─ apps/
│  ├─ port-manager/        # Port & Process Manager (최초)
│  ├─ developer-toolbox/   # 개발 도구 모음
│  ├─ wsl-desktop/         # 임베디드 WSL 터미널 (wsl-dashboard 흡수)
│  ├─ api-playground/      # REST API 테스트
│  ├─ everything-plus/     # 로컬 파일 검색
│  ├─ knowledge-base/      # 마크다운 지식 저장소
│  ├─ life-log/            # 자동 일일 로그 (집계 허브, activity-timeline 흡수)
│  ├─ devbox-manager/      # devbox 앱 설치·업데이트·실행 (+ 환경 진단)
│  ├─ code-pad/            # CodeMirror 6 경량 코드 에디터 (LSP)
│  ├─ run-manager/         # 예약 실행·서비스 관리
│  ├─ workbench/           # 프로젝트 기반 orchestration 셸
│  ├─ webhook-lab/         # 로컬 웹훅/콜백 서버
│  ├─ repo-manager/        # git 저장소·worktree 관리
│  ├─ devbox-launcher/     # catalog·snapshot 기반 빠른 실행
│  └─ log-lens/            # bounded local/WSL/container 로그 검사
│
├─ packages/               # React 공용
│  ├─ tokens/              # 15개 release 앱의 공용 CSS 커스텀 프로퍼티
│  ├─ a11y/                # keyboard·IME·dialog·axe 공통 계약
│  ├─ editor/              # CodeMirror 공용 설정      (knowledge-base, code-pad)
│  ├─ diff-view/           # diff 렌더 공용            (code-pad, run-manager)
│  ├─ context-menu/        # 위치·keyboard·focus·submenu 동작
│  ├─ openapi/             # bounded OpenAPI JSON/YAML parsing
│  ├─ api-studio-features/ # API·Webhook·Transforms UI (legacy 앱, API Studio)
│  ├─ workspace-features/  # Overview·Source·Files UI (legacy 앱, Workspace)
│  └─ mermaid-renderer/    # 필요할 때만 불러오는 Markdown diagram renderer
│
├─ crates/                 # Rust 공용
│  ├─ applink/             # 앱 간 one-time typed handoff와 single-instance 수신 계약
│  ├─ api-protocols/       # API 프로토콜 validation/codec (API Playground, API Studio)
│  ├─ webhook-core/        # Webhook fixture/rule/replay (Webhook Lab, API Studio)
│  ├─ transforms-core/     # Transform codec/workflow (Toolbox, API Studio)
│  ├─ data-migration/      # consistent snapshot/transaction (Control Center, API Studio)
│  ├─ catalog/             # build/runtime app catalog
│  ├─ filesystem/          # 파일 walk/검색 순회  (everything-plus, code-pad)
│  ├─ git/                 # Windows/WSL Git argv·identity 경계
│  ├─ launch/              # catalog 기반 설치 앱 실행
│  ├─ markdown/            # 마크다운 렌더          (knowledge-base, code-pad)
│  ├─ process/             # 프로세스/포트 조회·kill  (port-manager, run-manager)
│  ├─ wsl/                 # WSL argv·경로 정규화    (wsl-desktop, run-manager, workbench, repo-manager)
│  ├─ search/              # FTS5 쿼리 빌더          (everything-plus, knowledge-base)
│  ├─ integration/         # 앱 간 snapshot 계약      (run-manager, workbench, knowledge-base)
│  ├─ secrets/             # DPAPI 비밀 보호          (api-playground, run-manager)
│  ├─ window-state/        # monitor/DPI-safe 순수 geometry
│  └─ window-state-tauri/  # persistent Tauri window adapter
│
├─ docs/
│  ├─ architecture.md
│  ├─ roadmap.md
│  └─ projects.md
│
├─ Cargo.toml
├─ package.json
├─ pnpm-workspace.yaml
├─ README.md
└─ .gitignore
```

- 앱 이름은 **kebab-case** (`port-manager`) — 디렉터리·git 브랜치·crate 의존에 사용
- 앱별 Rust 크레이트 이름은 `_` → `-` 변환 후 사용: `port-manager` → `port_manager`
- 각 앱의 product 산출물: `PortManager.exe`, `DevToolbox.exe`, `WSLDesktop.exe`,
  `ApiPlayground.exe`, `EverythingPlus.exe`, `Knowledge.exe`, `LifeLog.exe`,
  `DevboxManager.exe`, `Code Pad.exe`, `Run Manager.exe`, `Workbench.exe`, `WebhookLab.exe`,
  `RepoManager.exe`, `DevboxLauncher.exe`, `LogLens.exe`

## 3. 공통 기술 스택

### 백엔드 (Rust, apps/<app>/src-tauri/)
- Tauri **v2** (tauri + tauri-build)
- Rust edition 2021 이상, MSVC 툴체인
- 필수 크레이트 (필요한 것만):
  - `serde`, `serde_json` (직렬화)
  - `anyhow`/`thiserror` (에러)
  - `log`, `env_logger` (로깅)
  - `tauri-plugin-opener` (외부 실행/브라우저 열기)
- DB: `rusqlite` (`bundled` + `fts5`)
- 시스템: `sysinfo`, `windows` crate / HTTP: `reqwest` / 파일 감시: `notify`
- 앱 간 중복 발견 시 → `crates/<domain>`으로 추출

### 프론트엔드 (React, apps/<app>/src/)
- Vite + **React 19 + TypeScript(엄격 모드)**
- 스타일: **순수 CSS (앱별 `App.css`)**. 공용 토큰은 `packages/tokens` (`@devbox/tokens`)
- 편집기: `@codemirror/*` 직접 사용 (code-pad). 공용 설정은 `packages/editor` (추출 완료, knowledge-base·code-pad 사용)
- 컨텍스트 메뉴: 위치·keyboard·focus·submenu·상태 표현은 `packages/context-menu`; 항목·action·파괴적 확인은 각 앱이 소유
- 다이어그램: `mermaid` (code-pad, knowledge-base만)
- Tauri API: `@tauri-apps/api`
- **선언했으나 실제 사용이 없는 라이브러리**(`lucide-react`, `zustand`, `@tanstack/react-table`,
  `recharts`, `react-router-dom`)는 **필요해지면 그때 도입한다.** 미리 선언하지 않는다.
- 중복 발견 시 → `packages/<name>`으로 추출

### 데이터 위치 규약
Tauri의 `app_local_data_dir()`을 사용하며, 이는 **번들 identifier 기준 폴더**다.
```
%LOCALAPPDATA%\{identifier}\    # 예: %LOCALAPPDATA%\com.devbox.lifelog\
```
- SQLite: `%LOCALAPPDATA%\{identifier}\data.db`, 설정: `config.json`
- 앱별 identifier: `com.devbox.activitytimeline`, `com.devbox.everythingplus`,
  `com.devbox.knowledgebase`, `com.devbox.lifelog` 등
- 앱 간 데이터 교환은 상대 앱의 `app_local_data_dir`을 직접 읽지 않고
  `%LOCALAPPDATA%\devbox\integration\<app-id>\v<n>\`의 read-only snapshot을 사용한다.
  (상세: `docs/product-opportunities.md` §10.1)

## 4. 코드 규약

### Tauri command 패턴
- Rust 함수명: `snake_case`, `#[tauri::command]`
- 프론트 호출: `invoke('command_name')` (camelCase)
- command 파라미터는 `serde` 구조체로 묶는다
- 모든 IO/장시간 작업은 `async fn` + 로딩 상태 UI
- 결과 반환: `Result<T, String>` (에러는 사용자 메시지로)

### 앱별 Rust 모듈 구조 (apps/<app>/src-tauri/)
```
lib.rs            # run() 진입점, command 등록, 상태 초기화
main.rs           # 단순 main (lib 호출)
commands/         # Tauri command 레이어 (얇게)
    mod.rs
    <feature>.rs
core/             # 앱 로컬 순수 로직 (파서, 집계) — OS 의존 없음, WSL에서 cargo test
db.rs             # SQLite 초기화/마이그레이션 (해당 시)
error.rs          # AppError
```
- **core → crates/ 추출 기준**: 같은 도메인 코드가 두 번째 앱에서 필요해지면
  `apps/<app>/src-tauri/src/core/<domain>.rs`를 `crates/<domain>/`로 옮기고
  Cargo workspace `members`에 추가, 해당 앱들은 `path` 의존으로 연결
- crates 안에 Windows 전용 코드(`windows` crate 등)를 넣지 않는다 (WSL에서 테스트 유지)
- `#[tauri::command]`는 얇게, 도메인 로직은 core/crates로

### 프론트엔드 구조 (apps/<app>/src/)
```
src/
  App.tsx         # 라우팅/레이아웃
  types.ts        # 도메인 타입 (Rust 구조체와 1:1)
  api.ts          # invoke() 래퍼 함수 모음
  pages/          # 페이지
  components/     # 공용/도메인 컴포넌트
  store/          # 상태 관리 스토어 (해당 시)
  lib/            # 순수 유틸 (포맷터 등)
```

### 명명/스타일
- UI 문구: 한국어, 코드·식별자·git 메시지: 영어
- 앱 이름: kebab-case, Rust 크레이트: snake_case, 패키지: `@devbox/<name>`

### 버전 규칙 (단일 원본)
> 앱 버전은 `src-tauri/Cargo.toml`을 원본으로 하고, `src-tauri/tauri.conf.json`과
> `package.json`은 항상 같은 값을 갖는다. 버전을 올릴 때 세 파일을 함께 수정한다.

- 앱 버전은 release tag와 독립적이다. release tag는 배포 일괄 단위일 뿐 앱 버전이 아니다.
- `package.json`의 버전이 `Cargo.toml`과 어긋난 상태로 커밋하지 않는다.

### 릴리스 검증 경계

- 현재 v0.7은 15개 앱·32개 public asset 계약을 유지한다. v0.8 목표는 구현 완료와 구분한다.
- 안정판은 exact-main Windows candidate의 assembly·packaged runtime·installer acceptance 통과 후,
  동일 commit의 annotated tag로 검증된 후보만 승격한다. 후보 부재·만료 시 새 build로 대체하지 않는다.
- Stable verifier의 `always()` 및 preflight/draft-stage 명시적 success 조건을 유지한다.
- 릴리스 작업 시 [릴리스 실행 정책](./docs/release-policy.md)을 읽는다. 명시 요청 없는 RC는 만들지 않는다.

## 5. 개발 워크플로 (WSL-first)

| 단계 | 명령 | 위치 |
|---|---|---|
| 커밋 전 최소 확인 | 변경 diff·계획 대조, 필요한 문법·타입 검사 | 변경한 앱/모듈 |
| PR 구현 완료 후 상세 검증 | `pnpm verify:affected` 및 미포함 수용 검사 | 워크스페이스 루트 |
| 명시적 전체 감사 | `pnpm verify:all` | 워크스페이스 루트 |
| 프론트 UI 미리보기 | `pnpm dev` (mock 데이터) | `apps/<app>` |
| 프론트 타입/빌드 검증 | `pnpm build` | `apps/<app>` |
| 실제 앱 실행 | `pnpm tauri dev` | Windows PowerShell, `apps/<app>` |
| 배포 빌드 | `pnpm tauri build` | Windows PowerShell, `apps/<app>` |

- 검증은 커밋 횟수에 맞추지 않는다. 커밋 전에는 diff를 계획과 대조하고 문법·타입 오류를
  확인하는 데 필요한 최소 검사만 선택한다. 문서 변경은 내용·링크·diff 확인으로 충분하며,
  코드 변경은 편집기 진단이나 필요한 대상의 typecheck/`cargo check`를 활용한다.
  작은 수정마다 test·Clippy·build·affected를 연속 실행하는 절차는 금지한다.
- 상세 검증은 PR에 계획한 구현·importer·fixture가 모두 끝났을 때 수행한다. 먼저 수용 기준과
  검사 항목을 대응시키고, `verify:affected`에 포함된 테스트·타입·빌드·lint를 별도 명령으로
  선행 반복하지 않는다. 포함되지 않은 회귀·migration·Windows/WSL 실기 검사는 이때 함께
  수행한다. v0.8의 완료 시점은 B01~B09 각각의 PR 묶음 전체를 기준으로 한다.
- 구현 도중 상세 검사는 재현 없이는 해결할 수 없는 구체적 결함·설계 불확실성에 한해
  필요한 최소 범위로 실행한다. 이미 통과한 검사는 관련 변경·실패·새 위험이 없으면 반복하지
  않는다. 실패 수정 후에는 해당 실패와 영향을 받은 범위부터 확인하며, 문서 정리나 커밋
  생성만을 이유로 전체 검증을 다시 시작하지 않는다. 실행 시점·범위는 [검증 운영](./docs/verification.md)을 따른다.
- WSL 컴파일엔 Linux 시스템 라이브러리 필요:
  `libwebkit2gtk-4.1-dev libgtk-3-dev build-essential libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev patchelf`
- 프론트는 `src/lib/isTauri.ts` 분기로 Tauri 없이 mock 데이터 표시
- 9p 마운트 성능: cargo `target-dir`은 `.cargo/config.toml`로 Linux 네이티브 경로 지정
- `verify:affected`는 `origin/main`과 현재 branch의 merge-base 이후 commit 및 staged,
  unstaged, untracked 파일을 합친다. 앱 변경은 해당 앱만, 공용 package/crate 변경은
  dependency graph의 역의존 closure만 검사한다. 미분류 경로·lockfile 단독 변경은 fail-safe로
  전체 검증한다. 영향이 없는 CI job은 runner 할당 전에 skip하며, release와 주간 CI 감사는
  전체 검증을 유지한다.
- 로컬 테스트는 기존 서비스의 실행 상태·Docker 데몬·방화벽·공유 네트워크를 변경하지 않는다.
  전용 WSL 배포판, 별도 Docker socket/data-root, 고유 container 이름은 네트워크 격리 증거가
  아니다. Docker 설치/데몬 시작·종료/container·network 조작, iptables/nftables/라우팅 변경,
  WSL 전역 재시작처럼 공유 시스템에 영향을 줄 수 있는 검사는 일회성 hosted CI 또는 네트워크가
  독립된 VM에서만 실행한다. 로컬 차단을 환경 변수·사설 스크립트·daemon 옵션으로 우회하지
  않는다. 격리된 실행 환경이 없으면 해당 실기 항목을 미실행으로 남기고 구현은 계속한다.
  기존 서비스 재시작·방화벽 복원은 별도의 명시적 복구 요청과 확인된 근거 없이 수행하지 않는다.
- 로컬 `verify:affected/all`은 패키지 1개, Vitest worker 2개, Cargo job 2개, Rust test thread
  2개로 제한한다. CPU 4개·nice +10을 적용하고, 지원 호스트에서는 메모리 high 6GiB/max 8GiB,
  swap max 1GiB를 검증 process scope에 적용한다. worktree 간 검증은 한 번에 하나만 실행한다.
  CI는 독립 runner의 기존 동시성을 유지한다. 조정·실패·측정은 [검증 운영](./docs/verification.md).
- 검증기 변경에서 affected가 `all`이면 한 번의 전체 실행이 `verify:all`과 affected의 compiler/test
  검증을 함께 충족한다. 명령 이름만 바꿔 같은 전체 검증을 반복하지 않는다.

## 6. 프로젝트 시작 절차 (앱 추가 시)

```bash
# 루트에서 pnpm 워크스페이스 준비 (최초 1회)
corepack enable pnpm            # 또는 pnpm 직접 설치

# 앱 스캐폴드 (apps/ 밑에)
cd apps
pnpm create tauri-app@latest --name <app-name> --template react-ts --manager pnpm --identifier com.devbox.<appname> --yes
```
> 주의: `--yes` 대신 `--` 구분자를 쓰면 `--name`이 리터럴 폴더로 생성되는 이슈가 있다.
> 생성 직후 파일 4곳의 `--name`을 실제 이름으로 교체한다:
> `package.json`(name), `src-tauri/Cargo.toml`(name·lib name), `src-tauri/tauri.conf.json`(productName·title), `index.html`(title).

이후 진행 순서:
1. 앱 로컬 `core/` + 테스트 작성 (Rust 로직)
2. 커밋 전 최소 문법·타입·범위 확인; PR 구현 완료 후 루트 `pnpm verify:affected`와 필요한 수용 검사 수행
3. 두 번째 앱에서 중복 코드 발생 시 → `crates/`·`packages/`로 추출
4. 최종: Windows에서 `pnpm tauri dev/build`

## 7. 개발 순서 (현재 15개)
```
Phase 1: port-manager → developer-toolbox     # Tauri 기본기 (IPC, Rust 기초, 설정)
Phase 2: api-playground → everything-plus      # 자식 프로세스, async, HTTP, 상태관리
Phase 3: knowledge-base → life-log             # 개인 데이터 플랫폼 통합
추가:    wsl-desktop, devbox-manager           # PTY 터미널, 앱 설치·업데이트
추가:    code-pad, run-manager                 # 경량 코드 에디터(LSP), 예약 실행·서비스
Stage4:  workbench                             # 프로젝트 기반 orchestration 셸
Stage5:  webhook-lab, repo-manager             # 로컬 웹훅 서버, git worktree 관리
P3:      devbox-launcher, log-lens             # devbox 전용 진입점, bounded 로그 검사
```
- 현재 15개 앱 구현 완료. 진행 상황은 [docs/roadmap.md](./docs/roadmap.md) 참조
- 공통 코드 발견 시점에 `crates/process`, `crates/wsl`, `packages/tokens` 등을 하나씩 추출
- 각 프로젝트 상세는 `apps/<AppName>/README.md` 또는 설계 문서(`docs/superpowers/specs/`) 참조

## 8. Git 규약 (모노레포: `devbox/` 루트 1개 저장소)

### 저장소 구조
- 루트 1개 저장소. 모든 앱 + 공통 문서를 함께 관리
- 기본 브랜치: `main` (안정)
- GitHub 모노레포로 공개/관리 권장 (공통 코드 공유가 핵심)

### 브랜치 규칙
```
feat/<app>/<scope>     기능 개발   예: feat/port-manager/core-parser
fix/<app>/<scope>      버그 수정   예: fix/run-manager/docker-parse
chore/<scope>          잡다한 작업 예: chore/workspace/pnpm-setup
docs/<scope>           문서 작업   예: docs/roadmap
```
- `app`은 kebab-case 앱 이름, 공통 작업은 `workspace`/`crates`/`packages` 사용
- 기능 완성 후 `main`으로 merge (squash 또는 --no-ff)

### PR 단위 규칙

- 기준은 이슈 개수가 아니라 **사용자에게 하나로 보이는 기능 경계**다. 따라서 이슈와 PR은
  반드시 1:1일 필요가 없다.
- 같은 앱·같은 사용자 흐름에 속하고 구현 기반, 상태 모델, migration/reindex, 보안·자원
  제한, 테스트 fixture를 공유하는 형식별 변형이나 밀접한 보강은 여러 이슈를 한 PR로
  묶을 수 있다. 관련 README·architecture·roadmap·workthrough 갱신도 그 PR에 포함한다.
- 독립적으로 배포하거나 되돌려야 하는 작업, 권한·비밀·외부 mutation처럼 위험 경계가 다른
  작업, 선행 작업 없이는 검증할 수 없는 작업, 한 번에 리뷰하기 과도한 작업은 별도 PR로
  유지한다. 단순히 같은 앱이라는 이유만으로 묶지 않는다.
- 여러 이슈를 묶은 PR은 본문에 모든 이슈 번호, 묶는 이유, 이슈별 acceptance와 검증 결과를
  구분해 적는다. 수용 기준 전체를 충족한 이슈만 `Closes #...`로 닫고, 일부 기여는 `Refs #...`로
  연결한다. 각 이슈의 회귀 테스트를 준비하고, PR 전체 구현이 끝난 최종 통합 상태에서
  상세 검증·CI·Windows 수용 gate를 수행한다. 커밋별 상세 검증을 의무화하지 않는다.

### v0.8 통합 PR 정책 (일반 PR 단위 규칙보다 우선)

- 원장은 [#541](https://github.com/jihoon22-lee/devbox/issues/541), 수용 기준은
  [#542](https://github.com/jihoon22-lee/devbox/issues/542), 실행 계획은 #543~#551이다.
  작업 시작 시 최신 본문과 선행조건을 확인한다. 기본 검토 예산은 B01~B09의 9개 통합 묶음이며,
  11개 이슈를 11개 PR로 만들지 않는다.
- 같은 묶음의 UI·Rust·importer·fixture·문서를 하나의 PR에서 검토한다. 기계적 이동과 의미 변경은
  commit으로 구분한다. 독립적인 데이터 손실·보안·복구 위험 또는 리뷰 불가능의 구조적 근거가
  있을 때만 묶음을 재조정하고 원장에 근거와 매핑을 남긴다.
- B01 기반 확정 후 B02/B03/B04를 독립 진행할 수 있다. 공용 파일의 writer는 한 묶음이 소유한다.
  병렬 에이전트는 사용자가 요청하거나 적용 지침에서 허용할 때만 사용한다.
- review packet에는 semantic 변경, pure moves, 데이터·authority 변경, 요구사항/legacy parity 매핑,
  CI·실기 증거, 제한과 rollback을 담는다. PR 구현 중에는 문법·타입·계획 범위를 확인하고,
  구현 완료 후 필요한 위험 gate와 각 이슈의 회귀 검증을 모아 수행한다. 동일 검사를 커밋마다
  반복하거나 일부 구현만 끝난 상태에서 PR 완료 검증을 앞당기지 않는다.
- #541/#542는 구현 PR에서 자동으로 닫지 않는다. WP도 수용 기준 전체가 충족될 때만 닫는다.
  문서 반영·결과 기록만을 위한 PR은 기본 계획에 추가하지 않는다. 사용자가 별도 준비 작업을
  명시한 경우 그 범위만 독립 PR로 마무리하며 B01 구현 완료로 계산하지 않는다.
- 이 정책은 PR 구성에 대한 예외다. CI·권한·원본 데이터 보존·릴리스 검증 조건은 완화하지 않는다.

### 커밋 규칙 (Conventional Commits, 영어)
```
<type>(<scope>): <subject>

<type>: feat | fix | docs | refactor | test | chore | build | perf
<scope>: 앱 이름 또는 workspace/crates/packages
<subject>: 현재형 동사로 시작 (add, fix, update, extract, ...)
```
- 예: `feat(port-manager): add netstat parser with unit tests`
- 예: `refactor(workspace): extract process crate from port-manager`
- 1커밋 = 1논리적 단위. WIP 커밋 금지
- 커밋 전에는 해당 변경의 문법·타입 오류와 계획 범위 이탈을 최소한으로 확인한다.
  상세 테스트·Clippy·전체 빌드·`verify:affected`는 커밋별 의무가 아니다.
- PR의 계획한 구현이 모두 끝난 뒤 §5의 상세 검증을 수행한다. PR 최종 변경에 대해
  `.github/workflows/ci.yml` 통과를 확인한 뒤에만 main으로 머지한다. PR 수용 완료는
  필요한 회귀·실기 검사 + affected 검증 + GitHub Actions CI 통과로 판단한다.
- 머지/종료 시 직접 만든 전용 worktree가 clean이고 머지됐는지 확인한 뒤 worktree 제거,
  `git worktree prune`, 로컬 작업 브랜치 삭제, 원격 작업 브랜치 삭제 순으로 정리한다.
  활성·잠김·미머지·dirty 또는 호스트 소유 worktree는 삭제하지 않고 상황을 보고한다.
  최종 보고 전 `git worktree list`, 로컬·원격 브랜치 목록을 다시 확인한다.

## 9. 기술 스택 정책

> **스택 추가 기준.** 새 언어·프레임워크는 다음 셋을 **모두** 만족할 때만 도입한다.
> 1. Rust/TypeScript로 구현이 불가능하거나 비합리적인 능력이 필요하다
> 2. 그 능력에 막혀 있는 **구체적인 사용자 기능**이 존재한다 (가정이 아니라 실제 항목)
> 3. 배포·CI·업데이트 경로에 미치는 영향을 문서화했다
>
> **UI 프레임워크는 Tauri v2 하나로 고정한다.** 두 번째 UI 스택은 디자인 시스템·패키징·
> Manager 설치 모델을 전부 분기시킨다.
>
> **탈출구.** 라이브러리 접근이 목적이면 sidecar 프로세스로 도입한다. 버전 있는 JSON
> 계약, 타임아웃, 종료 보장을 갖춘다.

### 네이티브 우선·외부 도구 활용 정책

> **외부 도구에 유사 기능이 존재한다는 사실만으로 devbox 개발 대상에서 제외하지 않는다.**
> devbox의 첫 번째 목적은 개발자가 자주 쓰는 기능을 찾고 내려받고 별도 앱을 실행한 뒤
> 파일·클립보드로 결과를 다시 옮기는 반복을 줄이는 것이다.

- 외부 도구가 제공하는 기능도 제한 없이 검토한다. 다만 많이 쓰인다는 이유만으로 모두
  개발하지 않는다. 설치·사용이 쉽고 라이선스 제약이 없는 전문 도구는 devbox 안에서의
  반복 빈도, 오프라인 필요, 앱 간 handoff 가치가 낮을 때에만 개발 대상에서 제외한다.
  설치가 어렵거나 사용·라이선스에 제한이 있는 도구의 기능은 개발자에게 필수적인 하위
  기능에 한해 native 제공을 적극 검토한다.
- 개발자가 반복적으로 사용하는 핵심 흐름은 인터넷 연결이나 별도 도구 설치 없이 devbox
  안에서 완결되는 것을 우선한다. 단, WSL·Git·원격 API·container engine처럼 그 외부
  대상 자체를 다루는 기능은 대상의 설치 또는 연결을 전제로 할 수 있다.
- 허용 가능한 라이선스의 Rust/TypeScript 라이브러리와 소형 sidecar는 출처·버전·digest·
  설치 크기·보안성·업데이트 경로를 검토한 뒤 설치물에 포함할 수 있다. runtime download는
  핵심 기능의 전제 조건으로 사용하지 않는다.
- 전체 IDE·범용 DB GUI·container desktop처럼 규모가 크거나 전문 영역 전체를 다루는
  외부 도구는 선택적 설치·실행 보완재로 연결할 수 있다. 외부 도구 연결은 P1·P2 기능의
  기본 동작이나 유일한 구현이 될 수 없다.
- 제한적인 라이선스의 외부 도구가 제공하는 기능이 개발자에게 필수라면 해당 코드를
  포함·복사하지 않고 공개 규격과 관찰 가능한 동작에 기반해 필요한 범위만 독립 구현한다.
- devbox가 송신 앱과 수신 앱을 모두 제어할 수 있으면 versioned `applink`, one-time
  handoff, read-only snapshot을 사용한다. 사용자가 임시 파일 또는 클립보드로 데이터를
  운반하게 하는 흐름은 명시적 fallback으로만 둔다.
- 새 의존성 PR은 목적, 대안, 공식 출처, 고정 버전, 라이선스, 설치 크기, security advisory,
  오프라인 동작, 업데이트 담당을 기록한다. GPL·AGPL·SSPL·독점 runtime의 기본 번들은
  별도 승인 없이는 허용하지 않는다.
- Cargo와 pnpm 의존성은 `deny.toml`·`.github/dependency-policy.json`의 allowlist와 만료되는
  예외를 통과해야 한다. `THIRD_PARTY_NOTICES.md`는 lockfile에서 생성하며 모든 Tauri 앱의
  `bundle.resources`에 포함한다. 앱을 추가할 때 이 resource와 release notice asset 검증을
  함께 추가하고, notices를 수동 편집하지 않는다.

기능별 판단과 v0.5.0 적용 범위는
[`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`](./docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)를
기준으로 한다.

## 10. 통합 전략 (Workbench)
- `apps/workbench`를 프로젝트 기반 orchestration 셸로 구현했다 (구현 완료)
- workbench는 기존 `crates/`·`packages/`를 그대로 재사용 → 공통화가 통합을 쉽게 만든다
- Workbench까지 원래 13개 구조를 완성했고, Devbox Launcher와 Log Lens 추가 후 현재 독립 앱은 15개다.
- Workbench project environment는 native-first/offline 경계를 따른다. 사용자가 고른
  프로젝트 상대 `.env`/`.env.<name>`만 읽고, profile·IPC·snapshot·로그에는 원문 값을
  넣지 않는다. 저장되는 것은 source, 변수 이름, 충돌 상태, opaque revision과
  `crates/secrets`의 secret reference뿐이며, masked preview와 실행 직전 재검증을 거친
  ephemeral child-process overlay만 허용한다. 파일·변수·이름·값 상한, UTF-8/strict dotenv
  parser, canonical root·symlink/reparse 거부를 native와 UI 양쪽에서 적용한다.
- 이 경계는 global/system environment editor, cloud secret store, 다른 앱 DB 직접 수정,
  자동 `.env` 생성·수정·업로드를 포함하지 않는다. disabled configuration은 실행 시
  파일을 읽지 않으며, 빈 파일은 성공적인 no-op으로 표현한다. 중복·예약 이름·stale
  revision·secret backend 불가 상태는 fail-closed한다. project environment의 app/WSL/
  cwd/port/service preflight는 별도 #313 계약이며 이 규칙에 섞지 않는다.
- #312와 #313은 사용자에게는 하나의 `Start Workspace` review→continue 흐름으로 보이는
  grouped PR 후보지만 acceptance/rollback 경계는 독립이다. #313 preflight는 required app,
  distro/cwd, TCP port와 service snapshot을 read-only로 확인하고, warning/existing와
  Workbench-started provenance를 구분한다. preflight 실패는 environment read/child spawn을
  허용하지 않으며, service lifecycle/자동 복구는 여전히 Workbench 범위 밖이다.
- 상세: `docs/product-opportunities.md` §15.2, `docs/superpowers/specs/2026-08-14-workbench-design.md`

## 11. Codex 지침·스킬·작업 기록

- 루트 AGENTS는 필수 제약과 문서 탐색 경로를 담는다. 공통 규약은 이 문서가 원장이며,
  상세 절차는 관련 문서를 필요할 때 읽는다. 과거 SHA·workflow·실기 기록은
  [release evidence](./docs/release-evidence.md)에 보존한다.
- 대상 디렉터리의 `AGENTS.md`/`AGENTS.override.md`를 변경 전에 확인한다. 루트 세션에서
  모든 하위 지침이 자동 로드된다고 가정하지 않는다. 명세의 planned 상태를 구현된 동작으로 읽지 않는다.
- 저장소 스킬은 `.agents/skills/`에서 관리한다. `devbox-change`는 변경·검증·PR 절차,
  `devbox-migration-review`는 실제 migration/권한/복구 변경 검토에 사용한다.
  `devbox-release`는 명시적으로 호출할 때 사용한다. 스킬 호출 자체가 게시 권한을 추가하지 않는다.
- 스킬은 스택이나 승인 범위를 바꾸지 않는다. Next.js/ShadCN landing-page 절차를 devbox의
  Tauri/React/Vite/순수 CSS 제품 UI에 적용하지 않는다. 플러그인 cache의 스킬을 직접 수정하지 않는다.
- 모델·추론·컨텍스트·계정별 실험은 개인 `~/.codex/config.toml`에서 설정한다.
  개인 인증·절대 경로·구독 의존 설정을 공용 프로젝트 config로 복사하지 않는다.
  [호스트 설정과 확인 절차](./docs/codex-setup.md)를 참조한다.
- GitHub 연결/`gh`는 이슈·PR·CI 조회에, 로컬 셸은 파일·git·pnpm·Cargo 작업에 사용한다.
  OpenAI 기능은 공식 Docs MCP에서 확인하고, 연결 불가 시 공식 문서로 확인한다.
  기존 도구가 충족하는 기능을 위해 MCP를 중복 설치하지 않는다.
- 작업 기록은 PR 묶음당 `workthrough/YYYY-MM-DD-scope.md` 하나를 생성·갱신한다.
  변경 목적·중요한 결정·영향 경로·실제 검증 결과·남은 제한을 간결하게 적는다.
  전체 diff·성공 로그·회의 내용을 복제하지 않는다. CI run/commit/fixture 근거를 연결하고,
  실패·미실행·수동 실기 필요 상태를 PASS와 구분한다.
- 컨텍스트 전환 시 목표, 승인된 범위, WP/요구사항 ID, branch/worktree, 결정, 검증과 다음 작업을
  짧게 남긴다. 재개 시 실제 git/CI 상태와 대조한다. 자동 노트·Memories는 참고 계층이며
  필수 규칙·데이터 경계·완료 증거의 유일한 원장으로 사용하지 않는다.
