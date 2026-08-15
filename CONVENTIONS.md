# devbox — Tauri 13개 데스크톱 앱 모노레포 공통 규약

13개 앱(port-manager, developer-toolbox, wsl-desktop, api-playground, everything-plus, knowledge-base,
life-log, devbox-manager, code-pad, run-manager, workbench, webhook-lab, repo-manager)을 하나의 저장소에서 관리하되,
각각은 **독립적으로 실행되고 독립적으로 .exe가 만들어지는 Tauri 앱**이다. 소스 저장소와 공통 코드만 공유한다.

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
| 개발 OS | WSL2 Ubuntu + Windows (편집은 WSL, 빌드는 Windows) |
| 소스 위치 | `/mnt/e/projects/devbox/apps/<AppName>` (Windows: `E:\projects\devbox\apps\<AppName>`) |
| 에디터 | 자유 (Rust-analyzer + ESLint + Prettier 권장) |
| 프론트 패키지 매니저 | **pnpm** (workspace) |
| Rust 빌드 | **Cargo workspace** (루트 `Cargo.toml`) |

### 빌드 원칙
- **개발(핫리로드)**: Windows PowerShell에서 `pnpm tauri dev` (각 앱 디렉터리에서)
- **배포 빌드**: Windows PowerShell에서 `pnpm tauri build`
- WSL은 편집·git·React dev server 용도로만 사용
- Rust 툴체인은 **Windows에 설치** (`winget install Rustlang.Rustup` → MSVC 기본 툴체인)
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
│  └─ repo-manager/        # git 저장소·worktree 관리
│
├─ packages/               # React 공용
│  ├─ tokens/              # 공용 CSS 커스텀 프로퍼티  (기존 앱 10곳 사용)
│  ├─ editor/              # CodeMirror 공용 설정      (knowledge-base, code-pad)
│  ├─ diff-view/           # diff 렌더 공용            (code-pad, run-manager)
│  └─ ...
│
├─ crates/                 # Rust 공용
│  ├─ filesystem/          # 파일 walk/검색 순회  (everything-plus, code-pad)
│  ├─ markdown/            # 마크다운 렌더          (knowledge-base, code-pad)
│  ├─ process/             # 프로세스/포트 조회·kill  (port-manager, run-manager)
│  ├─ wsl/                 # WSL argv·경로 정규화    (wsl-desktop, run-manager, workbench, repo-manager)
│  ├─ search/              # FTS5 쿼리 빌더          (everything-plus, knowledge-base)
│  ├─ integration/         # 앱 간 snapshot 계약      (run-manager, workbench, knowledge-base)
│  └─ secrets/             # DPAPI 비밀 보호          (api-playground, run-manager)
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
- 각 앱의 산출물: `PortManager.exe`, `DevToolbox.exe`, `WSLDesktop.exe`, `ApiPlayground.exe`, `EverythingPlus.exe`, `Knowledge.exe`, `LifeLog.exe`, `DevboxManager.exe`, `CodePad.exe`, `RunManager.exe`

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
- 편집기: `@codemirror/*` 직접 사용 (code-pad). 공용 설정은 `packages/editor` (추출 예정 — PR 24)
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

## 5. 개발 워크플로 (WSL-first)

| 단계 | 명령 | 위치 |
|---|---|---|
| 앱 로컬 로직 개발 | `cargo test` | `apps/<app>/src-tauri` |
| 전체 Rust 컴파일 검증 | `cargo check` | 워크스페이스 루트 or 앱 |
| 프론트 UI 미리보기 | `pnpm dev` (mock 데이터) | `apps/<app>` |
| 프론트 타입/빌드 검증 | `pnpm build` | `apps/<app>` |
| 실제 앱 실행 | `pnpm tauri dev` | Windows PowerShell, `apps/<app>` |
| 배포 빌드 | `pnpm tauri build` | Windows PowerShell, `apps/<app>` |

- WSL 컴파일엔 Linux 시스템 라이브러리 필요:
  `libwebkit2gtk-4.1-dev libgtk-3-dev build-essential libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev patchelf`
- 프론트는 `src/lib/isTauri.ts` 분기로 Tauri 없이 mock 데이터 표시
- 9p 마운트 성능: cargo `target-dir`은 `.cargo/config.toml`로 Linux 네이티브 경로 지정

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
2. `cargo check`(src-tauri) / `pnpm build`(프론트) 통과 확인
3. 두 번째 앱에서 중복 코드 발생 시 → `crates/`·`packages/`로 추출
4. 최종: Windows에서 `pnpm tauri dev/build`

## 7. 개발 순서 (13개 전체)
```
Phase 1: port-manager → developer-toolbox     # Tauri 기본기 (IPC, Rust 기초, 설정)
Phase 2: api-playground → everything-plus      # 자식 프로세스, async, HTTP, 상태관리
Phase 3: knowledge-base → life-log             # 개인 데이터 플랫폼 통합
추가:    wsl-desktop, devbox-manager           # PTY 터미널, 앱 설치·업데이트
추가:    code-pad, run-manager                 # 경량 코드 에디터(LSP), 예약 실행·서비스
Stage4:  workbench                             # 프로젝트 기반 orchestration 셸
Stage5:  webhook-lab, repo-manager             # 로컬 웹훅 서버, git worktree 관리
```
- 13개 앱 모두 구현 완료. 진행 상황은 [docs/roadmap.md](./docs/roadmap.md) 참조
- 공통 코드 발견 시점에 `crates/process`, `crates/wsl`, `packages/tokens` 등을 하나씩 추출
- 각 프로젝트 상세는 `apps/<AppName>/PLAN.md` 또는 설계 문서(`docs/superpowers/specs/`) 참조

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
- 완료 정의(`cargo test`/`cargo check`/`pnpm build` 통과)를 커밋 전에 확인

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

## 10. 통합 전략 (Workbench)
- `apps/workbench`를 프로젝트 기반 orchestration 셸로 구현했다 (구현 완료)
- workbench는 기존 `crates/`·`packages/`를 그대로 재사용 → 공통화가 통합을 쉽게 만든다
- 결과적으로 "독립 앱 13개" 구조 (workbench 포함, 독립 .exe)
- 상세: `docs/product-opportunities.md` §15.2, `docs/superpowers/specs/2026-08-14-workbench-design.md`
