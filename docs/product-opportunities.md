# devbox 제품 기회 및 실행 계획

> - 상태: **완료(Completed)** — §17 실행 계획(PR 1~39 + Stage 4/5)은 v0.4.0에서 전부 실행됨.
> - 이 문서는 이제 **결정·분석 근거의 보존용**이다. 신규 작업은
>   `docs/roadmap.md`와
>   [`2026-08-22-v0.5.0-native-first-plan.md`](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)를
>   따른다.
> - 최초 검토일: 2026-08-13 · 실행 완료: 2026-08-15 · v0.5.0 방향 개정: 2026-08-22
> - 검토 기준: `main` (`43f941b`), 12개 앱 코드 직접 대조
> - 범위: 앱 통폐합, 기술 스택 정책, 배포 기반, 공용 프리미티브, 앱 간 통합, 신규 앱 후보, PR 단위 실행 계획

> **2026-08-22 정정.** 이 문서의 최초 분석은 외부 도구와 영역이 겹친다는 사실을 기능 제외
> 근거로 지나치게 강하게 사용했다. devbox의 목적은 개발자가 자주 쓰는 작업을 여러 도구에
> 나누어 수행하고 파일·클립보드로 옮기는 비용을 줄이는 것이다. 외부 도구의 존재만으로
> 기능을 제외하지 않으며, P1·P2 core는 오프라인 native 제공, 대형 전문 도구는 선택적
> 보완재로 두는 정책으로 변경했다. §15·§16은 이 결정에 맞춰 개정했고, 상세 범위와 상한은
> 위 v0.5.0 계획이 우선한다.

## 1. 문서 목적

이 문서는 이미 구현된 기능의 이력을 기록하지 않는다. 현재 `main`에서 앞으로 할 가치가
있는 작업만 남긴 제품 방향 문서이자, 그 작업을 순서대로 수행하기 위한 실행 계획이다.

각 제안은 다음 원칙을 따른다.

- 기능 하나를 하나의 PR로 나눌 수 있어야 한다.
- 기존 앱의 책임을 다른 앱에서 복제하지 않는다.
- Windows와 WSL을 함께 사용하는 개발 흐름을 devbox의 차별점으로 삼는다.
- 파일 삭제, 프로세스 종료, 업데이트, secret 취급은 편의성보다 안전 경계를 먼저 정한다.
- 공용 코드는 두 번째 실제 소비자가 생길 때 추출한다.
- 이 문서의 후보를 구현할 때는 별도 설계 문서에서 상세 동작과 실패 정책을 확정한다.

### 1.1 이 개정에서 확정한 결정

| # | 결정 | 내용 |
|---|---|---|
| 1 | **기술 스택 유지** | Tauri v2 + Rust + React 단일 스택. UI 프레임워크를 추가하지 않는다. 라이브러리 접근이 필요하면 sidecar (§2.3) |
| 2 | **WSL 앱 병합** | wsl-dashboard → wsl-desktop 흡수. `gitStatus`는 Workbench 이관 대상으로 표시 (§3.1) |
| 3 | **활동 앱 병합** | activity-timeline → life-log 흡수. §9.1 integration 파일럿을 run-manager로 교체 (§3.2) |
| 4 | **Knowledge/CodePad 분리 유지** | 대신 knowledge-base에 CodeMirror를 도입하고, 그 시점에 `packages/editor`를 추출 (§3.3) |
| 5 | **Port/Run 분리 유지** | `crates/process`가 이미 공유 중. 병합 대신 상호 링크 (§3.4) |
| 6 | **`crates/search` 추출** | `build_fts_query`가 두 앱에 중복 존재. 실소비자 2개 (§7.2) |
| 7 | **identifier 네임스페이스 변경** | `com.workbench.*` → `com.devbox.*` (§4) |
| 8 | **레포명 유지** | `devbox` 그대로 |

### 1.2 최초본 대비 변경 사항

2026-08-13 최초본 이후 코드를 대조하며 다음을 수정했다.

| 구분 | 내용 |
|---|---|
| 정정 | §4.1의 "release workflow가 10개 앱만 빌드한다"는 `43f941b`에서 이미 해결됐다 |
| 추가 | 버전 원본이 셋이고 `package.json`은 12개 전부 `0.1.0`에서 멈춰 있다 (§5.2) |
| 추가 | 앱 통폐합 §3. 12개 → 10개 |
| 추가 | 기술 스택 정책 §2 |
| 추가 | identifier 네임스페이스 정리 §4 |
| 추가 | §7 **P0.5 공용 프리미티브**. `crates/wsl`, `crates/search`, UI 토큰, CSP |
| 추가 | `CONVENTIONS.md` §3이 선언한 프론트 스택이 실제 코드와 전혀 일치하지 않는다 (§7.4) |
| 추가 | 12개 앱 전부 `tauri.conf.json`의 `"csp": null`이다 (§7.5) |
| 변경 | §10.1 integration snapshot 위치를 producer identifier 아래에서 **devbox 공용 루트**로 |
| 변경 | integration 파일럿을 activity→life-log에서 **run-manager→life-log**로 (병합 때문) |
| 추가 | §12.5 흩어진 네 기능이 같은 "변경 집합 preview" 부품을 요구한다 |
| 추가 | §17 **실행 계획**. PR별 변경 파일·작업 순서·체크리스트·검증 명령·완료 조건 |

## 2. 기술 스택 정책

### 2.1 결론

**Tauri v2 + Rust + React 단일 스택을 유지한다.** Python/PySide6, C++/Qt를 UI 스택으로
도입하지 않는다.

### 2.2 근거

#### 코드 질량이 전제를 뒤집는다

| 계층 | 규모 |
|---|---|
| Rust (`apps/*/src-tauri`) | **39,456줄** |
| TypeScript (`apps/*/src`) | 약 **3,500줄** — App.tsx가 앱당 163~330줄 |

앱들은 **얇은 React 껍데기 + 두꺼운 Rust 백엔드**다. 프론트는 이미 최소한이다.

그 39,456줄이 하는 일은 PTY 세션 관리, Windows Job Object, foreground window 추적, DPAPI,
LSP stdio 전송과 서버 프로세스 관리, cron 스케줄러, WSL 프로세스 종료 보장, FTS5 인덱싱,
파일시스템 watcher다.

**전부 Python이 가장 약하고 Rust가 가장 강한 영역이다.** Python이 강한 영역(pandas/numpy,
문서 파싱, 과학 계산, 스크래핑)은 12개 앱과 §8~§15 로드맵 어디에도 없다. 즉 규모의
무게중심이 OS 통합과 프로세스 생명주기에 있는데, 거기에 Python을 넣는 것은 확장이 아니다.

#### Rust로 막힌 기능이 하나도 없다

| 필요 | 현재 수단 | 상태 |
|---|---|---|
| FTS5 전문 검색 | `rusqlite` + fts5 | 사용 중 |
| Windows API (foreground window, idle) | `windows` crate | 사용 중 |
| LSP 클라이언트 | 직접 구현 (code-pad 약 8,000줄) | 사용 중 |
| cron·프로세스·Job Object | 직접 구현 | 사용 중 |
| HTTP·watcher·마크다운·PTY | reqwest, notify, pulldown-cmark, portable-pty | 사용 중 |
| LLM 일기 생성 (§11.1) | HTTP 호출 | Rust로 충분 |

#### 배포 경로가 정면으로 막는다

§5.4 manifest는 앱당 `portable`(exe 하나)과 `installer`(NSIS) 두 종류만 안다.

- Tauri 산출물: 약 5~10MB. WebView2는 Windows 11 기본 탑재라 런타임 의존이 없다.
- PySide6 산출물: PyInstaller/Nuitka 번들 40~80MB, 시작 지연, 백신 오탐이 흔하다.
- 세 번째 패키징 모델이 생기면 manifest 스키마·digest 검증·원자 설치·rollback이 전부
  분기한다. 그 작업은 아직 시작도 안 됐다 (§17.3).

Qt/PySide6의 LGPL(동적 링크와 재링크 가능성 보장)도 Tauri(MIT/Apache)에는 없는 제약이다.

#### 이 저장소에 이미 실패 사례가 있다

`CONVENTIONS.md` §3이 선언한 프론트 스택 7개 중 실사용이 **0개**다 (§7.4).

차이는 비용이다. 안 쓴 React 라이브러리는 비용이 0이지만, **안 쓴 두 번째 UI 스택은
비용이 발생한다** — CI 매트릭스, 릴리스 경로, Manager 패키징 모델, 문서, 그리고 반복되는
의사결정.

### 2.3 탈출구 — sidecar

라이브러리 접근이 목적이면 UI 스택을 늘리지 않고 **sidecar 프로세스**로 들인다.
Tauri v2는 `externalBin`을 지원한다.

```text
Tauri 앱 (Rust + React)
   └─ 자식 프로세스로 외부 도구 실행
      계약: 버전 있는 JSON stdin/stdout, 타임아웃, 종료 보장
```

UI 스택·디자인 시스템·패키징 모델은 하나로 유지되고, Devbox Manager는 여전히 exe 하나만
알면 된다.

### 2.4 CONVENTIONS에 넣을 규칙

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

현재 조건 2를 만족하는 항목이 하나도 없다. 생기면 그때 근거가 명확해진다.

## 3. 앱 통폐합 — 12개에서 10개로

판단 기준은 "주제가 비슷한가"가 아니라 다음 넷이다.

1. 데이터 모델을 공유하는가, 아니면 주제만 같은가
2. 사용자가 하나의 작업을 하려고 둘을 동시에 여는가
3. 세션 성격이 같은가 (상시 vs 필요할 때)
4. 합치면 통합인가, 탭 두 개짜리 런처인가

### 3.1 WSL Dashboard → WSL Desktop 흡수 (병합)

#### 근거

wsl-dashboard에 다음 함수가 있다.

```rust
// apps/wsl-dashboard/src-tauri/src/commands/wsl.rs:75
pub fn open_terminal(distro: String) -> Result<(), String> {
    std::process::Command::new("wt.exe")   // Windows Terminal에 위임
```

**자기가 필요로 하는 터미널을 외부 앱에 위임하고 있다.** 그 터미널이 바로 옆 앱
(wsl-desktop)이다. distro 열거와 출력 디코딩 코드도 두 앱에 중복돼 있다 (§7.2).

"distro 상태 확인 → 거기서 터미널 열기"는 하나의 작업이다.

#### 병합 방향

**wsl-desktop이 흡수한다.** PTY 세션 상태, pane 레이아웃, 탭 지속화가 옮기기 어려운
자산이기 때문이다.

| | Rust | 프론트 |
|---|---:|---:|
| wsl-desktop | 346줄 | 약 700줄 (components·libs·테스트 포함) |
| wsl-dashboard | 405줄 | 약 300줄 |

#### 기능 3분할

wsl-dashboard는 단순 흡수가 아니라 세 갈래로 나뉜다.

| 기능 | 갈 곳 | 근거 |
|---|---|---|
| `listDistros` + `openTerminal` | wsl-desktop에 병합 | 위 근거 |
| `dockerPs` / `dockerAction` | wsl-desktop에 병합, distro 스코프 유지 | Docker는 WSL 안에서 돈다. §16의 "Docker Desktop 복제 금지" 범위 안 |
| `gitStatus(projects)` + 프로젝트 목록 | **Workbench 이관 대상** (§15.2) | §15.2 Workbench MVP와 동일 기능 |

`gitStatus`는 **Workbench가 생길 때까지 병합앱에 그대로 둔다.** 대체물 없이 동작하는 기능을
먼저 제거하지 않는다. 코드에 이관 예정 주석을 남긴다.

#### 앱 정보

| 항목 | 값 |
|---|---|
| id / 디렉터리 | `wsl-desktop` (유지) |
| productName | `WSLDesktop` (유지) |
| identifier | `com.devbox.wsldesktop` |
| 삭제 | `apps/wsl-dashboard/` 전체 |

디렉터리명을 바꾸지 않는 이유: Cargo members, pnpm workspace, CI scope, 카탈로그, release
asset 이름이 모두 따라 바뀐다. 상태 패널이 추가된다고 이름을 바꿀 필요는 없다.

### 3.2 Activity Timeline → Life Log 흡수 (병합)

#### 근거

`apps/life-log/src/types.ts`가 Life Log의 도메인 모델 전부다.

```ts
DaySummary  { date, pc_usage_ms, app_totals[], git }
RangeSummary { label, pc_usage_ms, app_totals[], git, daily[] }
```

`pc_usage_ms`와 `app_totals`는 **activity-timeline의 데이터**다. 남는 고유 자산은
`git.total_commits` 하나다. Life Log는 사실상 "activity-timeline 데이터를 일/주/월로 접고
git 커밋 수를 붙인 뷰"다.

그리고 `apps/life-log/src/api.ts`에 이것이 있다.

```ts
getActivityDb() → "%LOCALAPPDATA%\\com.workbench.activitytimeline\\data.db"
```

**사용자가 두 앱을 다 설치해야 하나가 동작한다.** life-log 단독으로는 아무 값도 없다.

#### 병합 방향

**life-log가 흡수한다.** 제품 가치가 "내 하루 기록"이고 활동 추적은 그 엔진이기 때문이다.
코드 규모가 대등해서(Rust 537줄 vs 433줄) 이동 방향이 비용을 결정하지 않는다.

| | Rust |
|---|---:|
| activity-timeline | 537줄 (db 159, sessionizer 117, lib 75, tracking 69, window 52, models 29, queries 24) |
| life-log | 433줄 (readers/activity 131, commands/life 126, aggregate 63, models 49, lib 39) |

옮겨야 할 것: SQLite 세션 스키마, sessionizer, foreground window 추적, tray, poller.
tray/poller는 `commands/tracking.rs`(69줄)와 `lib.rs`의 `setup_tray`(약 30줄)에 있다.

#### 세션 성격 차이는 문제가 아니다

life-log는 상시 트레이 프로세스가 된다. 트레이 앱이 창을 갖는 것은 정상 패턴이고
activity-timeline이 이미 그렇다. 병합은 "추적기를 따로 설치해야 하는" 잘못된 선택지를
없앤다.

#### 필수 후속 조치

§10.1은 "첫 소비 흐름은 Activity Timeline → Life Log로 검증한다"고 되어 있었다. 병합하면
그 흐름이 앱 내부가 되어 계약이 검증되지 않는다.

> **integration snapshot 파일럿을 `run-manager → life-log`로 교체한다.**

계약 자체는 여전히 필요하다. §11.1이 run-manager·knowledge·everything+·code-pad를 source로
원하기 때문이다.

#### 앱 정보

| 항목 | 값 |
|---|---|
| id / 디렉터리 | `life-log` |
| productName | `LifeLog` (유지) |
| identifier | `com.devbox.lifelog` |
| 삭제 | `apps/activity-timeline/` 전체 |
| 데이터 이관 | 구 activity-timeline의 세션 DB를 life-log DB로 흡수 |

### 3.3 Knowledge Base + Code Pad — 분리 유지

#### 공유하는 것이 이미 다 추출돼 있다

| | Knowledge Base | Code Pad |
|---|---|---|
| 에디터 | **`<textarea>`** (`src/App.tsx:283`) | CodeMirror 6 + LSP 8개 언어 |
| 프론트 의존성 | `mermaid`만 | `@codemirror/*` 14개 + `mermaid` |
| 마크다운 렌더 | `crates/markdown` | `crates/markdown` |

knowledge-base는 CodeMirror를 쓰지 않는다. 두 앱이 실제로 공유하는 것은 마크다운 렌더링뿐이고
그것은 **이미 `crates/markdown`으로 추출돼 있다.**

#### 안전 경계가 반대다

- knowledge-base: 첨부·프리뷰 경로가 knowledge root **밖으로 나가면 안 된다** (§11.2 완료조건)
- code-pad: **임의의 경로를 열어야 한다**

한 프로세스에 넣으면 두 경계가 충돌한다.

#### 대신 할 것 — knowledge-base에 CodeMirror 도입

knowledge-base의 마크다운 편집기가 순수 `<textarea>`다. 문법 하이라이팅도 마크다운 편의
기능도 없다. code-pad의 CodeMirror 설정을 도입하면 두 가지를 동시에 얻는다.

1. 실제 UX 개선
2. `packages/editor` 추출을 정당화하는 **두 번째 실소비자**

§17.5 PR 23·24가 이 작업이다.

### 3.4 Port Manager + Run Manager — 분리 유지

#### 반론과 그 반론이 지는 이유

port-manager는 App.tsx 188줄로 아주 작다. 서비스는 포트를 점유하고 §15.2 Workbench도
expected port 확인을 원한다. 합치면 한 화면에서 볼 수 있다.

그럼에도 분리를 유지한다. **소유권 모델이 반대**이기 때문이다.

| | port-manager | run-manager |
|---|---|---|
| 대상 | 내가 만들지 않은 **남의 프로세스** | 내가 정의한 **내 프로세스** |
| 성격 | 진단 도구 | 감독자(supervisor) |
| 권한 | 임의 프로세스 종료 | DPAPI secret, 시작프로그램 등록, 스케줄러 |

합치면 "시스템의 아무 프로세스나 죽이는 권한"이 secret과 자동 시작을 가진 감독자 앱 안으로
들어간다. 폭발 반경만 넓어진다.

공유 프리미티브 `crates/process`는 **이미 올바르게 추출돼 있다.** 필요한 것은 병합이 아니라
상호 링크다 (§14.1).

### 3.5 병합 후 앱 구성

```text
환경·실행   WSL Desktop(병합)    Port Manager    Run Manager
탐색·편집   Everything+          Code Pad        Knowledge Base
기록        Life Log(병합)
도구        Developer Toolbox    API Playground
메타        Devbox Manager
                                   ↑ 위에 Workbench (wsl-desktop의 gitStatus 흡수)
```

### 3.6 병합이 나머지 계획에 미치는 영향

| 항목 | 변화 |
|---|---|
| 릴리스 매트릭스 | 12 → 10개 |
| Devbox Manager 목록 | 자기 자신 제외 9개 |
| `crates/wsl` | 소비자 3 → 2. 추출 근거가 "중복 제거"에서 "canonical identity"로 이동 (§7.2) |
| UI 토큰 | 12벌 → 10벌 |
| §10.1 파일럿 | activity→life-log에서 **run-manager→life-log**로 교체 |
| CSP 기준선 | 12개 → 10개 앱 |

**병합은 배포 정상화(§17.3)보다 먼저 한다.** 카탈로그·manifest·Manager를 12개 기준으로
만든 뒤 10개로 줄이면 같은 작업을 두 번 한다.

## 4. identifier 네임스페이스

### 4.1 현재 문제

12개 앱의 bundle identifier가 전부 `com.workbench.*`다.

```
com.workbench.activitytimeline   com.workbench.apiplayground
com.workbench.codepad            com.workbench.devboxmanager
com.workbench.developertoolbox   com.workbench.everythingplus
com.workbench.knowledgebase      com.workbench.lifelog
com.workbench.portmanager        com.workbench.runmanager
com.workbench.wsldashboard       com.workbench.wsldesktop
```

동시에 §15.2는 `Workbench`라는 이름의 통합 앱을 만들려 한다. 그러면 identifier가
`com.workbench.workbench`가 된다. 이름이 셋(레포 `devbox`, 네임스페이스 `com.workbench`,
앱 `Workbench`) 돌아다닌다.

### 4.2 결정

**`com.workbench.*` → `com.devbox.*`**

- 레포명 `devbox`를 유지하기로 했으므로 네임스페이스가 일치한다
- `Workbench`가 앱 이름으로 자유로워진다 (`com.devbox.workbench`)
- §10.1의 integration 공용 루트 `%LOCALAPPDATA%\devbox\`와 개념이 일치한다

### 4.3 최종 앱 목록 (10개)

| id | displayName | productName | identifier |
|---|---|---|---|
| `port-manager` | Port Manager | `PortManager` | `com.devbox.portmanager` |
| `developer-toolbox` | Developer Toolbox | `DevToolbox` | `com.devbox.developertoolbox` |
| `wsl-desktop` | WSL Desktop | `WSLDesktop` | `com.devbox.wsldesktop` |
| `api-playground` | API Playground | `ApiPlayground` | `com.devbox.apiplayground` |
| `everything-plus` | Everything+ | `EverythingPlus` | `com.devbox.everythingplus` |
| `knowledge-base` | Knowledge | `Knowledge` | `com.devbox.knowledgebase` |
| `life-log` | Life Log | `LifeLog` | `com.devbox.lifelog` |
| `devbox-manager` | Devbox Manager | `DevboxManager` | `com.devbox.devboxmanager` |
| `code-pad` | Code Pad | `CodePad` | `com.devbox.codepad` |
| `run-manager` | Run Manager | `RunManager` | `com.devbox.runmanager` |
| *(예정)* `workbench` | Workbench | `Workbench` | `com.devbox.workbench` |

### 4.4 시점이 중요하다

Tauri의 `app_local_data_dir()`은 `%LOCALAPPDATA%\{identifier}\`를 반환한다. identifier를
바꾸면 **10개 앱의 SQLite와 설정이 전부 이사**해야 한다.

그리고 §10.1이 도입할 다음 경로는 여러 앱이 읽는 계약 경로가 된다.

```text
%LOCALAPPDATA%\devbox\integration\<app-id>\v1\summary.json
```

> **identifier 변경은 카탈로그(§17.3 PR 6)보다 먼저, 반드시 integration snapshot
> (§17.6 PR 26)보다 먼저 끝낸다.** 카탈로그가 identifier를 단일 원본으로 담게 되므로
> 그 전에 정하면 한 번에 끝난다. 사용자 데이터가 가장 적은 지금이 가장 싸다.

## 5. P0 — 배포와 설치 체계

### 5.1 현재 문제

`43f941b`에서 release workflow의 앱 배열은 12개로 교정됐다. 남은 문제는 다음과 같다.

- `.github/workflows/release.yml`의 앱 목록이 여전히 **workflow 안에 하드코딩된 PowerShell
  배열**이다. 앱을 추가·삭제할 때 Cargo workspace, pnpm workspace, release.yml,
  Devbox Manager를 각각 고쳐야 한다.
- artifact 수집이 `target/release/*.exe`라는 넓은 glob이다. Cargo workspace라 모든 앱이 같은
  `target/`을 공유하므로 이전 빌드의 stale artifact가 섞일 수 있다.
- publish 단계가 `installers/**/*.exe`를 그대로 올린다. 무엇이 올라가야 하는지 선언이 없다.
- release tag 하나가 모든 앱의 버전으로 취급된다. 실제 앱 버전은 서로 다르다.
- artifact의 크기·digest를 어디에도 기록하지 않는다.
- `workflow_dispatch`의 기본 version이 고정된 과거 값(`v0.1.0`)이다.
- 같은 tag로 다시 실행해도 막지 않는다. `make_latest: true`가 무조건 적용된다.

### 5.2 버전 원본이 셋이다

앱 하나의 버전이 세 파일에 따로 존재한다.

| 파일 | 현재 값 | 용도 |
|---|---|---|
| `apps/<app>/src-tauri/Cargo.toml` | `0.2.0`~`0.3.0` (앱마다 다름) | Cargo package version |
| `apps/<app>/src-tauri/tauri.conf.json` | Cargo와 동일 | 번들·installer 파일명에 사용 |
| `apps/<app>/package.json` | **12개 전부 `0.1.0`** | pnpm workspace package version |

`package.json`의 version은 한 번도 올라간 적이 없다. 실제 버전 분포는 다음과 같다.

```
0.3.0  code-pad, run-manager
0.2.2  life-log, wsl-desktop
0.2.0  activity-timeline, api-playground, devbox-manager, developer-toolbox,
       everything-plus, knowledge-base, port-manager, wsl-dashboard
```

정합성 검사를 만들 때 **반드시 세 원본을 모두 봐야 한다.** 두 원본만 검사하면 이 drift가
그대로 통과한다. 그리고 검사를 도입하기 **전에** `package.json`을 먼저 맞춰야 한다.
그렇지 않으면 검사 PR이 도입 즉시 실패한다 (§17.3 PR 5가 PR 7보다 앞서는 이유).

### 5.3 앱 카탈로그 단일 원본

`apps/catalog.json`을 추가한다.

```json
{
  "schemaVersion": 1,
  "apps": [
    {
      "id": "code-pad",
      "displayName": "Code Pad",
      "productName": "CodePad",
      "identifier": "com.devbox.codepad",
      "cargoPackage": "code-pad",
      "appDir": "apps/code-pad",
      "release": true,
      "managerVisible": true,
      "selfManaged": false
    }
  ]
}
```

카탈로그가 소유할 값:

- 안정적인 앱 ID (kebab-case, 디렉터리 이름과 동일)
- 사용자 표시 이름
- Tauri product name과 bundle identifier
- Cargo package 이름과 앱 디렉터리 경로
- release 포함 여부
- Devbox Manager 표시 여부
- self-update 대상 여부

카탈로그가 소유하지 않을 값:

- 앱 기능 설명 전체
- 앱 내부 설정
- 변경 이력
- 빌드 시 계산할 digest
- **앱 버전** (버전은 §5.2의 세 파일이 원본이고, 카탈로그는 참조만 한다)

카탈로그는 배포 대상 목록일 뿐 아니라 **런타임 discovery의 단일 원본**이기도 하다.
§10.1의 integration snapshot 경로가 카탈로그의 앱 ID를 그대로 쓴다.

#### CI scope 함정

`.github/scripts/ci-scope.sh`는 변경 경로를 앱으로 매핑한다. `apps/catalog.json`은 다음
분기를 탄다.

```bash
apps/*)
  app=${path#apps/}      # → "catalog.json"
  app=${app%%/*}         # → "catalog.json"
  if [[ -z $app || ! -f apps/$app/package.json ]]; then
    frontend_all=true    # ← 여기로 떨어진다
  ...
```

즉 카탈로그만 고치면 **frontend 전체 게이트만 돌고 Rust 게이트는 돌지 않는다.** 카탈로그를
추가하는 PR에서 명시적 분기를 함께 넣어야 한다 (§17.3 PR 6).

### 5.4 앱별 release manifest

release tag와 앱 버전을 분리한다. release build가 다음 manifest를 생성해 asset으로 함께
게시한다. 파일 이름은 `release-manifest.json`으로 고정한다. Manager가 이름을 추측하지
않아야 하기 때문이다.

```json
{
  "schemaVersion": 1,
  "releaseTag": "v0.4.0",
  "generatedAt": "2026-08-14T12:00:00Z",
  "apps": [
    {
      "id": "life-log",
      "version": "0.2.2",
      "portable": { "name": "life-log.exe", "sha256": "...", "size": 123456 },
      "installer": { "name": "LifeLog_0.2.2_x64-setup.exe", "sha256": "...", "size": 234567 }
    }
  ],
  "notices": {
    "name": "THIRD_PARTY_NOTICES.md",
    "sha256": "...",
    "size": 345678
  }
}
```

`notices`는 schemaVersion 1의 backward-compatible 추가 필드다. 기존 Manager는 앱 asset을
그대로 읽고, release verifier는 notices까지 선언된 asset으로 검증한다. Manager는 GitHub
asset 이름을 추측하지 않고 이 manifest만 신뢰한다.

### 5.5 release workflow 개선

권장 PR 분할은 §17.3에 있다. 추가 규칙:

- `target/release/*.exe`처럼 넓은 glob을 최종 publish 기준으로 사용하지 않는다.
- 이전 빌드의 stale artifact가 포함되지 않도록 앱별 staging directory를 사용한다.
- 한 앱의 실패가 누락된 상태의 정상 release로 보이지 않도록 publish는 전체 matrix 성공 후
  수행한다.
- manual dispatch 기본 version은 고정된 과거 값 대신 필수 입력으로 둔다.
- release tag가 이미 존재하면 덮어쓰지 않고 명확히 실패한다.

### 5.6 완료 조건

- catalog, Cargo workspace, pnpm workspace의 앱 집합이 CI에서 일치한다.
- 앱별 `Cargo.toml`·`tauri.conf.json`·`package.json`의 version이 CI에서 일치한다.
- 10개 앱 모두 release matrix에 포함된다.
- 각 앱은 release tag와 독립적인 실제 앱 버전을 manifest에 가진다.
- publish된 모든 artifact가 manifest의 size와 SHA-256에 일치한다.
- Devbox Manager가 하드코딩 배열이나 파일명 추측 없이 앱과 업데이트를 표시한다.

## 6. P0 — Devbox Manager 안전성

### 6.1 확인된 코드 위치

| 문제 | 위치 |
|---|---|
| 앱 목록 하드코딩 (9개, code-pad·run-manager 누락) | `apps/devbox-manager/src/App.tsx:6` |
| release tag = 앱 버전 가정, asset 파일명 추측 | `apps/devbox-manager/src/App.tsx:40` `findAsset()` |
| 프론트가 URL 문자열을 결정해 전달 | `apps/devbox-manager/src/api.ts` `installApp()` |
| URL을 검증 없이 다운로드 | `apps/devbox-manager/src-tauri/src/commands/manager.rs:174` `download()` |
| 전체를 메모리에 받고 digest·size 검증 없이 저장 | 같은 함수 |
| 검증 없이 installer 실행 | `manager.rs:99` `install()` |
| registry를 비원자적으로 기록 | `manager.rs:49` `write_registry()` |
| 자동화 테스트 없음 | 앱 전체 (Rust `mod tests` 0건, 프론트 테스트 0건) |

### 6.2 버전 모델

registry에 저장하는 version과 화면에서 비교하는 release tag의 정규화 규칙을 없애고,
manifest의 앱별 `version`을 사용한다.

- Rust에 하나의 semver 파서와 비교 함수를 둔다.
- `v0.3.0`과 `0.3.0` 같은 tag 표현은 release 계층에서만 정규화한다.
- 앱 버전은 Tauri/Cargo version과 manifest가 일치하는지 build 단계에서 검증한다.
- downgrade는 별도 사용자 동작으로 구분한다.
- prerelease는 stable channel에서 자동 제안하지 않는다.

### 6.3 다운로드 신뢰 경계

1. 허용한 GitHub repository의 HTTPS asset URL만 받는다.
2. redirect 후 최종 host와 경로를 다시 검증한다.
3. 응답 크기에 상한을 두고 manifest size와 대조한다.
4. `.partial` 파일에 streaming download한다.
5. SHA-256을 계산해 manifest와 대조한다.
6. 검증 성공 후 version directory로 atomic rename한다.
7. installer는 검증 전 절대 실행하지 않는다.

프론트가 URL과 version을 자유롭게 결정하지 않도록 command 경계를 `install(appId, mode)`
형태로 바꾼다. Rust가 이미 검증한 manifest에서 대상 asset을 선택한다.

### 6.4 원자 업데이트와 rollback

portable 모드 권장 layout:

```text
apps/<app-id>/
├─ versions/
│  ├─ 0.2.0/<app>.exe
│  └─ 0.3.0/<app>.exe
├─ current.json
└─ download/<version>.partial
```

- 새 버전을 별도 directory에 완전히 준비한 뒤 `current.json`만 원자 교체한다.
- 직전 정상 버전을 최소 하나 보존한다.
- 새 버전 실행 실패를 사용자가 확인하면 rollback할 수 있게 한다.
- 중단된 `.partial`은 재시도하거나 안전하게 정리한다.
- registry 파일도 임시 파일 + rename으로 기록한다.

installer 모드는 installer process를 시작한 시점과 설치 완료를 구분한다. 가능하다면 exit
code와 설치된 executable/version을 확인한 뒤 registry를 갱신한다.

### 6.5 관리 기능

- 설치 제거 / 깨진 경로 복구 / 설치 상태 다시 검색
- 버전별 disk usage / 이전 portable version 정리
- 전체 앱 설정·데이터 위치 열기 / 앱별 backup·export 진입점
- Devbox Manager self-update 정책

self-update는 실행 중인 자신의 EXE를 직접 덮어쓰지 않는다. 별도 helper 또는 다음 실행 시
교체하는 방식으로 설계한다.

### 6.6 테스트 우선순위

Devbox Manager에는 현재 자동화된 Rust·프론트 테스트가 없다. 다음을 먼저 순수 로직으로
분리한다.

catalog와 manifest 파싱 / 버전 비교 / asset 선택 / URL allowlist / size·digest mismatch /
registry migration / interrupted download recovery / rollback target 선택

### 6.7 완료 조건

- 변조되거나 잘린 파일은 저장 완료나 실행 상태가 되지 않는다.
- 업데이트가 중단되어도 기존 버전을 실행할 수 있다.
- installer 실행과 설치 완료를 구분한다.
- catalog 대상 앱이 자동으로 나타난다.
- 앱별 서로 다른 최신 버전을 정확히 표시한다.

### 6.8 v0.5.0 batch 구현 경계 (#274)

다중 선택은 catalog에서 Manager가 관리할 수 있고 manifest에 현재보다 새 버전이 있는 앱만
활성화한다. public IPC는 최대 32개의 `{appId, mode}`만 받고 빈 목록, duplicate, unsafe/unknown ID와
unknown mode를 다운로드 전에 거부한다. release manifest와 HTTP client는 batch당 한 번만 만들며
다운로드와 registry 변경은 입력 순서대로 실행해 네트워크·disk 부하와 registry 경합을 제한한다.

batch의 transaction 단위는 전체 목록이 아니라 앱 하나다. 한 앱의 다운로드·검증·상태 반영이
실패해도 다음 앱을 계속하고 이미 성공한 앱을 되돌리지 않는다. portable은 새 version artifact와
current를 준비한 뒤 registry 기록이 실패하면 이전 current를 원자 복구하며, 최초 설치였다면 생성한
current를 제거한다. setup은 registry를 먼저 durable하게 준비한 뒤 검증된 installer를 실행하고 spawn
실패 시 원래 registry를 복구한다. setup 성공은 installer process 시작이지 설치 완료 증명이 아니므로
UI는 선택 수만큼 마법사가 열린다는 확인을 받고 결과에도 이 의미를 표시한다.

stale frontend state가 batch downgrade를 만들지 않도록 backend가 installed와 available을 strict
SemVer로 비교한다. available이 더 큰 경우만 실제 설치하고 동일·더 최신 installed version은 download
없는 성공 no-op로 반환한다. 파싱할 수 없는 version은 임의 문자열 순서로 비교하지 않고 해당 앱을
안전하게 실패시킨다.

frontend 결과는 catalog app ID, mode, 성공 여부와 고정 메시지만 보관한다. lower-level reqwest,
process와 filesystem 오류의 URL·absolute path를 반사하지 않는다. 성공 항목은 선택에서 제거하고 실패
항목만 원래 mode로 재시도한다. install path 표시(#275), custom root(#308), 안전한 제거(#309)와
Data Inspector는 각 기능 경계에서 유지한다.

### 6.9 v0.5.0 install path 표시 경계 (#275)

경로 표시는 설치·실행·제거 command의 응답을 확장하지 않고 별도 read-only IPC로 제공한다. frontend는
catalog app ID 하나만 보내며 locator path, manifest path나 executable 후보를 입력하지 않는다.
backend는 build/runtime catalog 선택과 locator `catalogRevision`이 일치해야 진행하고, canonical root,
그 root 내부의 canonical source manifest, manifest 전체 app/version/mode/중복 계약과 portable exact
layout을 모두 재검증한다. source manifest는 현재 Manager 목록의 canonical manifest와도 같아야 하며,
유효한 locator 뒤의 손상·다른 root manifest/path는 legacy fallback으로 우회하지 않는다.

portable은 실제 canonical executable, 이를 소유한 install root와 source manifest를 표시한다. installer는
Manager가 검증된 설치 프로그램을 실행했다는 manifest record만 소유할 뿐 wizard의 완료·최종 위치를
증명하지 못한다. 따라서 installer의 executable/root는 `null`이며 UI는 추측 대신 “Manager가 실제 설치
위치를 추적하지 않습니다”라고 표시한다. source manifest는 상태의 provenance로 계속 표시한다.

표시 패널은 명시적인 `읽기 전용` 배지와 executable/root/source manifest label을 갖고 긴 경로를 panel
내에서 wrap한다. 이 PR에는 copy, Explorer open, path 선택·변경, custom root migration, install/remove,
registry write가 없다. 일반 installed/current DTO도 path-free를 유지한다. fixture는 조회 전후 locator,
manifest, executable byte가 같음을 확인하고, revision mismatch·unsafe path는 원문 경로 없는 고정 오류로
fail-closed 처리한다.

### 6.10 v0.5.0 custom install root 경계 (#308, #309 분리)

`#308`은 Devbox Manager가 인터넷이나 별도 설치 프로그램에 의존하지 않고, 개발자가 선택한
로컬 디렉터리를 다음 portable 설치의 root로 안전하게 사용할 수 있게 하는 기능이다. 큰 외부
도구를 다운로드해 대체하는 기능이 아니며, Manager가 이미 소유한 catalog·release manifest·portable
layout·runtime locator를 한 화면에서 연결하는 native 기능이다.

#### #308에서 제공하는 흐름

1. 사용자가 절대 경로를 입력한다. frontend는 4,096 bytes 상한, IME 조합 중 Enter 차단,
   입력 변경 시 이전 preview 폐기, 중복 preview/apply 차단과 접근 가능한 label/status/error를
   제공한다.
2. `preview_install_root`가 입력을 trim하고 native에서 다시 검사한다. unresolved environment
   variable, `~`, `.`/`..`, root/home/workspace/current working directory, 비정규 canonical alias,
   symlink/reparse component, 일반 파일과 누락 디렉터리를 거부한다. 후보는 이미 존재하는 빈
   canonical directory여야 하며 direct entry를 최대 4,096개까지 bounded scan하고 write permission과 OS
   free space 최소 128 MiB를 확인한다.
3. 현재 versioned locator가 없을 때만 v0.4.x default root를 read-only fallback으로 사용한다.
   locator가 존재하면 16 KiB 이하 strict schema만 읽고, active manifest는 1 MiB/256 rows 이내로
   파싱한다. active manifest record 또는 `apps`/partial/기타 artifact가 남아 있으면
   `existing-install`로 보고 자동 migration을 제공하지 않는다. locator 자체가 없더라도 이미
   존재하는 locator parent component가 symlink/reparse이면 legacy fallback을 사용하지 않으며,
   portable record는 canonical exact layout 밖의 executable을 trusted state로 취급하지 않는다.
4. 사용자의 별도 확인 뒤 `apply_install_root`가 preview의 `registryRevision`을 CAS token으로
   검증하고 active root·manifest·candidate·free-space를 즉시 재검사한다. 성공 시 후보에
   `apps/`와 빈 `registry.json`만 안전한 component 단위로 만들고, locator를
   `%LOCALAPPDATA%\devbox\install-roots\v1\registry.json`에 tmp+rename한다. revision은
   overflow 없이 증가하고 catalog revision provenance를 기록한다.
5. locator commit 실패·경합·stale preview는 성공으로 숨기지 않는다. 이번 호출이 만든 빈
   manifest/apps만 rollback하며 기존 root의 registry, binary, partial, user data는 이동·삭제·
   덮어쓰지 않는다. 이후 install/current/rollback/launch/path 조회는 active locator root를
   사용한다. `.partial`은 regular `create_new` slot으로만 만들어 기존 중단 파일을 덮어쓰지 않고,
   present corrupt locator는 startup sync가 default metadata로 바꾸지 않는다.

#### 명시적 비범위와 후속 순서

- `#308`에는 기존 설치를 새 root로 이동하는 migration wizard, 기존 설치 병합, root reset,
  binary removal, user-data 삭제, catalog 수정, installer wizard의 실제 설치 위치 추적이 없다.
- 기본/custom root의 portable 제거와 데이터 보존 정책은 별도 `#309`가 소유하며, #308의
  empty-root pointer 전환과 섞지 않는다.
- `crates/launch` consumer가 읽는 locator/manifest도 같은 path·schema·bytes/rows bound와
  canonical/symlink/reparse fail-closed 규칙을 적용한다. public DTO·UI 오류에는 입력 path,
  locator/manifest 원문, OS 오류, credential을 반사하지 않는다.

#### #309 safe removal 구현

`#309`는 위 #308의 active locator를 소비하는 Manager-owned portable binary 제거 기능이다.
`preview_remove_app({ appId })`가 현재 catalog-visible/non-self-managed 대상, locator provenance,
active manifest digest와 exact `<root>/apps/<app>/versions/<version>/<app>.exe` layout을
read-only로 검증하고, app-owned `current.json`·versions tree의 bounded count/size를 표시한다.
사용자 data는 preview와 결과에 항상 보존으로 표시된다.

별도 확인 뒤 `remove_portable_app`은 preview token의 registry/catalog revision, root ID와
manifest digest를 CAS로 재검증한다. manifest에서 record를 먼저 atomic claim하고, link/reparse,
special/foreign entry, traversal 또는 arbitrary path가 없는 exact regular file/directory 목록만
깊은 순서로 제거한다. `remove_dir_all`과 강제 삭제는 사용하지 않는다. Installer record는
wizard의 실제 위치·uninstaller를 Manager가 소유하지 않으므로 제거하지 않는다.

삭제 중 권한·잠금·I/O 문제가 생기면 남은 항목 수와 `partial` 결과를 반환하고, 이번 호출의
manifest digest가 그대로일 때만 원래 bytes를 복원한다. 이미 삭제된 exact final executable은
복구 parser에서만 missing으로 허용해 interrupted removal을 다시 preview할 수 있으며, 경쟁
writer의 manifest는 덮어쓰지 않는다. frontend는 stale preview를 폐기하고 최신 preview 재확인을
요구하며, 앱 사용자 data 삭제·임의 경로 선택·root migration/reset은 계속 비범위다.

#### PR·검증 경계

기능 단위 PR은 (1) locator/path core 및 bounded parser, (2) Manager command와 active-root
lifecycle, (3) frontend preview/confirm/a11y, (4) 문서·fixture 보강으로 나누어 검토할 수
있지만 merge 단위는 #308 하나로 유지한다. Rust fixture는 missing-vs-corrupt locator,
canonical/protected/symlink/reparse path, active artifact/manifest conflict, candidate conflict,
permission/free-space status, revision CAS/overflow, atomic publish와 rollback residue를 확인한다. frontend
fixture는 preview 전에는 apply가 없고, input/IME 변경이 stale preview를 폐기하며, confirm 거부,
existing-install status와 unmount 중 늦은 응답이 mutation을 만들지 않음을 확인한다. WSL에서는
focused `cargo test -p devbox-manager --lib`, `cargo check -p devbox-manager --lib`, frontend
unit/typecheck를 수행하고, 실제 Tauri 실행·Windows junction/reparse·free-space API와 packaged
installer acceptance는 Windows W2에서 별도로 수행한다. 현재 전용 worktree에서는 manager
67개/launch 23개 focused test, 양쪽 check·clippy·fmt, frontend 17개 test와 build를 통과했고,
full workspace gate와 Windows packaged 검증은 PR/CI/W2로 남겼다.

## 7. P0.5 — 공용 프리미티브

기능 추가가 아니다. 저장소가 이미 선언한 추출 규칙을 집행하고, 문서와 코드의 불일치를
없애는 정리 작업이다. **오늘 실제 소비자가 2개 이상인 것만** 포함한다.

### 7.1 `crates/wsl`

#### 중복 근거 (병합 전)

| 대상 | wsl-dashboard | wsl-desktop | run-manager |
|---|---|---|---|
| `wsl.exe` 출력 UTF-16LE/UTF-8 디코딩 | `commands/wsl.rs:121` `decode_output` | `commands/terminal.rs:265` `decode_output` | — |
| distro 목록 파싱 | `core/parsers.rs:10` `parse_wsl_list` | `commands/terminal.rs:255` `parse_distros` | — |
| `wsl.exe -d <distro>` argv 구성 | `commands/wsl.rs:97` `run_wsl` | `commands/terminal.rs:73` | `core/shell.rs:267` `build_wsl_command` |
| distro 이름 검증 | 없음 | 없음 | `core/shell.rs:308` |
| `wslpath` 변환 argv | 없음 | 없음 | `core/shell.rs:313` |

#### 병합 후 추출 근거가 바뀐다

§3.1 병합으로 wsl-dashboard가 사라지면 `decode_output`과 distro 파싱은 **소비자가 1개**가
된다. 저장소 규칙상 그것만으로는 추출 대상이 아니다.

그럼에도 `crates/wsl`을 추출한다. 근거가 "3벌 중복 제거"에서 **"canonical identity 확보"**로
바뀐다.

> §10.2 `ProjectProfile.wsl = { distro, path }`는 Windows 경로와 WSL 경로의 정규화 규칙이
> **하나**여야 성립한다. 규칙이 둘이면 같은 프로젝트를 서로 다른 identity로 인식한다.
> Workbench(§15.2)는 그 위에 선다.

#### 크레이트 경계

`CONVENTIONS.md` §4는 "crates 안에 Windows 전용 코드를 넣지 않는다"고 정한다. 이 규칙이
경계를 결정한다.

**포함한다** (전부 순수 함수, WSL에서 `cargo test` 가능, 실소비자 2개 = wsl-desktop + run-manager):

- `validate_distro_name(name) -> Result<(), WslError>`
- `build_exec_argv(distro, cwd, command) -> Vec<String>`
- `build_wslpath_argv(distro, windows_path) -> Vec<String>`
- `windows_path_to_wsl` / `wsl_path_to_windows`의 정규화 규칙 (ProjectProfile이 쓴다)

**포함하지 않는다**:

- `decode_wsl_output`, `parse_distro_list` — 병합 후 소비자 1개. wsl-desktop에 남긴다.
  두 번째 소비자가 생기면 그때 옮긴다
- `Command::new("wsl.exe")` 실행 자체 — 각 앱의 command/platform 레이어
- PTY 세션 관리 (wsl-desktop 고유)
- docker/git 출력 파싱 (wsl-desktop 고유)
- run-manager의 marker·handshake·termination 정책 (실행 정책이지 WSL 프리미티브가 아니다)

### 7.2 `crates/search`

#### 중복 근거 — 실소비자 2개

```
apps/everything-plus/src-tauri/src/core/db.rs:237   fn build_fts_query
apps/knowledge-base/src-tauri/src/core/db.rs:122    fn build_fts_query
```

이름도 목적도 같다. FTS5 쿼리 이스케이프와 토큰 단위 prefix 매치다. 두 앱 모두 external
content table + trigger 패턴을 쓴다.

```sql
-- everything-plus
CREATE VIRTUAL TABLE files_fts USING fts5(name, content='files', content_rowid='id');
CREATE VIRTUAL TABLE file_content_fts USING fts5(content, content='file_content', content_rowid='id');
-- knowledge-base
CREATE VIRTUAL TABLE docs_fts USING fts5(title, body, content='docs', content_rowid='id');
```

#### 크레이트 경계

**포함한다**: `build_fts_query(user_input) -> String` — 이스케이프, 토큰 분리, prefix 매치
규칙. 순수 함수이므로 테스트가 쉽다.

**포함하지 않는다**: 스키마 DDL. 두 앱의 테이블 구조가 다르다(`name`/`content` vs
`title`/`body`). 공통화하면 각 앱의 스키마 진화를 막는다.

`crates/database`(마이그레이션 헬퍼)는 **지금 추출하지 않는다.** SQLite 소비자는 5개지만
스키마가 서로 무관해서 공유할 것이 연결 열기 정도뿐이다. §10.1이 `schemaVersion` 규율을
요구할 때 그 목적으로 다시 판단한다.

### 7.3 UI 토큰

10개 앱의 `src/App.css` 합계가 **4,507줄**이다 (병합 전 12개 기준).

```
941 run-manager     428 knowledge-base   268 wsl-desktop    255 everything-plus
866 code-pad        373 api-playground   261 life-log       221 activity-timeline
290 developer-toolbox                    256 wsl-dashboard  (외 2개)
```

`packages/`는 비어 있다. 색·간격·radius·타이포·포커스 스타일의 공용 기준이 없다.
Devbox Manager가 이들을 하나의 제품군으로 제시하는데 시각적 일관성을 담보하는 것이 없다.

**추출 범위를 토큰으로 한정한다. 컴포넌트를 만들지 않는다.**

- 포함: CSS 커스텀 프로퍼티 (색상, 간격, radius, 폰트, 포커스 링, 상태색)
- 제외: React 컴포넌트, 레이아웃, 앱별 상태, 테마 전환 로직

토큰은 오늘 10개 앱이 전부 필요로 하는 것이 확인됐지만, 공용 컴포넌트는 두 번째 실소비자가
확인되지 않았다.

### 7.4 `CONVENTIONS.md` 스택 선언이 실제와 다르다

`CONVENTIONS.md` §3 "프론트엔드"가 선언한 스택 중 **실제로 쓰이는 것이 하나도 없다.**

| 선언 | 실제 |
|---|---|
| Tailwind CSS (Vite 플러그인) | 사용처 0. 손으로 쓴 `App.css` |
| `lucide-react` | 사용처 0 |
| `zustand` | 사용처 0 (code-pad는 자체 `store/documentStore.ts`) |
| `@tanstack/react-table` | 사용처 0 |
| `recharts` | 사용처 0 |
| `react-router-dom` | 사용처 0 |
| `@uiw/react-codemirror` | 사용처 0 (`@codemirror/*` 직접 사용) |

실제 공통 스택은 React 19 + TypeScript + Vite + 순수 CSS이며, code-pad만 `@codemirror/*`와
`mermaid`를, knowledge-base는 `mermaid`만 추가로 쓴다.

이 drift는 두 가지를 망친다. 새 앱을 스캐폴드하는 사람이 잘못된 스택을 도입하고, §7.3
토큰 작업이 "Tailwind 설정을 공용화해야 하나"라는 잘못된 질문에서 출발한다. **토큰 작업보다
먼저 고쳐야 한다.**

같은 문서의 다른 부정확한 서술도 함께 정리한다.

- §2 트리 주석 `crates/wsl (wsl-dashboard, life-log)` → 병합 후 실제 소비자는
  wsl-desktop, run-manager. life-log는 WSL을 쓰지 않는다.
- §3 "데이터 위치 규약"의 "life-log는 다른 앱의 DB를 읽기 위해 이 실제 경로를 기본값으로
  사용한다" → §3.2 병합과 §10.1로 폐기되는 결합이다.

### 7.5 CSP 기준선

12개 앱 전부 `src-tauri/tauri.conf.json`에 다음이 있다.

```json
"security": { "csp": null }
```

`create-tauri-app` 스캐폴드 기본값이며 한 번도 손대지 않았다.

**정확히 말하면 현재 알려진 악용 경로는 없다.** 검증한 완화 요인:

- 마크다운은 Rust에서 `ammonia`로 살균된다 (`crates/markdown/src/lib.rs:239` `sanitize`).
  `<script>` 제거와 `javascript:` 차단 테스트가 있다.
- mermaid는 두 앱 모두 `securityLevel: "strict"`로 초기화한다
  (`code-pad/src/components/PreviewPane.tsx:8`,
  `knowledge-base/src/components/MarkdownPreview.tsx:12`).
- API Playground는 응답 본문을 HTML로 렌더링하지 않는다.

따라서 이것은 취약점이 아니라 **hardening 공백**이다. 다만 `csp: null` + `core:default`
capability 조합에서는 어떤 경로로든 DOM injection이 성립하면 곧바로 `invoke`에 닿는다.
앱들이 임의의 로컬 파일(code-pad, knowledge-base, everything-plus)과 임의의 원격 응답
(api-playground)을 다루므로 방어선을 하나 더 둔다.

정책은 앱마다 다르다. 최소 기준선은 `default-src 'self'`이며 다음은 예외 검토가 필요하다.

- mermaid가 SVG를 삽입하는 code-pad·knowledge-base: `style-src` 인라인 허용 여부
- Vite dev 서버: 개발 모드에서 `connect-src`에 HMR 오리진
- 아이콘·폰트를 data URI로 쓰는 앱: `img-src data:`, `font-src data:`

## 8. P1 — Everything+ 실시간성

### 8.1 목표

전체 재인덱싱을 사용자가 반복하지 않아도 검색 DB가 실제 파일 시스템과 수렴하도록 한다.

### 8.2 기능 범위

- 등록 root별 `notify` watcher
- create/modify/remove/rename event 처리
- 짧은 구간의 중복 event debounce
- 파일명 변경 시 이전 FTS row 제거와 새 row 추가
- 내용 인덱싱 대상만 해당 content row 갱신
- root 제거 시 watcher와 pending event 함께 해제
- watcher overflow 또는 backend 오류 시 해당 root reconciliation scan
- 앱 재시작 시 watcher 복원
- root별 마지막 반영 시각, pending 수, 오류 상태

### 8.3 구현 주의

- editor save는 임시 파일 + rename 형태일 수 있으므로 단순 modify event만 가정하지 않는다.
- symlink/junction 정책은 기존 `crates/filesystem` 규칙과 일치시킨다.
- event path를 root 밖으로 canonicalize할 수 없으면 반영하지 않는다.
- 긴 content read는 watcher thread에서 수행하지 않는다.
- full re-index와 incremental writer가 같은 root를 동시에 변경하지 않도록 generation을 둔다.

`apps/code-pad/src-tauri/src/watcher.rs`(609줄)에 이미 `notify` 기반 watcher가 있다.
**먼저 읽고** debounce·rename 처리 방식을 참고한다. 세 번째 소비자(§11.2 knowledge-base)가
생기는 시점에 `crates/watcher` 추출을 판단한다.

### 8.4 후속 UX

검색 결과 keyboard navigation, 기본 앱으로 열기, containing folder 열기, 경로 복사,
Code Pad로 열기, saved query와 extension/size/modified filter.

### 8.5 완료 조건

- 정상 조건에서 create/modify/delete/rename이 2초 안에 검색 결과에 반영된다.
- rename 후 이전 경로가 남지 않는다.
- watcher overflow 뒤 자동으로 해당 root와 다시 수렴한다.
- 10만 파일 기준 검색 성능을 회귀시키지 않는다.

## 9. P1 — Life Log 정확성과 privacy

§3.2 병합 후 활동 수집기는 Life Log 안에 있다. 이 절의 작업 스코프는 `life-log`다.

### 9.1 목표

자리를 비운 시간과 민감한 창 제목을 무조건 저장하지 않도록 한다.

### 9.2 기능 범위

- `GetLastInputInfo` 기반 idle 감지
- Windows lock/unlock, suspend/resume 경계 처리
- 설정 가능한 idle threshold
- idle session 분리 또는 통계 제외
- process 이름별 수집 제외
- title 전체 미저장 규칙
- title regex redaction/치환
- private mode와 일정 시간 추적 일시중지
- 시작프로그램 등록 상태와 자동 시작
- 잘못 기록된 session의 split/delete 보정

### 9.3 데이터 경계

privacy rule은 UI 표시 단계가 아니라 **DB insert 전에** 적용한다. 제외하거나 치환하기로 한
원문은 DB, 진단 로그, integration snapshot 어디에도 남지 않아야 한다.

lock 또는 suspend 직전에 열린 session을 닫고, resume 후 새 observation으로 시작한다. 그렇지
않으면 밤새 하나의 긴 사용 session으로 합쳐질 수 있다.

### 9.4 완료 조건

- idle·lock 시간이 앱 사용 시간으로 집계되지 않는다.
- 제외한 title 원문이 persistence 계층에 나타나지 않는다.
- suspend/resume 전후 session이 잘못 합쳐지지 않는다.
- 자동 시작 여부를 앱 안에서 확인하고 되돌릴 수 있다.

## 10. P1 — 앱 간 연동 기반

### 10.1 versioned read-only snapshot

#### 문제

consumer가 producer의 내부 SQLite table과 경로를 직접 알면 producer migration이 consumer를
깨뜨린다. 현재 `apps/life-log/src-tauri/src/commands/life.rs:31`의 `set_activity_db(path)`가
정확히 그 구조다. §3.2 병합으로 이 쌍은 해소되지만, §11.1이 run-manager·knowledge·
everything+·code-pad를 source로 원하므로 계약 자체는 여전히 필요하다.

#### 제안 — devbox 공용 루트

producer가 **devbox 공용 루트** 아래에 privacy-safe snapshot을 원자적으로 기록한다.

```text
%LOCALAPPDATA%\devbox\integration\<producer-id>\v1\summary.json
```

```json
{
  "schemaVersion": 1,
  "producer": "run-manager",
  "producerVersion": "0.3.0",
  "generatedAt": "2026-08-14T12:00:00+09:00",
  "data": {}
}
```

`<producer-id>`는 §5.3 카탈로그의 앱 ID를 그대로 쓴다.

> **최초본에서 변경한 이유.** 최초본은 snapshot을 producer 자신의 bundle identifier 아래
> (`%LOCALAPPDATA%\com.workbench.activitytimeline\integration\v1\`)에 두자고 했다. 그러면
> consumer가 각 producer의 bundle identifier를 알아야 한다. 이는 Life Log가 남의 DB 경로를
> 아는 문제를 한 층 위로 옮긴 것에 불과하다.
>
> 공용 루트를 쓰면 consumer는 **이미 §5.3에서 만들기로 한 카탈로그의 앱 ID만** 알면 된다.
> P0 작업이 P1 작업을 직접 떠받친다.

규칙:

- producer만 자기 snapshot을 쓴다. consumer는 읽기만 한다.
- 임시 파일을 쓴 뒤 atomic rename한다.
- secret, authorization header, 원문 창 제목은 기본적으로 내보내지 않는다.
- consumer는 schema version과 freshness를 표시한다.
- producer가 꺼져 있어도 마지막 정상 snapshot은 읽을 수 있다.
- 원시 event 대량 조회가 꼭 필요할 때만 별도 versioned read-only DB view를 추가한다.
- 공용 루트는 사용자별 `%LOCALAPPDATA%` 아래이므로 추가 권한 설정이 필요 없다.

#### 파일럿

**첫 소비 흐름은 `run-manager → life-log`로 검증한다.** (§3.2 병합으로 activity-timeline
파일럿이 앱 내부가 되어 계약을 검증하지 못하기 때문이다.)

두 번째 producer(knowledge-base)가 같은 envelope를 사용하게 될 때 `crates/integration`으로
최소 추출한다.

`CONVENTIONS.md` §3 "데이터 위치 규약"에 이 공용 루트를 추가해야 한다.

### 10.2 ProjectProfile

Windows path, WSL path, Git root, worktree가 앱마다 별도 문자열이면 같은 프로젝트를 서로
다르게 인식한다.

**이미 두 벌이 존재한다.**

| 앱 | 저장 위치 |
|---|---|
| wsl-dashboard (병합 후 wsl-desktop) | **localStorage** `wsld-projects` (`src/lib/projectPaths.ts`) |
| life-log | **SQLite settings** (`set_setting(conn, "projects", ...)`) |

ProjectProfile은 세 번째 저장소가 아니라 **이 둘을 흡수하는 것**이다.

```text
ProjectProfile
├─ id: UUID
├─ name
├─ windowsPath
├─ wsl: { distro, path } | null
├─ gitRoot
├─ preferredEditor
├─ terminalProfileId
├─ runManagerJobIds[]
├─ runManagerServiceIds[]
└─ expectedPorts[]
```

초기에는 Workbench가 원본을 소유하는 단일 writer가 된다. 다른 앱은 project ID와 자기에게
필요한 context만 전달받는다. 공유 JSON을 여러 앱이 동시에 수정하는 구조는 피한다.

**선행 조건: §7.1 `crates/wsl`.** `wsl.distro`와 `wsl.path`의 정규화 규칙이 하나여야 같은
프로젝트가 하나로 식별된다.

### 10.3 앱 실행 context

초기에는 custom URL scheme보다 검증하기 쉬운 명시적 CLI argument를 권장한다.

```text
CodePad.exe    --workspace <path> --open <file>
WSLDesktop.exe --profile <id>
PortManager.exe --port <number>
RunManager.exe --service <id>
```

필수 규칙:

- 허용된 argument와 path를 앱 시작 시 엄격히 검증한다.
- 이미 실행 중인 앱에 전달할 경우 single-instance message schema에 version을 둔다.
- path와 ID는 shell command 문자열로 재조합하지 않는다.
- 앱이 설치되지 않았거나 schema가 다르면 명확한 fallback을 제공한다.

## 11. P1 — 데이터 앱 확장

### 11.1 Life Log

#### 방향

단순 활동 + Git 합계에서 **프로젝트별 개발 활동 설명**으로 확장한다.

#### 권장 source 순서

1. 내부 활동 데이터 (병합으로 앱 안에 있다)
2. Run Manager 실행 성공·실패와 service uptime — **integration 파일럿**
3. Knowledge의 note 작성·수정 수
4. Everything+의 프로젝트 root 내 생성·수정 파일 수
5. Code Pad의 privacy-safe 편집 요약

#### 기능

- source별 enabled, freshness, schema status
- ProjectProfile 기준 activity 귀속
- "오늘 가장 활동한 프로젝트"의 근거 breakdown
- source가 없거나 오래돼도 부분 결과 표시
- 같은 event를 여러 source에서 중복 집계하지 않는 규칙
- 일/주/월 Markdown과 JSON export
- 사용자가 직접 쓰는 회고와 자동 수집 데이터 분리

LLM 일기 생성은 source 정확성과 privacy 설정 이후의 opt-in 기능으로 둔다. API key 보호,
전송 payload preview, 민감 필드 제거가 선행되어야 한다.

#### 완료 조건

- source 하나가 실패해도 나머지 요약은 정상 표시된다.
- 모든 합계에서 source와 계산 근거를 확인할 수 있다.
- 미귀속 활동과 중복 제거 결과를 설명할 수 있다.
- `set_activity_db(path)` 방식의 직접 DB 경로 지정이 제거된다.

### 11.2 Knowledge Base

#### 기능 순서

1. **CodeMirror 편집기 도입** (§3.3 결정) — 현재 `App.tsx:283`의 `<textarea>` 교체
2. `packages/editor` 추출 (code-pad가 첫 소비자, knowledge-base가 두 번째)
3. 외부 변경 watcher와 검색 인덱스 증분 갱신
4. `[[wikilink]]` 해석과 unresolved link
5. backlink 패널
6. rename 전 깨질 link preview
7. system-wide quick capture와 Inbox note
8. daily/weekly template
9. knowledge root 내부 attachment 관리
10. opt-in Git status·commit

`packages/editor`는 테마나 전체 state를 공용화하지 않는다. CodeMirror extension setup,
언어 감지, 공통 keymap처럼 **두 앱에서 동일한 부분만** 옮긴다.

#272에서 4~5번을 하나의 parser/index 계약으로 구현했다. `[[target]]`·`[[target|alias]]`
자동완성은 root-relative path without `.md`를 삽입하며, 편집기와 preview가 missing/ambiguous/invalid
상태를 구분한다. backlink는 source path와 1-based line·UTF-16 column만 반환하고 클릭 시 해당
CodeMirror 위치로 이동한다. raw target은 파일 경로가 아니며 유일하게 resolve된 DB 상대 경로도
canonical root 내부 `.md`·10 MiB 경계에서 다시 검증한다. 외부 watcher와 앱 저장은 같은 link
metadata 갱신 함수를 사용하고 새 target은 source 재작성 없이 resolution/backlink에 반영된다.
#273에서 6번을 구현했다. 파일·폴더 rename을 즉시 실행하던 command 대신 다음 경계를 사용한다.

1. canonical root 내부 source와 덮어쓰지 않는 destination을 검증한다. 폴더를 자기 하위로 옮기거나
   symlink를 경유하는 경로, 동일 destination은 변경 전에 거부한다. 파일은 미래 key simulation과
   실제 link index의 종류를 일치시키기 위해 Markdown 여부(`.md`/비 Markdown)를 바꾸지 않는다.
2. root의 경로 inventory, 모든 Markdown, 이동 subtree 파일을 SHA-256 snapshot으로 묶는다.
   root 10,000항목, 읽은 내용 합계 64 MiB, rewrite 200파일·5,000링크를 상한으로 둔다.
3. 현재 유일하게 이동 note를 가리키는 link만 분석한다. 이동 후에도 동일 key가 유일하면 그대로
   두고, 깨지는 link만 새 root-relative path without `.md`로 바꾼다. 새 canonical key가 다른 title,
   filename 또는 path key와 충돌하면 preview 자체를 만들지 않는다. explicit alias와 target 주위
   whitespace는 보존한다.
4. UI에는 이동 경로와 영향받는 `[[...]]` syntax만 before/after로 보내고 전체 note body나 절대
   경로를 반환하지 않는다. `@devbox/diff-view`를 고정 목록 모드로 사용해 일부 link만 선택하는
   비원자 적용은 허용하지 않는다.
5. 원문 전체 plan은 `Serialize`/`Debug` 없는 app-managed slot 하나에만 보관한다. opaque ID는
   승인 한 번 또는 취소에 소비되며 새 preview가 이전 plan을 폐기한다.
6. apply 직전에 snapshot과 root/source/destination을 재검증한다. 통과한 경우 파일별 atomic
   rewrite, source rename, SQLite FTS/link transaction 순서로 수행하고 실패 시 이미 바뀐 파일,
   rename, 새 parent directory를 역순 복구한다. 이는 다중 파일 OS-global atomicity가 아니라
   bounded preflight + per-file atomic replace + rollback 기반 all-or-rollback이다.
7. dirty editor가 있는 동안 preview를 시작하지 않으며 성공 후 선택 note를 authoritative disk
   내용으로 다시 읽는다. watcher는 DB mutex가 풀린 뒤 event를 수렴시킨다.

#### 완료 조건

- 마크다운 편집에 문법 하이라이팅과 기본 keymap이 동작한다.
- 외부 editor 수정이 재시작 없이 검색·태그·backlink에 반영된다.
- rename 전에 영향받을 link를 확인할 수 있다.
- attachment와 preview path가 knowledge root 밖으로 벗어나지 않는다.

### 11.3 API Playground

#### 기능 순서

1. collection과 folder
2. environment별 `{{variable}}` 치환
3. DPAPI secret variable과 기본 masking
4. history와 저장 collection의 민감값 정책 분리
5. multipart/form-data와 file upload
6. OpenAPI 3 import → 선택 endpoint request 초안
7. collection/environment export·import
8. response search, header/cookie viewer, binary download
9. SSE와 WebSocket은 각각 별도 PR
10. Webhook Lab에서 포착한 요청 가져오기

v0.5.0 P1-09 #268에서 duplicate 순서, enabled persistence와 backend-only secret reference를
지원하는 request header table을 오프라인 native 기능으로 구현했다. 이어 #269에서 domain cookie
jar가 아닌 request `Cookie` header 전용 editor를 추가했다. 구조화 name/value 행은 순서와 enabled를
보존하고, 직접 값은 History·Collection·기본 cURL에서 마스킹하며 전체 값이 단일 environment
reference일 때만 참조를 유지한다. raw `Cookie` header와의 동시 사용, 잘못된 문자와 100행 초과는
native 전송 전 fail-closed한다. #270에서는 multipart body에 ordered text/file part, enabled,
part별 Content-Type과 native file picker를 추가했다. 파일 경로·byte backup은 저장하지 않고
basename만 남겨 History·Collection 재호출 때 재선택하게 한다. Rust가 파일당 25 MiB·전체 50 MiB와
regular file을 검증한 뒤 stream하며 text part의 secret reference도 backend에서만 해제한다.
#270은 PR #402로 CI 통과·머지됐다. #271에서는 응답을 Body/Headers/Cookies 전용 탭으로 나누고,
Headers는 ordered table, Cookies는 `Set-Cookie` name과 `[REDACTED]` value 및 bounded safe
attribute로 표시한다. 일반 DTO와 기본 복사는 항상 마스킹하고, raw headers는 현재 native 응답
1건에 한해 backend memory에 100개·64 KiB로 보관한다. 새 요청은 이전 raw entry를 폐기하며
stale response ID, 상한 초과, 비텍스트 header는 원문 복사를 fail-closed한다. 사용자가 별도 경고를
확인한 경우에만 전체 headers 또는 Set-Cookie 원문을 clipboard에 한 번 전달하며 저장·History·
Collection·로그에는 남기지 않는다. browser preview는 Fetch가 Set-Cookie를 노출하지 않는 한계를
명시하고 raw copy를 비활성화한다.

history는 자동·단기·secret 제거를 기본으로 한다. collection은 사용자가 명시적으로 저장한
재사용 자산으로 취급한다. Authorization, Cookie, 민감 multipart text와 file path를 history에
평문으로 남기지 않는다.

**`crates/secrets` 추출 지점.** run-manager가 DPAPI 환경변수 보호를 이미 구현했다
(`apps/run-manager/src-tauri/src/platform/environment.rs`). 위 3번이 두 번째 실소비자가
되므로 저장소 규칙대로 **그 PR에서** DPAPI 봉인/해제 프리미티브를 추출한다. 미리 뽑지 않는다.

#### 완료 조건

- environment 전환이 원본 request template을 변경하지 않는다.
- secret이 history, curl 복사, 오류 메시지에 나타나지 않는다.
- OpenAPI import가 기존 collection을 조용히 덮어쓰지 않는다.

## 12. P2 — Code Pad 다음 단계

### 12.1 crash recovery

현재 세션 복원과 별도로 opt-in 미저장 buffer recovery를 제공한다.

- 문서별 bounded snapshot
- debounce와 전체 저장량 상한
- 정상 저장·닫기 시 해당 recovery 제거
- 비정상 종료 후 원본 파일과 recovery의 시간·hash 비교
- 자동 덮어쓰기 대신 diff/복구/폐기 선택
- 손실 decoding 또는 read-only 파일에는 명확한 정책

세션 파일과 recovery 파일을 분리해 "열려 있던 파일"과 "저장하지 않은 내용"의 수명 정책을
섞지 않는다.

### 12.2 Problems와 navigation history

- 열린 문서 진단을 합친 Problems panel
- error/warning/source/code filter, 클릭 이동
- definition/reference 이동의 back/forward history
- server degraded/crash 상태를 현재 문서와 함께 표시
- diagnostics가 stale일 때 이전 오류를 계속 보이지 않음

### 12.3 안전한 multi-file rename

현재 UI는 열려 있지 않은 문서를 포함한 LSP 변경을 거부한다. 확장한다면 순서를 지킨다.

1. workspace 안의 대상만 허용
2. 모든 대상 파일의 snapshot을 먼저 읽고 hash 기록
3. 전체 변경 preview
4. 겹치는 edit와 resource operation 거부
5. 사용자가 승인한 뒤 임시 파일 + atomic replace
6. 하나라도 preflight 실패하면 아무 파일도 변경하지 않음
7. 적용 중 외부 변경을 발견하면 중단하고 결과 보고

Code Action과 서버 custom command는 임의 실행 경계를 넓히므로 이 단계와 함께 넣지 않는다.

### 12.4 앱 간 연결

- Everything+ 결과에서 Code Pad로 열기
- Workbench project context로 workspace 열기
- Run Manager job 정의에서 cwd를 Code Pad로 열기
- 진단 위치를 copyable `path:line:column`으로 내보내기

### 12.5 흩어진 네 기능이 요구하는 하나의 부품

이 문서의 서로 다른 절이 요구하는 다음 항목들은 같은 UI 부품을 필요로 한다.

| 위치 | 요구 |
|---|---|
| §12.1 crash recovery | "자동 덮어쓰기 대신 diff/복구/폐기 선택" |
| §12.3 multi-file rename | "전체 변경 preview" |
| §13.2 Run Manager definition import | "import preview와 충돌 처리" |
| §15.2 Workbench | "단계별 실패와 rollback 가능 여부 표시" |

넷 다 **"적용 전 변경 집합을 보여주고 사용자 승인을 받는 UI"** 다. 네 번 따로 만들면
동작과 용어가 갈라진다.

처리 방침은 저장소 규칙 그대로다.

1. §12.1 crash recovery에서 **Code Pad 안에** 만든다 (`apps/code-pad/src/components/`)
2. §13.2 Run Manager import가 두 번째 실소비자가 될 때 `packages/diff-view`로 추출한다

지금 미리 만들지 않는다. 다만 §12.1을 설계할 때 다음을 염두에 둔다.

- 입력을 "파일 경로 → 변경 전/후 텍스트" 목록으로 일반화한다 (Code Pad 문서 모델에 묶지 않는다)
- 승인/거부를 항목 단위와 전체 단위로 모두 받을 수 있게 한다
- 적용은 부품 밖에서 수행한다 (부품은 preview와 선택까지만 책임진다)

## 13. P2 — Run Manager 다음 단계

### 13.1 서비스 관찰성

- PID 또는 WSL identity의 안전한 표시
- 시작 시각과 uptime, 마지막 exit code와 종료 사유
- health probe 최근 결과와 마지막 성공 시각
- 다음 retry 시각과 backoff 단계
- restart 횟수와 최근 상태 전이
- 서비스별 stdout/stderr tail, 서비스 상태 이력

화면은 definition과 runtime instance를 명확히 분리한다. definition이 존재한다고 실행 중인
것이 아니고, DB state만으로 실제 process 생존을 단정하지 않는다.

### 13.2 정의 export/import

- job/service definition JSON schema version
- secret 값 제외, "secret configured" 표시만 export
- import preview와 충돌 처리 (§12.5의 부품을 여기서 추출한다)
- 다른 PC에서 WSL distro·cwd가 없을 때 disabled draft로 가져오기
- 선택한 VS Code `tasks.json` task를 job 초안으로 변환
- 원본 `tasks.json`은 수정하지 않음

### 13.3 실행 이력 UX

- status, 기간, 수동/예약 filter
- failure code 설명, 재실행
- 실행별 환경 key 이름만 확인하되 값은 표시하지 않음
- 로그 검색·download
- Life Log integration snapshot (§10.1 파일럿)

### 13.4 후순위 또는 제외

원격 host 실행, Kubernetes orchestration, 범용 DAG workflow, Windows Service Control
Manager 대체. 현재 local scheduler/service의 명확한 범위를 흐리므로 별도 제품 요구가 생길
때만 검토한다.

## 14. P3 — 나머지 기존 앱

### 14.1 Port Manager

- process 경로, command line, 시작 시각, CPU, memory 상세
- parent/child process tree
- Windows native, WSL forwarded, Docker published port 출처
- 즐겨찾는 포트와 예상 process 규칙
- 포트 열림/닫힘/소유자 변경 diff, 선택 포트 충돌 알림
- **Run Manager service로 이동** (§3.4 병합 대신 링크)
- Workbench project로 이동

kill 직전 PID뿐 아니라 process identity를 다시 확인해 PID 재사용 위험을 줄인다. system
또는 critical process에는 추가 확인을 둔다.

### 14.2 WSL Desktop (병합 후)

병합으로 흡수한 것과 원래 것을 함께 정리한다.

**흡수한 기능의 다음 단계**

- distro CPU/memory/disk와 `.wslconfig` limit 비교
- 컨테이너 목록을 distro 스코프로 유지 (Docker Desktop 복제 금지, §16)
- WSL restart 전 영향받는 terminal·service 표시
- Port Manager의 forwarded port로 이동

**원래 기능의 다음 단계**

- ProjectProfile 기반 distro/cwd/layout/startup command preset
- Workbench에서 terminal profile 열기
- broadcast 대상 pane을 실행 직전에 명확히 표시
- 위험 command pattern 경고와 broadcast history
- terminal search와 clickable path/URL
- shell-reported cwd/process 기반 pane title

**이관 대상**

- `gitStatus(projects)`와 localStorage 프로젝트 목록 → Workbench (§15.2). Workbench가
  생기기 전까지 유지한다

Windows Terminal과 범용 탭·테마·프로필 경쟁을 하기보다 프로젝트 layout과 안전한 broadcast를
차별점으로 유지한다.

### 14.3 Developer Toolbox

새 converter 개수를 늘리기보다 기존 도구를 연결한다.

- clipboard/input 자동 감지와 도구 추천
- `URL decode → Base64 decode → JSON format` pipeline
- 단계별 preview와 실패 위치
- 즐겨찾기·최근 사용·도구별 설정
- API Playground response를 formatter/JWT 도구로 보내기
- JSON↔YAML은 v0.5.0 P1-09 #264에서 오프라인 native 기능으로 구현
- UTF-8/Hex/Base64/Base64URL byte codec은 v0.5.0 P1-09 #265에서 오프라인 native 기능으로 구현
- 2·8·10·16진수 bounded radix converter는 v0.5.0 P1-09 #266에서 오프라인 native 기능으로 구현
- deterministic JSON→TypeScript type generator는 v0.5.0 P1-09 #267에서 오프라인 native 기능으로 구현
- CSV↔JSON과 URL parser는 수요·형식 경계가 확정되면 후속 추가

JWT decode와 signature verify는 별도 기능으로 구분한다. decode 성공을 신뢰 가능한 token으로
보이게 하지 않는다.

## 15. 신규 앱 후보

### 15.1 후보 비교

점수는 현재 저장소와의 상대 비교다. 5가 가장 높고, 위험은 5가 가장 큰 구현·보안 부담이다.

| 후보 | 사용자 영향 | 차별성 | 기존 코드 재사용 | MVP 경계 | 위험 | 권장 |
|---|---:|---:|---:|---:|---:|---|
| workbench | 5 | 5 | 5 | 3 | 4 | 구현 완료 |
| webhook-lab | 4 | 4 | 5 | 5 | 3 | 구현 완료 |
| dev-env-doctor | 4 | 4 | 4 | 5 | 2 | Manager 내부 구현 완료 |
| log-lens | 4 | 4 | 4 | 4 | 3 | **v0.5.0 신규 앱 선택** |
| repo-manager | 3 | 4 | 4 | 4 | 3 | 구현 완료, v0.5.0 강화 |
| data-inspector | 3 | 3 | 4 | 4 | 4 | v0.5.0 Manager 내부 기능 |
| devbox-launcher | 5 | 5 | 5 | 4 | 3 | **v0.5.0 신규 앱 선택** |

최초본의 “신규 앱 후보를 추가로 제안하지 않는다”는 결론은 **기반이 갖춰지기 전의 조건부
판단**으로 정정한다. v0.4.0~v0.4.1에서 catalog, `crates/applink`, `crates/integration`,
ProjectProfile과 공용 primitive가 실제로 생겼다. 이제 앱 간 계약 부재를 먼저 해결한다는
원칙은 유지하면서, 책임과 수명주기가 독립적인 Log Lens와 devbox 생태계 전용 Launcher를
추가하는 것이 타당하다. 두 앱은 기존 UI를 복제하지 않고 handoff·snapshot의 실사용
소비자가 된다.

### 15.2 Workbench — Project Workspace Orchestrator

#### 제품 정의

기존 앱 UI를 한 창에 복제하는 통합 앱이 아니다. 프로젝트를 기준으로 여러 앱과 서비스를
조정하고 상태를 요약하는 orchestration shell이다.

#### 핵심 흐름

```text
프로젝트 선택
  → Git/WSL/포트/서비스 사전 점검
  → Run Manager 서비스 시작
  → 예상 포트 준비 확인
  → WSL Desktop layout 열기
  → Code Pad workspace 열기
  → 필요하면 API request 열기
```

#### MVP

- ProjectProfile CRUD (§10.2 — 기존 두 저장소를 흡수)
- Git, WSL distro, expected port, Run Manager service 상태
- **WSL Desktop의 `gitStatus` 기능 이관** (§3.1)
- 앱 실행 context 전달
- `Start Workspace` / `Stop What I Started`
- 이미 실행 중이던 자원과 Workbench가 시작한 자원 구분
- 단계별 실패와 rollback 가능 여부 표시

#### 안전 경계

- 시작 전부터 실행 중이던 process/service는 자동 종료하지 않는다.
- 각 단계에 idempotency key 또는 실행 기록을 둔다.
- 다른 앱의 DB를 직접 수정하지 않는다.
- 앱이 없으면 Devbox Manager의 해당 앱 설치 화면으로 안내한다.
- 범용 앱·파일·웹 launcher를 만들지 않는다.

#### 선행 조건

| 조건 | 어디서 |
|---|---|
| 앱 카탈로그 | §17.3 PR 6 |
| identifier 확정 | §17.2 PR 2 |
| `crates/wsl` | §17.4 PR 14 |
| ProjectProfile 스키마 | §17.6 PR 28 |
| 최소 한 개 producer snapshot | §17.6 PR 26 |
| Run Manager service 상태 API | §17.7 PR 31 |
| 앱 실행 context 규약 | §10.3 |

### 15.3 Webhook Lab — Local Mock/Webhook Server

API Playground가 outbound HTTP client라면 Webhook Lab은 inbound HTTP 요청을 받고 검사하고
재현하는 로컬 서버다.

**MVP**: localhost bind 주소·포트 선택, method/path별 request history, headers/query/body/
timestamp, 고정 status/header/body response rule, delay와 대표 오류 응답, JSON fixture 저장,
captured request를 API Playground request로 변환, Port Manager primitive로 포트 충돌 확인.

**안전 경계**: 기본 bind는 `127.0.0.1`. LAN 공개는 명시적 경고와 별도 설정. Authorization·
Cookie·API key header 기본 masking. body 크기·history 개수·request rate 상한. fixture root
밖의 파일을 응답하지 못하게 한다. request를 받아 외부 command를 실행하는 hook은 MVP 제외.

**확장**: dynamic template과 request field echo, response sequence와 stateful scenario,
replay, HTTPS 개발 인증서, Run Manager service definition export, OpenAPI example에서
mock rule 생성.

### 15.4 Dev Environment Doctor

#### 제안 배경

앱이 여럿이 되면서 문제 원인이 앱 자체인지 Windows/WSL/Docker/runtime 설치 상태인지 구분하기
어려워졌다. 범용 시스템 최적화 앱이 아니라 devbox 실행 전제와 프로젝트 개발환경을 진단하는
read-only 도구다.

#### MVP

- Windows version과 WebView2 확인
- WSL 설치, distro, default version, path 변환 확인
- Git, Node, pnpm, Rust, Cargo 설치·버전 확인
- Docker Desktop/engine/WSL integration 상태
- Code Pad managed LSP runtime 상태
- Run Manager startup shortcut과 DB/log directory 접근 상태
- devbox 앱별 data directory와 schema/version 요약
- **카탈로그·manifest·설치 버전 정합 진단** — §5.2에서 발굴한 drift 재발 감시
- 진단 결과 복사와 redacted support bundle

#### 경계

MVP는 read-only다. 자동 설치·registry 수정·WSL reset을 하지 않는다. "수정"은 공식 명령과
문서 안내만 제공한다. path와 environment value는 support bundle에서 redaction한다.

#### 분리 판단 기준

먼저 Devbox Manager의 "환경 진단" 탭으로 검증한다. 진단 항목이 Manager의 설치 책임을 넘어
프로젝트별 health까지 확장되고 독립 실행 수요가 확인되면 별도 앱으로 분리한다.

이 탭은 사용자 기능이기 전에 **개발자 자신을 위한 회귀 감지 장치**다. §5.2·§7.1·§7.2에서
발굴한 drift가 정확히 그 탭이 진단해야 할 대상이다.

### 15.5 Log Lens — Unified Local Log Viewer

**v0.5.0 선택 확정.** local file tail, Run Manager job/service stdout·stderr handoff, WSL
file/journal, 설치된 container engine source, timestamp/level/source 정규화, plain text·regex·
JSONL/logfmt field filter, pause·follow·wrap·bookmark·export, saved view를 제공한다.

100,000행 또는 64MiB ring buffer와 backpressure를 사용한다. Docker 전용 dashboard는 만들지
않고 engine이 설치됐을 때 log adapter 하나로 연결한다. network ingest, arbitrary command,
ELK·distributed tracing 대체는 하지 않는다. 세부 source identity, rotation, merge 규칙은
v0.5.0 네이티브 우선 계획 §5 P3-02를 따른다.

### 15.6 Repo Manager — Git Worktree Manager

**MVP**: 지정 root 아래 Git repository 탐색, branch·dirty·ahead/behind·worktree 목록,
worktree 생성, Code Pad·WSL Desktop·Workbench로 열기, merged/stale branch 후보,
remove 전 uncommitted/untracked 검사.

force delete, reset, clean을 기본 동작으로 제공하지 않는다. Windows와 WSL path가 같은
repository를 중복 등록하지 않도록 canonical identity를 정의한다 (§7.1 `crates/wsl` 재사용).

### 15.7 Data Inspector — 내부 기능부터 검증

일반 SQLite GUI로 시작하지 않는다. v0.5.0에는 Devbox Manager의 환경 진단 옆 내부 기능으로
다음 read-only 기능을 먼저 제공한다: devbox DB 자동 발견, schema version·table/view·row
count, integrity check, 2초·1,000행 제한 read-only query, JSON/CSV export, backup. SQLite
read-only open, `PRAGMA query_only=ON`, authorizer write 차단을 함께 적용하고 임의 DB path는
받지 않는다.

실제 독립 사용성이 확인될 때만 새 앱으로 승격한다.

### 15.8 Devbox Launcher — Devbox Action Entrypoint

**v0.5.0 선택 확정.** PowerToys와 경쟁하는 범용 OS launcher가 아니다. catalog capability와
snapshot에서 devbox 앱, Workbench profile, repository/worktree, Run task/service, Everything+
saved query, WSL profile, Knowledge capture, Toolbox transformer를 발견하고 versioned applink와
handoff로 실행하는 devbox 전용 진입점이다.

기본 단축키는 `Ctrl+Alt+Space`이며 current clipboard/selected text는 실행 전 preview하고
저장하지 않는다. 범용 파일·웹·Windows 설정 검색, arbitrary shell command, clipboard history,
외부 plugin host는 범위에서 제외한다.

## 16. 책임 경계상 피할 범용 앱

외부 도구가 존재한다는 이유만으로 기능을 제외하지 않는다. 다음 항목은 외부 경쟁 회피가
아니라 devbox 앱의 책임과 설치 규모를 지키기 위해 범용 구현을 피하는 것이다. 필요한
devbox-scoped 하위 기능은 각 문단처럼 native로 제공한다.

**범용 command palette** — 앱·파일·웹·시스템 전체 launcher는 만들지 않는다. 대신 §15.8의
Devbox Launcher가 devbox app/action/context를 오프라인으로 검색하고 직접 handoff한다.

**범용 clipboard manager** — clipboard history와 OS 전역 감시는 만들지 않는다. Developer
Toolbox와 Launcher는 사용자가 호출한 순간의 current clipboard만 preview하고 pipeline·devbox
앱 간 입력 전달을 native로 제공한다.

**hosts·시스템 환경변수 편집기** — registry/hosts 전체 편집은 프로젝트 orchestration 책임을
벗어난다. Workbench project-scoped environment, Run Manager secret 주입, API Playground
environment 연결은 native로 강화한다.

**범용 terminal** — 모든 shell·platform을 포괄하지 않는다. WSL Desktop의 clipboard,
profile, native pane layout, action palette, ProjectProfile, multi-pane broadcast는 외부 terminal
없이 동작하도록 강화한다.

**Docker Desktop 복제** — image build, volume, registry, Kubernetes 전체 UI는 만들지 않는다.
WSL Desktop·Port Manager·Log Lens는 프로젝트 상태, port, container start/stop/log adapter를
engine-neutral 방식으로 native 제공한다.

---

# 17. 실행 계획

> **역사적 실행 계획.** 이 절의 PR 1~39와 Stage 4/5는 v0.4.0에서 완료됐다. v0.5.0의
> 현재 실행 순서와 PR 지도는
> [`2026-08-22-v0.5.0-native-first-plan.md` §7](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)을
> 따른다. 아래 내용은 기존 결정과 구현 근거를 보존하기 위해 유지한다.

## 17.0 이 계획을 읽는 법

### 상세도 계약

모든 단계를 같은 깊이로 쓰지 않았다. 쓸 수 있는 깊이가 다르기 때문이다.

| 범위 | 깊이 | 근거 |
|---|---|---|
| Stage -1 · 0a · 0b · 0.5 (PR 1~17) | **재현 가능 수준** — 정확한 파일 경로, 코드 스켈레톤, 실행할 명령, 기대 출력 | 대상 코드가 전부 존재하고 설계 결정이 끝났다 |
| Stage 1~3 (PR 18~39) | **작업 지시 수준** — 파일 경로, 단계, 체크리스트, 검증 명령. 미결정 사항은 `[설계]`로 표시 | 대상 코드는 존재하지만 동작·실패 정책이 미정이다 |
| Stage 4~5 | **선행조건·산출물·완료 조건** | 앱이 아직 없다. §1 원칙대로 별도 설계 문서에서 확정한다 |

`[설계]` 표시가 붙은 항목은 **해당 PR을 시작하기 전에 결정해야 하는 것**이다. 결정 없이
구현하면 결과가 갈라진다.

### 공통 규약 (모든 PR에 적용)

`CONVENTIONS.md` §8을 따른다. 반복하지 않기 위해 여기 한 번만 적는다.

- 브랜치: `<type>/<scope>/<name>`. 공통 작업의 scope는 `workspace`/`crates`/`packages`
- 커밋: Conventional Commits, 영어, 현재형. `feat(port-manager): add netstat parser`
- **1 기능 = 1 PR.** 여러 기능을 묶지 않는다
- 모든 PR은 `.github/workflows/ci.yml` 통과 후 `main`에 머지
- `main`에서 직접 작업하지 않는다

### 공통 검증 명령

```bash
# Rust (WSL). 새 셸에서는 먼저:
source ~/.cargo/env

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace

# 프론트 (WSL)
pnpm install --frozen-lockfile
pnpm -r build
pnpm -r test
pnpm -r exec tsc --noEmit

# 특정 앱만
cargo test -p <cargo-package>
pnpm --filter ./apps/<app> test
```

WSL에서 `src-tauri`를 컴파일하려면 시스템 라이브러리가 필요하다 (`AGENTS.md` 참조).

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev build-essential \
  libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

Windows에서만 가능한 검증은 각 PR의 **Windows 검증** 항목에 따로 적었다. Windows 접근이
없는 세션에서는 WSL 검증까지만 진행하고 Windows 항목을 이슈로 남긴다.

### 전체 순서

```
Stage -1  결정을 문서에 고정 (코드 변경 없음)
  PR 1   docs/conventions-alignment

Stage 0a  통폐합과 네이밍 (배포 정상화보다 먼저)
  PR 2   refactor/workspace/identifier-namespace
  PR 3   refactor/wsl-desktop/absorb-dashboard
  PR 4   refactor/life-log/absorb-activity-timeline

Stage 0b  배포 정상화 (10개 앱 기준)
  PR 5   chore/workspace/version-single-source
  PR 6   chore/workspace/app-catalog
  PR 7   test/workspace/catalog-consistency
  PR 8   build/workspace/catalog-release-matrix
  PR 9   build/workspace/release-manifest
  PR 10  test/devbox-manager/core-extraction
  PR 11  fix/devbox-manager/per-app-versions
  PR 12  fix/devbox-manager/download-integrity
  PR 13  feat/devbox-manager/atomic-update-rollback

Stage 0.5 공용 프리미티브
  PR 14  refactor/crates/wsl-extraction
  PR 15  refactor/crates/search-extraction
  PR 16  feat/packages/ui-tokens
  PR 17  chore/workspace/csp-baseline

Stage 1   정확성과 privacy          PR 18~25
Stage 2   앱 간 연동                PR 26~30
Stage 3   기존 앱 깊이              PR 31~39
Stage 4   Workbench
Stage 5   다음 독립 앱
```

**의존 관계 요약**

```
PR 1 ──> PR 16 (스택 선언이 정리돼야 토큰 방향이 맞다)
PR 2 ──> PR 3, PR 4 (identifier가 확정된 뒤 병합이 데이터를 흡수한다)
PR 3, PR 4 ──> PR 6 (카탈로그는 최종 10개 앱을 담는다)
PR 5 ──> PR 7 (검사 도입 전에 drift를 먼저 없앤다)
PR 9 ──> PR 10 (manifest 스키마가 확정돼야 파서를 쓴다)
PR 3 ──> PR 14 (병합 후 crates/wsl 범위가 정해진다)
```

Stage 0.5의 PR 15·17은 다른 PR과 파일이 겹치지 않아 언제든 병렬 진행할 수 있다.

---

## 17.1 Stage -1 — 결정을 문서에 고정

### PR 1 — `docs/conventions-alignment`

**목표.** §1.1의 8개 결정과 §7.4의 스택 drift를 `CONVENTIONS.md`에 고정한다. 코드 변경 없음.

**선행.** 없음

**왜 첫 번째인가.** 이후 모든 PR이 이 문서를 근거로 삼는다. 특히 PR 16(토큰)은 스택 선언이
정리되지 않으면 "Tailwind 설정을 공용화해야 하나"라는 잘못된 질문에서 출발한다.

**변경 파일**

| 경로 | 작업 |
|---|---|
| `CONVENTIONS.md` | §3 프론트 스택, §2 트리 주석, §4 버전 규칙, §8 뒤에 스택 추가 기준 신설, §3 "데이터 위치 규약" |
| `docs/architecture.md` | 크레이트 의존 그래프, 앱 목록 10개, 보안 경계 절 |
| `docs/projects.md` | 앱 표 10개, 공유 후보 매트릭스 |
| `docs/roadmap.md` | 병합과 Stage 구조 반영 |
| `README.md` | 앱 표 10개 |

**작업 순서**

1. `CONVENTIONS.md` §3 "프론트엔드" 항목을 실제와 일치시킨다. §7.4 표가 근거다.
   - 스타일: `Tailwind CSS` → `순수 CSS (앱별 App.css). 공용 토큰은 packages/tokens (PR 16 예정)`
   - 편집기: `@uiw/react-codemirror` → `@codemirror/* 직접 사용`
   - **제거**: `lucide-react`, `zustand`, `@tanstack/react-table`, `recharts`,
     `react-router-dom`. "필요해지면 그때 도입한다"는 문장으로 대체한다
   - 유지·추가: React 19, TypeScript strict, Vite, `@tauri-apps/api`, `mermaid`(code-pad·
     knowledge-base)
2. `CONVENTIONS.md`에 **스택 추가 기준**을 신설한다. §2.4의 블록을 그대로 넣는다.
3. `CONVENTIONS.md` §4에 **버전 단일 원본 규칙**을 넣는다.
   > 앱 버전은 `src-tauri/Cargo.toml`을 원본으로 하고, `src-tauri/tauri.conf.json`과
   > `package.json`은 항상 같은 값을 갖는다. 버전을 올릴 때 세 파일을 함께 수정한다.
4. `CONVENTIONS.md` §2 트리 주석을 고친다.
   - `wsl/ (wsl-dashboard, life-log)` → `wsl/ (wsl-desktop, run-manager)`
   - `search/ (everything-plus, knowledge-base)`에 "추출 예정 — PR 15" 표시
5. `CONVENTIONS.md` §3 "데이터 위치 규약"을 고친다.
   - identifier 예시를 `com.devbox.*`로 교체
   - 다음을 추가한다.
     > 앱 간 데이터 교환은 상대 앱의 `app_local_data_dir`을 직접 읽지 않고
     > `%LOCALAPPDATA%\devbox\integration\<app-id>\v<n>\`의 read-only snapshot을 사용한다.
     > (상세: `docs/product-opportunities.md` §10.1)
   - "life-log는 다른 앱의 DB를 읽기 위해 이 실제 경로를 기본값으로 사용한다" 문장을 제거한다
6. `CONVENTIONS.md` 최상단 앱 목록과 §2 트리를 **10개**로 고친다. 병합 결과를 반영한다
   (§4.3 표).
7. `docs/architecture.md`
   - 크레이트 의존 그래프에서 `wsl ◄── wsl-dashboard, wsl-desktop, life-log (후보)`를
     `wsl ◄── wsl-desktop, run-manager`로
   - `search ◄── everything-plus, knowledge-base (추출 예정)` 추가
   - "앱별 데이터 흐름"에서 wsl-dashboard·activity-timeline 항목을 병합 결과로 통합
   - **보안 경계** 절을 신설한다: ammonia 살균, mermaid `securityLevel: "strict"`, CSP가
     각각 무엇을 막는지 (§7.5)
8. `docs/projects.md` 앱 표를 10개로, 공유 후보 매트릭스에서 life-log의 `wsl` 제거
9. `docs/roadmap.md`의 "후속 작업"을 이 문서의 Stage 구조로 대체하고 이 문서를 링크
10. `README.md` 앱 표를 10개로

**체크리스트**

- [ ] 사용하지 않는 라이브러리 5개가 "선언된 스택"에서 빠졌다
- [ ] 실제 사용 중인 것(`@codemirror/*`, `mermaid`)이 반영됐다
- [ ] 스택 추가 기준 3조건과 sidecar 탈출구가 명문화됐다
- [ ] 버전 단일 원본 규칙이 있다
- [ ] identifier 예시가 `com.devbox.*`다
- [ ] devbox 공용 integration 루트가 데이터 위치 규약에 있다
- [ ] 5개 문서의 앱 목록이 전부 10개다
- [ ] 코드 변경이 없다

**검증**

```bash
# 문서가 선언한 라이브러리가 실제로 쓰이는지 역검증
grep -l '"tailwindcss"' apps/*/package.json | wc -l
grep -l '"lucide-react"' apps/*/package.json | wc -l
grep -l '"zustand"' apps/*/package.json | wc -l
grep -l '"@tanstack/react-table"' apps/*/package.json | wc -l
grep -l '"recharts"' apps/*/package.json | wc -l
grep -l '"react-router-dom"' apps/*/package.json | wc -l
grep -l '"@uiw/react-codemirror"' apps/*/package.json | wc -l
```

기대: 전부 `0`. 이 값이 0이 아닌 라이브러리만 문서에 남긴다.

```bash
# 문서에 남은 옛 이름
grep -rn "com.workbench" CONVENTIONS.md docs/ README.md
grep -rn "wsl-dashboard\|activity-timeline" CONVENTIONS.md docs/architecture.md docs/projects.md README.md
```

기대: 첫 번째는 0건. 두 번째는 §3 병합 설명 문맥에서만 나타난다 (앱 목록에는 없어야 한다).

**완료 조건**

- 위 grep 결과와 문서 서술이 일치한다
- CI 통과 (문서 변경이라 컴파일 게이트는 스킵된다)

**함정**

- 이 PR에서 코드를 고치고 싶어진다. 고치지 않는다. 문서와 코드를 같은 PR에서 바꾸면
  "무엇이 결정이고 무엇이 구현인지" 리뷰에서 구분되지 않는다.

---

## 17.2 Stage 0a — 통폐합과 네이밍

배포 정상화보다 **먼저** 한다. 카탈로그·manifest·Manager를 12개 기준으로 만든 뒤 10개로
줄이면 같은 작업을 두 번 한다.

### PR 2 — `refactor/workspace/identifier-namespace`

**목표.** 12개 앱의 bundle identifier를 `com.workbench.*` → `com.devbox.*`로 바꾸고,
기존 사용자 데이터를 새 경로로 이전한다.

**선행.** PR 1

**왜 병합보다 먼저인가.** 병합 PR(3·4)은 "상대 앱의 데이터를 흡수"해야 한다. identifier가
먼저 확정돼 있어야 흡수 대상 경로가 하나로 정해진다. 반대로 하면 병합 PR이 옛 경로와 새
경로를 모두 다뤄야 한다.

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/*/src-tauri/tauri.conf.json` × 12 | `identifier` 교체 |
| `apps/*/src-tauri/src/` × 12 | 데이터 디렉터리 마이그레이션 코드 추가 |
| `apps/life-log/src/api.ts` | MOCK 문자열의 옛 경로 교체 |
| `CONVENTIONS.md` | 이미 PR 1에서 반영됨 — 재확인만 |

**작업 순서**

1. 현재 값을 확인한다.
   ```bash
   grep -H '"identifier"' apps/*/src-tauri/tauri.conf.json
   ```
2. 12개 `tauri.conf.json`의 `identifier`를 §4.3 표대로 교체한다. 규칙은
   `com.workbench.X` → `com.devbox.X`이며 뒷부분(`X`)은 그대로 둔다.
3. **마이그레이션 함수를 공통 패턴으로 만든다.** 각 앱의 `lib.rs` setup에서 상태 초기화보다
   **먼저** 실행한다.
   ```rust
   /// 구 identifier 디렉터리가 있고 새 디렉터리가 없으면 통째로 옮긴다.
   /// 실패해도 앱을 막지 않는다 (로그만 남기고 빈 새 디렉터리로 시작).
   fn migrate_local_data(app: &tauri::AppHandle, legacy_id: &str) -> std::io::Result<()> {
       let new_dir = app.path().app_local_data_dir()?;      // %LOCALAPPDATA%\com.devbox.X
       if new_dir.exists() { return Ok(()); }
       let legacy_dir = new_dir.parent()?.join(legacy_id);  // %LOCALAPPDATA%\com.workbench.X
       if !legacy_dir.exists() { return Ok(()); }
       std::fs::rename(&legacy_dir, &new_dir)
   }
   ```
   [설계] `rename`이 실패할 때(다른 프로세스가 파일을 열고 있음, 볼륨 경계) 복사+삭제로
   fallback할지 결정한다. **같은 부모 아래이므로 볼륨 경계 문제는 없다.** 파일 잠금만
   고려하면 되고, 그 경우 다음 실행에서 재시도하는 것으로 충분하다.
4. 각 앱에서 자기 `legacy_id`를 상수로 넘긴다. 예: life-log는 `"com.workbench.lifelog"`.
5. `apps/life-log/src/api.ts`의 MOCK 문자열
   `"%LOCALAPPDATA%\\com.workbench.activitytimeline\\data.db"`를 새 경로로 바꾼다.
   (이 함수 자체는 PR 27에서 제거된다. 지금은 표시만 고친다.)
6. **마이그레이션 코드에 제거 예정 표시를 남긴다.**
   ```rust
   // TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
   ```

**체크리스트**

- [ ] 12개 `tauri.conf.json`이 전부 `com.devbox.*`다
- [ ] 12개 앱 전부 마이그레이션을 호출한다
- [ ] 마이그레이션이 상태 초기화(DB open)보다 **먼저** 실행된다
- [ ] 마이그레이션 실패가 앱 시작을 막지 않는다
- [ ] 새 디렉터리가 이미 있으면 아무것도 하지 않는다 (덮어쓰기 금지)
- [ ] 제거 예정 주석이 있다
- [ ] 앱 기능은 변경되지 않았다

**검증**

```bash
grep -c 'com.workbench' apps/*/src-tauri/tauri.conf.json
```

기대: 전부 `0`.

```bash
grep -H '"identifier"' apps/*/src-tauri/tauri.conf.json | sort
```

기대: 12줄 전부 `com.devbox.`로 시작.

```bash
grep -rn "com.workbench" apps/ --include=*.rs --include=*.ts --include=*.tsx
```

기대: `legacy_id` 상수와 그 주석에서만 나타난다.

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm -r build
```

**Windows 검증.** 필수다. 이 PR의 핵심은 데이터 이전이다.

1. 현재 버전을 실행해 앱 하나(예: knowledge-base)에 데이터를 만든다.
   `%LOCALAPPDATA%\com.workbench.knowledgebase\`가 존재하는지 확인한다.
2. 새 버전으로 교체해 실행한다.
3. `%LOCALAPPDATA%\com.devbox.knowledgebase\`로 옮겨졌고 앱에서 기존 데이터가 보이는지 확인한다.
4. `%LOCALAPPDATA%\com.workbench.knowledgebase\`가 사라졌는지 확인한다.
5. 한 번 더 실행해 오류 없이 시작하는지 확인한다 (두 번째 실행은 마이그레이션을 건너뛴다).
6. 신규 사용자 시나리오: 두 디렉터리를 모두 지우고 실행 → 정상 시작.

**완료 조건**

- 12개 앱의 identifier가 `com.devbox.*`
- 기존 사용자 데이터가 자동으로 이전된다
- 신규 설치도 정상 동작한다

**함정**

- **DB를 열기 전에 마이그레이션해야 한다.** 순서가 바뀌면 빈 DB가 새 경로에 생성되고,
  `new_dir.exists()`가 true가 되어 마이그레이션이 영원히 건너뛰어진다. 사용자 데이터가
  조용히 사라진 것처럼 보인다.
- activity-timeline은 트레이에서 상시 실행된다. 업데이트 시 구 프로세스가 남아 파일을
  잠그면 `rename`이 실패한다. 실패를 로그로 남기고 다음 실행에서 재시도하게 한다.

---

### PR 3 — `refactor/wsl-desktop/absorb-dashboard`

**목표.** wsl-dashboard의 기능을 wsl-desktop으로 옮기고 `apps/wsl-dashboard/`를 삭제한다.
기능 손실 없음.

**선행.** PR 2

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/wsl-desktop/src-tauri/src/commands/dashboard.rs` | 신규 — dashboard의 command 이관 |
| `apps/wsl-desktop/src-tauri/src/core/parsers.rs` | 신규 — dashboard의 `core/parsers.rs` 이관 |
| `apps/wsl-desktop/src-tauri/src/core/models.rs` | 신규 — `DistroInfo`, `ContainerInfo`, `GitStatus` |
| `apps/wsl-desktop/src-tauri/src/commands/mod.rs` | `dashboard` 모듈 등록 |
| `apps/wsl-desktop/src-tauri/src/lib.rs` | command 등록 추가 |
| `apps/wsl-desktop/src-tauri/Cargo.toml` | dashboard가 쓰던 의존 병합 |
| `apps/wsl-desktop/src/components/DistroPanel.tsx` | 신규 — distro·docker 패널 |
| `apps/wsl-desktop/src/components/ProjectPanel.tsx` | 신규 — gitStatus (이관 예정 표시) |
| `apps/wsl-desktop/src/lib/projectPaths.ts` + 테스트 | 이관 |
| `apps/wsl-desktop/src/api.ts`, `types.ts`, `App.tsx`, `App.css` | 병합 |
| `Cargo.toml` | `members`에서 `apps/wsl-dashboard/src-tauri` 제거 |
| `apps/wsl-dashboard/` | **삭제** |

**작업 순서**

1. **먼저 테스트를 옮긴다.** 이것이 회귀 방지선이다.
   - `apps/wsl-dashboard/src-tauri/src/core/parsers.rs`의 5개 테스트
     (`parses_wsl_list`, `parses_wsl_list_without_default_marker`, `parses_docker_ps`,
     `parses_git_status_clean`, `parses_git_status_dirty`)
   - `apps/wsl-dashboard/src-tauri/src/commands/wsl.rs`의 4개 디코딩 테스트
   - `apps/wsl-dashboard/src/lib/projectPaths.test.ts`
2. 테스트가 통과하도록 구현을 옮긴다.
3. **`decode_output` 중복을 여기서 해소한다.** wsl-dashboard와 wsl-desktop에 각각 있다.
   두 구현을 **나란히 놓고 diff를 확인한 뒤** 더 엄격한 쪽을 채택한다. 차이가 있으면 커밋
   메시지에 적는다. 결과는 wsl-desktop에 하나만 남는다.
   ```bash
   sed -n '/fn decode_output/,/^}/p' apps/wsl-dashboard/src-tauri/src/commands/wsl.rs > /tmp/a.rs
   sed -n '/fn decode_output/,/^}/p' apps/wsl-desktop/src-tauri/src/commands/terminal.rs > /tmp/b.rs
   diff /tmp/a.rs /tmp/b.rs
   ```
4. distro 목록 파싱도 통일한다. wsl-dashboard는 `parse_wsl_list -> Vec<DistroInfo>`,
   wsl-desktop은 `parse_distros -> Vec<String>`이다. **`DistroInfo` 구조체로 통일**하고
   터미널 쪽은 `.name`만 쓴다.
5. `open_terminal(distro)`(`wt.exe` 실행)을 **삭제**한다. 병합 후에는 앱 안의 터미널을
   열면 된다. 대신 distro 패널에 "이 distro로 새 탭 열기" 동작을 붙인다. 이것이 병합의
   핵심 가치다.
6. 프론트를 합친다. [설계] 레이아웃을 정한다. **권장: 좌측 사이드 패널(distro/docker/
   projects) + 우측 터미널 영역.** 터미널이 주 작업 영역이므로 패널은 접을 수 있게 한다.
7. `gitStatus`와 `projectPaths`는 그대로 옮기되 **이관 예정 표시**를 남긴다.
   ```ts
   // TODO(workbench): 프로젝트 목록과 git 상태는 Workbench의 ProjectProfile로 이관한다.
   // (docs/product-opportunities.md §3.1, §15.2). Workbench 출시 전까지 여기서 유지한다.
   ```
8. 루트 `Cargo.toml`의 `members`에서 제거하고 `apps/wsl-dashboard/`를 삭제한다.
9. `pnpm install`로 워크스페이스를 갱신한다.

**체크리스트**

- [ ] dashboard의 9개 Rust 테스트 + 1개 프론트 테스트가 전부 wsl-desktop으로 이동했다
- [ ] `decode_output`이 저장소 전체에 하나만 존재한다
- [ ] distro 모델이 `DistroInfo` 하나로 통일됐다
- [ ] `open_terminal`(`wt.exe`)이 제거되고 앱 내부 터미널로 대체됐다
- [ ] docker start/stop/restart가 동작한다
- [ ] `gitStatus`에 이관 예정 주석이 있다
- [ ] `apps/wsl-dashboard/`가 삭제됐다
- [ ] 루트 `Cargo.toml` `members`에서 제거됐다
- [ ] 기능 추가가 없다 (레이아웃 통합 외)

**검증**

```bash
cargo test -p wsl-desktop
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
pnpm install
pnpm --filter ./apps/wsl-desktop test
pnpm --filter ./apps/wsl-desktop build
```

```bash
# 앱이 사라졌는지
ls apps/ | grep wsl
```

기대: `wsl-desktop`만 출력.

```bash
# 중복 함수가 사라졌는지
grep -rn "fn decode_output" apps/ --include=*.rs | wc -l
```

기대: `1`.

```bash
# 워크스페이스 정합
grep -c "wsl-dashboard" Cargo.toml
```

기대: `0`.

**Windows 검증.** `pnpm tauri dev`로 실행하고 확인한다.

1. distro 목록이 이전 wsl-dashboard와 동일하게 나온다 (비ASCII 이름 distro가 있으면 그것으로)
2. distro 패널에서 "새 탭 열기"가 해당 distro로 터미널을 연다
3. docker 컨테이너 목록과 start/stop/restart가 동작한다
4. 프로젝트 git 상태가 이전과 동일하다
5. 기존 터미널 탭·pane 레이아웃 저장/복원이 깨지지 않았다

**완료 조건**

- 앱이 10+1개(devbox-manager 포함 11개 → 이 PR 후 11개, PR 4 후 10개)
- wsl-dashboard의 모든 기능이 wsl-desktop에서 동작한다
- `decode_output` 중복 0

**함정**

- wsl-desktop의 pane/탭 상태는 localStorage에 저장된다(`lib/storage.ts`). dashboard의
  `wsld-projects` 키와 충돌하지 않는지 확인한다. 둘 다 `wsld-` 접두사를 쓴다.
- `App.css` 병합 시 클래스 이름 충돌을 확인한다. 두 앱 모두 일반적인 이름
  (`.panel`, `.row`, `.badge`)을 쓸 가능성이 높다.

---

### PR 4 — `refactor/life-log/absorb-activity-timeline`

**목표.** activity-timeline의 수집기를 life-log로 옮기고 `apps/activity-timeline/`을
삭제한다. 기존 세션 데이터를 보존한다.

**선행.** PR 2

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/life-log/src-tauri/src/core/db.rs` | activity의 `core/db.rs`(159줄) 흡수 + life-log settings 테이블 |
| `apps/life-log/src-tauri/src/core/sessionizer.rs` | 신규 — activity의 sessionizer(117줄) |
| `apps/life-log/src-tauri/src/core/window.rs` | 신규 — activity의 window(52줄) |
| `apps/life-log/src-tauri/src/core/models.rs` | activity의 models 병합 |
| `apps/life-log/src-tauri/src/commands/tracking.rs` | 신규 — poller와 상태(69줄) |
| `apps/life-log/src-tauri/src/commands/queries.rs` | 신규 — activity의 queries(24줄) |
| `apps/life-log/src-tauri/src/core/readers/activity.rs` | **삭제** — 내부 DB 직접 조회로 대체 |
| `apps/life-log/src-tauri/src/lib.rs` | tray·poller setup 추가, command 등록 |
| `apps/life-log/src-tauri/Cargo.toml` | activity의 의존(`windows` 등) 병합 |
| `apps/life-log/src-tauri/tauri.conf.json` | tray 아이콘 설정 병합 |
| `apps/life-log/src/` | 타임라인·앱 통계 화면 추가 |
| `Cargo.toml` | `members`에서 `apps/activity-timeline/src-tauri` 제거 |
| `apps/activity-timeline/` | **삭제** |

**작업 순서**

1. **DB 스키마를 합친다.** 이 PR에서 가장 중요한 부분이다.
   - activity-timeline: `%LOCALAPPDATA%\com.devbox.activitytimeline\data.db` — 세션 테이블
   - life-log: `%LOCALAPPDATA%\com.devbox.lifelog\data.db` — settings만
   - 병합 후: `%LOCALAPPDATA%\com.devbox.lifelog\data.db` — 세션 + settings

   [설계] 세션 테이블을 그대로 가져올지 스키마를 손볼지 정한다. **그대로 가져오는 것을
   권장한다.** 이 PR은 병합이지 스키마 개선이 아니다. 개선은 PR 20·21에서 한다.

2. **일회성 데이터 흡수를 구현한다.** life-log 시작 시:
   ```rust
   // TODO(0.5.0): activity-timeline 병합에 따른 1회성 흡수. 두 릴리스 뒤 제거한다.
   fn absorb_activity_timeline_db(app: &tauri::AppHandle) -> Result<(), String> {
       // 1. 이미 흡수했으면(settings에 마커 존재) 아무것도 하지 않는다
       // 2. %LOCALAPPDATA%\com.devbox.activitytimeline\data.db 를 찾는다
       // 3. ATTACH 로 열어 세션 행을 life-log DB로 INSERT 한다
       // 4. 성공 시 settings에 마커를 기록한다
       // 5. 원본은 삭제하지 않는다 (사용자가 직접 정리)
   }
   ```
   [설계] 중복 삽입 방지 키를 정한다. 세션 테이블에 자연 키(시작 시각 + 프로세스)가 있으면
   그것을, 없으면 `INSERT OR IGNORE` + 유니크 인덱스를 쓴다.

   **원본을 삭제하지 않는 이유**: 흡수가 잘못되면 되돌릴 방법이 필요하다. 안내 문구로
   사용자가 직접 지우게 한다.

3. 수집기를 옮긴다. `core/db.rs`, `core/sessionizer.rs`, `core/window.rs`,
   `commands/tracking.rs`, `commands/queries.rs`. 순수 로직이므로 테스트가 함께 온다.
4. tray와 poller를 `lib.rs` setup에 붙인다. activity-timeline의 `setup_tray`(약 30줄)와
   `spawn_poller` 호출을 옮긴다.
5. **`core/readers/activity.rs`(131줄)를 삭제한다.** 외부 DB를 읽던 코드가 내부 테이블
   직접 조회로 바뀐다. `core/aggregate.rs`가 이를 호출하도록 수정한다.
6. `commands/life.rs`의 `set_activity_db` / `get_activity_db`를 **삭제**한다. 프론트
   `api.ts`의 대응 함수와 설정 UI도 함께 제거한다.
7. 프론트에 타임라인·앱 통계 화면을 추가한다. activity-timeline의 `App.tsx`(163줄)를
   life-log의 탭 또는 섹션으로 넣는다. [설계] 정보 구조를 정한다. **권장: "오늘/주/월"
   요약이 기본 화면, "타임라인 상세"가 보조 화면.** 요약이 제품 가치이기 때문이다.
8. `tauri.conf.json`에 tray 아이콘 설정을 병합한다. `LifeLog` productName은 유지한다.
9. 루트 `Cargo.toml`에서 제거하고 `apps/activity-timeline/`을 삭제한다.

**체크리스트**

- [ ] activity-timeline의 Rust 테스트가 전부 life-log로 이동했다
- [ ] 세션 DB 흡수가 구현됐고 중복 삽입을 막는다
- [ ] 흡수 마커가 있어 두 번 실행해도 안전하다
- [ ] 원본 DB를 삭제하지 않는다
- [ ] `set_activity_db` / `get_activity_db`가 Rust·프론트 양쪽에서 제거됐다
- [ ] `core/readers/activity.rs`가 삭제됐다
- [ ] tray가 동작하고 poller가 돈다
- [ ] `apps/activity-timeline/`이 삭제됐다
- [ ] 제거 예정 주석이 있다

**검증**

```bash
cargo test -p life-log
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm install
pnpm --filter ./apps/life-log test
pnpm --filter ./apps/life-log build
```

```bash
ls apps/ | wc -l
```

기대: `11` (10개 앱 + `catalog.json`은 아직 없으므로 10). PR 6 이후에는 11.

```bash
grep -rn "set_activity_db\|get_activity_db\|readers::activity" apps/
```

기대: 0건.

```bash
grep -c "activity-timeline" Cargo.toml
```

기대: `0`.

**Windows 검증.** 필수다.

1. 병합 전 버전으로 activity-timeline을 실행해 세션 데이터를 만든다
2. 병합 버전 life-log를 실행한다
3. 이전 세션 데이터가 요약과 타임라인에 나타나는지 확인한다
4. 트레이 아이콘이 뜨고 새 활동이 계속 기록되는지 확인한다
5. 앱을 두 번 재시작해 데이터가 중복되지 않는지 확인한다 (같은 날 사용 시간이 두 배가 되면
   흡수 마커가 동작하지 않는 것이다)
6. 신규 사용자 시나리오: 두 데이터 디렉터리를 지우고 실행 → 정상 시작, 빈 요약

**완료 조건**

- 앱이 **10개**가 된다
- 기존 세션 데이터가 보존된다
- 트레이 수집이 계속 동작한다
- life-log가 외부 DB 경로를 알지 못한다

**함정**

- **중복 삽입.** 흡수 마커 없이 매 시작마다 ATTACH+INSERT하면 사용 시간이 실행 횟수만큼
  부풀려진다. Windows 검증 5번이 이것을 잡는 항목이다.
- activity-timeline은 `windows` crate를 쓴다. life-log의 `Cargo.toml`에 의존을 옮길 때
  `#[cfg(target_os = "windows")]` 격리를 유지한다. 그러지 않으면 WSL에서
  `cargo check --workspace`가 깨진다.
- tray 아이콘 리소스 파일(`icons/`)도 함께 옮겨야 한다.

---

## 17.3 Stage 0b — 배포 정상화

이 시점의 앱은 **10개**다.

### PR 5 — `chore/workspace/version-single-source`

**목표.** `package.json`의 멈춘 `0.1.0`을 실제 버전에 맞춘다.

**선행.** PR 4 (앱이 10개로 확정된 뒤)

**왜 PR 7보다 먼저인가.** PR 7의 정합성 검사가 도입 즉시 실패하지 않게 하려면 이 정리가
먼저다.

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/api-playground/package.json` | `version` `0.1.0` → `0.2.0` |
| `apps/devbox-manager/package.json` | → `0.2.0` |
| `apps/developer-toolbox/package.json` | → `0.2.0` |
| `apps/everything-plus/package.json` | → `0.2.0` |
| `apps/knowledge-base/package.json` | → `0.2.0` |
| `apps/port-manager/package.json` | → `0.2.0` |
| `apps/life-log/package.json` | → `0.2.2` |
| `apps/wsl-desktop/package.json` | → `0.2.2` |
| `apps/code-pad/package.json` | → `0.3.0` |
| `apps/run-manager/package.json` | → `0.3.0` |

> 병합된 두 앱(`life-log`, `wsl-desktop`)의 `Cargo.toml` 버전을 PR 3·4에서 올렸다면 그 값에
> 맞춘다. 병합은 기능 변화이므로 minor 상승(예: `0.3.0`)이 자연스럽다. 세 파일이 일치하는
> 것이 이 PR의 목적이므로 어떤 값이든 셋을 같게 만든다.

**작업 순서**

1. 현재 값을 확인한다.
   ```bash
   grep -H '^version' apps/*/src-tauri/Cargo.toml
   grep -H '"version"' apps/*/src-tauri/tauri.conf.json apps/*/package.json
   ```
2. 각 앱의 `package.json` `version`을 같은 앱의 `Cargo.toml` `version`과 동일하게 맞춘다.
3. `pnpm install`(frozen 없이)을 실행해 `pnpm-lock.yaml`을 갱신하고 함께 커밋한다.

**체크리스트**

- [ ] 10개 앱 전부 세 파일의 version이 일치한다
- [ ] `pnpm-lock.yaml` 변경분을 커밋에 포함했다 (변경이 있다면)
- [ ] 코드 변경 없음 — 메타데이터만 건드린다

**검증**

```bash
python3 - <<'PY'
import json, os, re
bad = 0
for app in sorted(os.listdir("apps")):
    d = f"apps/{app}"
    if not os.path.isdir(d): continue
    cargo = re.search(r'^version\s*=\s*"([^"]+)"',
                      open(f"{d}/src-tauri/Cargo.toml").read(), re.M).group(1)
    tauri = json.load(open(f"{d}/src-tauri/tauri.conf.json"))["version"]
    pkg   = json.load(open(f"{d}/package.json"))["version"]
    ok = cargo == tauri == pkg
    bad += 0 if ok else 1
    print(("OK   " if ok else "FAIL "), app, cargo, tauri, pkg)
print("mismatches:", bad)
PY
```

기대: 10줄 전부 `OK`, `mismatches: 0`.

```bash
pnpm install --frozen-lockfile
pnpm -r build
```

**완료 조건**

- 위 스크립트가 `mismatches: 0`을 낸다
- CI 통과

**함정**

- `pnpm install --frozen-lockfile`은 lock과 manifest가 어긋나면 실패한다. 3번 단계에서
  lock을 먼저 갱신한다.

---

### PR 6 — `chore/workspace/app-catalog`

**목표.** `apps/catalog.json`을 앱 식별자의 단일 원본으로 도입한다. 아직 소비자는 없다.

**선행.** PR 5

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/catalog.json` | 신규 |
| `.github/scripts/ci-scope.sh` | `apps/catalog.json` 분기 추가 |
| `docs/architecture.md` | 카탈로그를 레이어 설명에 추가 |

**작업 순서**

1. 각 앱의 실제 값을 수집한다.
   ```bash
   grep -H '"productName"\|"identifier"' apps/*/src-tauri/tauri.conf.json
   grep -H '^name' apps/*/src-tauri/Cargo.toml
   ```
2. `apps/catalog.json`을 만든다. §5.3 스키마를 따르고 §4.3 표의 10개 앱을 등록한다.
   - `release`: 10개 전부 `true`
   - `managerVisible`: `devbox-manager`만 `false` (자기 자신을 목록에 표시하지 않는다)
   - `selfManaged`: `devbox-manager`만 `true`
   - **`version` 필드를 넣지 않는다** (§5.3)
3. `.github/scripts/ci-scope.sh`의 `case` 문에서 `apps/*)` **앞에** 분기를 넣는다.
   순서가 중요하다. `apps/*)`가 먼저 매칭되면 이 분기에 도달하지 않는다.
   ```bash
   apps/catalog.json)
     # 카탈로그는 release matrix와 Manager가 모두 소비하므로 양쪽 게이트를 켠다.
     frontend_all=true
     rust_all=true
     ;;
   ```
4. `docs/architecture.md`에 카탈로그의 역할을 한 문단 추가한다. "배포 대상 목록이자 런타임
   discovery의 단일 원본"이라는 점을 적는다.

**체크리스트**

- [ ] 10개 앱이 전부 등록됐다
- [ ] `id`가 `apps/` 디렉터리 이름과 정확히 같다
- [ ] `identifier`가 각 앱 `tauri.conf.json`의 값(`com.devbox.*`)과 같다
- [ ] `productName`이 각 앱 `tauri.conf.json`의 값과 같다
- [ ] `cargoPackage`가 각 앱 `src-tauri/Cargo.toml`의 `[package] name`과 같다
- [ ] 카탈로그에 version 필드가 없다
- [ ] `ci-scope.sh`의 새 분기가 `apps/*)` **앞**에 있다

**검증**

```bash
python3 -m json.tool apps/catalog.json > /dev/null && echo "valid JSON"
```

```bash
python3 - <<'PY'
import json, os
cat = json.load(open("apps/catalog.json"))
ids  = {a["id"] for a in cat["apps"]}
dirs = {d for d in os.listdir("apps") if os.path.isdir(f"apps/{d}")}
print("only in catalog:", ids - dirs)
print("only in apps/  :", dirs - ids)
print("count:", len(ids))
PY
```

기대: 두 집합 모두 비어 있고 `count: 10`.

```bash
bash -n .github/scripts/ci-scope.sh && echo "syntax OK"
grep -n -B1 'apps/catalog.json)' .github/scripts/ci-scope.sh
```

기대: `apps/catalog.json)` 분기가 `apps/*)`보다 위에 있다.

**완료 조건**

- 두 Python 스크립트가 불일치 0, 10개를 보고한다
- `bash -n` 통과, CI 통과

**함정**

- `case` 분기 순서. `apps/*)`가 위에 있으면 새 분기는 죽은 코드가 된다.
- `pnpm-workspace.yaml`의 `apps/*`는 디렉터리만 패키지로 인식하므로 `apps/catalog.json`은
  pnpm에 영향을 주지 않는다. 확인만 하고 넘어간다.

---

### PR 7 — `test/workspace/catalog-consistency`

**목표.** 카탈로그·Cargo workspace·pnpm workspace·앱별 버전이 어긋나면 CI가 실패한다.

**선행.** PR 5, PR 6

**변경 파일**

| 경로 | 작업 |
|---|---|
| `.github/scripts/check-catalog.sh` | 신규 |
| `.github/workflows/ci.yml` | `catalog` job 추가 |

**작업 순서**

1. `.github/scripts/check-catalog.sh`를 만든다. 다음 여섯 가지를 검사하고, 하나라도
   실패하면 `exit 1`한다. 실패 시 **무엇이 어긋났는지 정확히 출력**한다.

   | # | 검사 |
   |---|---|
   | 1 | 카탈로그 `id` 집합 == `apps/` 하위 디렉터리 집합 |
   | 2 | 카탈로그 `cargoPackage` 집합 ⊆ 루트 `Cargo.toml`의 `[workspace] members`가 가리키는 패키지 |
   | 3 | 각 앱의 `Cargo.toml` / `tauri.conf.json` / `package.json` version 3자 일치 |
   | 4 | 카탈로그 `identifier`·`productName` == 해당 앱 `tauri.conf.json` 값 |
   | 5 | 카탈로그 `appDir`이 실제로 존재하고 `package.json`을 가진다 |
   | 6 | 모든 `identifier`가 `com.devbox.` 로 시작한다 |

   구현 언어는 자유지만 러너에 이미 있는 것을 쓴다. **bash + python3 권장, `jq` 의존을
   추가하지 않는다.**

2. `.github/workflows/ci.yml`에 job을 추가한다. **scope 게이트를 타지 않는다.**
   카탈로그 정합성은 어떤 변경에서도 싸고 빠르게 확인할 가치가 있다.
   ```yaml
   catalog:
     name: Catalog consistency
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v4
       - run: bash .github/scripts/check-catalog.sh
   ```

3. **실패 경로를 직접 확인한다.** 임시로 한 앱의 `package.json` version을 틀리게 바꾸고
   스크립트가 그 앱 이름과 세 값을 출력하며 실패하는지 본다. 확인 후 되돌린다.

**체크리스트**

- [ ] 여섯 검사가 전부 구현됐다
- [ ] 실패 메시지가 "어떤 앱의 어떤 값이 어떻게 다른지" 출력한다
- [ ] `jq` 등 러너에 없을 수 있는 도구를 쓰지 않았다
- [ ] job이 scope 게이트에 걸리지 않는다
- [ ] 실패 경로를 실제로 한 번 확인했다

**검증**

```bash
bash .github/scripts/check-catalog.sh && echo "PASS"
```

기대: `PASS`.

```bash
# 실패 경로 확인 (반드시 되돌릴 것)
sed -i 's/"version": "0.2.0"/"version": "9.9.9"/' apps/port-manager/package.json
bash .github/scripts/check-catalog.sh; echo "exit=$?"
git checkout apps/port-manager/package.json
```

기대: `exit=1`이고 `port-manager`와 불일치한 세 값이 출력된다.

```bash
# identifier 검사 확인
sed -i 's/com.devbox.portmanager/com.workbench.portmanager/' apps/port-manager/src-tauri/tauri.conf.json
bash .github/scripts/check-catalog.sh; echo "exit=$?"
git checkout apps/port-manager/src-tauri/tauri.conf.json
```

기대: `exit=1`.

**완료 조건**

- 정상 상태에서 통과, 인위적 drift 두 종류에서 실패
- CI에 `catalog` job이 나타난다

---

### PR 8 — `build/workspace/catalog-release-matrix`

**목표.** release workflow가 앱 목록을 하드코딩하지 않고 카탈로그에서 읽는다. 앱별 staging
디렉터리로 stale artifact를 차단한다.

**선행.** PR 6, PR 7

**변경 파일**

| 경로 | 작업 |
|---|---|
| `.github/workflows/release.yml` | 앱 목록 생성, 앱별 staging, upload 경로, dispatch 기본값, tag 중복 방지 |

**작업 순서**

1. `build-windows` job에 카탈로그를 읽는 step을 추가한다.
   ```yaml
   - id: apps
     shell: bash
     run: |
       list=$(python3 -c "import json;print(' '.join(a['id'] for a in json.load(open('apps/catalog.json'))['apps'] if a['release']))")
       echo "list=$list" >> "$GITHUB_OUTPUT"
       echo "빌드 대상: $list"
   ```
   [설계] 앱별 병렬 matrix job으로 갈지 단일 job 순차 빌드를 유지할지 정한다. **첫 PR에서는
   순차를 유지하고 목록만 카탈로그에서 읽는 것을 권장한다.** matrix는 빠르지만 Rust 캐시가
   job마다 분리돼 총 소요가 오히려 늘 수 있다. matrix 전환은 별도 PR로 측정 후 결정한다.

2. 빌드 루프에서 앱 하나를 빌드한 **직후** 그 앱의 산출물만 staging으로 옮긴다.
   ```text
   staging/<app-id>/portable/<app-id>.exe
   staging/<app-id>/installer/<ProductName>_<version>_x64-setup.exe
   ```
   Cargo workspace라 `target/release/`를 모든 앱이 공유한다. 빌드 직후 즉시 옮기지 않으면
   어느 앱의 산출물인지 구분할 수 없다.

3. `upload-artifact`의 `path`를 `staging/**`로 바꾼다. `target/release/*.exe` glob을
   제거한다. `if-no-files-found: error`는 유지한다.

4. `publish` job의 `files:`를 staging 구조에 맞춘다.

5. `workflow_dispatch`의 `version` 입력에서 `default: "v0.1.0"`을 제거한다
   (`required: true`만 남긴다).

6. tag 중복 방지 step을 `publish` 앞에 넣는다.
   ```yaml
   - name: Fail if tag already exists
     env:
       GH_TOKEN: ${{ github.token }}
       TAG: ${{ steps.tag.outputs.tag }}
     run: |
       if gh release view "$TAG" >/dev/null 2>&1; then
         echo "release $TAG already exists"; exit 1
       fi
   ```

**체크리스트**

- [ ] release.yml에 앱 이름 배열이 남아 있지 않다
- [ ] 앱별 staging 디렉터리를 쓴다
- [ ] `target/release/*.exe` glob이 제거됐다
- [ ] `workflow_dispatch` 기본 version이 제거됐다
- [ ] 기존 tag 재사용 시 실패한다
- [ ] publish는 전체 빌드 성공 후에만 실행된다 (`needs:` 유지)

**검증**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml')); print('valid YAML')"
```

```bash
grep -n 'port-manager\|code-pad\|run-manager\|wsl-desktop' .github/workflows/release.yml
```

기대: 아무것도 출력하지 않는다 (카탈로그에서 읽으므로).

```bash
grep -n 'target/release/\*.exe' .github/workflows/release.yml
```

기대: 0건.

**Windows 검증.** 필요하다. 실제 릴리스 전에 `workflow_dispatch`로 임시 tag를 써서 한 번
돌린다.

1. artifact를 내려받아 `staging/<app-id>/` 10개가 각각 portable 1개 + installer 1개를
   가지는지 확인
2. 같은 tag로 다시 실행 → tag 중복 방지 step에서 실패하는지 확인

**완료 조건**

- dry-run에서 10개 앱의 산출물이 각자 staging에 정확히 하나씩 존재
- 이전 빌드 산출물이 섞이지 않음

**함정**

- Tauri NSIS 산출물 이름은 `<productName>_<version>_x64-setup.exe`다. `version`은 앱의
  `tauri.conf.json` 값이지 release tag가 아니다. PR 5에서 세 원본을 맞췄으므로
  `Cargo.toml`에서 읽어도 같다.
- `gh release view`는 `GH_TOKEN`이 필요하다. `github.token`으로 충분하다.

---

### PR 9 — `build/workspace/release-manifest`

**목표.** `release-manifest.json`을 생성해 release asset으로 올리고, 업로드된 파일이
manifest와 일치하는지 릴리스 후 검증한다.

**선행.** PR 8

**변경 파일**

| 경로 | 작업 |
|---|---|
| `.github/scripts/build-manifest.py` | 신규 |
| `.github/scripts/verify-release.py` | 신규 |
| `.github/workflows/release.yml` | manifest 생성 step, verify job 추가 |

**작업 순서**

1. `build-manifest.py`를 만든다. staging 디렉터리를 훑어 §5.4 스키마의 manifest를 만든다.
   - `id`: 카탈로그에서
   - `version`: 앱의 `src-tauri/Cargo.toml`에서
   - `portable` / `installer`의 `name`·`size`·`sha256`: staging의 **실제 파일**에서 계산
   - `releaseTag`: workflow 입력 또는 `github.ref_name`
   - `generatedAt`: UTC ISO-8601
   - `notices`: lockfile에서 생성한 `THIRD_PARTY_NOTICES.md`의 `name`·`size`·`sha256`
   - 앱 하나라도 portable 또는 installer가 없으면 **실패**한다 (조용한 누락 금지)
2. `publish` job의 `files:`에 `THIRD_PARTY_NOTICES.md`와 `release-manifest.json`을 포함한다.
3. `verify-release.py`와 이를 실행하는 `verify` job을 추가한다. `needs: publish`.
   - GitHub API로 release asset 목록을 가져온다
   - manifest에 있는 모든 asset이 존재하는지 확인
   - 각 asset을 내려받아 size와 SHA-256을 **다시 계산**해 대조
   - **manifest에 없는 asset이 release에 있으면 실패**한다
     ([설계] 경고가 아니라 실패로 한다. 무엇이 배포됐는지 선언과 실제가 같아야 한다.
     단 `release-manifest.json` 자신은 예외 목록에 넣는다.)
4. verify 실패 시 release를 draft로 되돌리거나 삭제할지 정한다.
   [설계] **첫 구현에서는 실패만 시키고 자동 삭제하지 않는 것을 권장한다.** 자동 삭제는
   부분 실패 시 복구를 어렵게 만든다. 실패를 보고 사람이 판단한다.

**체크리스트**

- [ ] manifest에 10개 앱이 전부 있다
- [ ] 각 앱의 `version`이 release tag가 아니라 앱 실제 버전이다
- [ ] portable/installer 각각 name·size·sha256이 있다
- [ ] notices asset의 name·size·sha256이 있고 installer resource에도 같은 파일이 포함된다
- [ ] sha256이 64자 hex다
- [ ] `release-manifest.json`이 release asset으로 올라간다
- [ ] verify job이 실제로 다운로드해 digest를 재계산한다
- [ ] manifest에 없는 asset을 감지한다
- [ ] 앱의 산출물이 하나라도 없으면 manifest 생성이 실패한다

**검증**

```bash
# 로컬에서 스키마 검증 (staging을 흉내낸 디렉터리로 build-manifest.py 실행 후)
python3 - <<'PY'
import json
m = json.load(open("release-manifest.json"))
assert m["schemaVersion"] == 1
assert len(m["apps"]) == 10, len(m["apps"])
for a in m["apps"]:
    for kind in ("portable", "installer"):
        assert set(a[kind]) >= {"name", "sha256", "size"}, a["id"]
        assert len(a[kind]["sha256"]) == 64, a["id"]
        assert a[kind]["size"] > 0, a["id"]
assert m["notices"]["name"] == "THIRD_PARTY_NOTICES.md"
assert len(m["notices"]["sha256"]) == 64
assert m["notices"]["size"] > 0
print("manifest OK:", len(m["apps"]), "apps")
PY
```

**Windows 검증.** 필요하다.

1. 임시 tag로 실제 릴리스를 만들고 verify job이 통과하는지 확인
2. asset 하나를 수동으로 삭제 → verify 실패 확인
3. manifest에 없는 파일을 release에 추가 → verify 실패 확인
4. manifest의 앱 버전이 tag와 다를 수 있음을 실제로 확인 (예: tag `v0.4.0`인데
   code-pad는 `0.3.0`)

**완료 조건**

- 정상 릴리스에서 verify 통과
- asset 변조·삭제·추가 시 verify 실패
- §5.6의 완료 조건 전부 충족

---

### PR 10 — `test/devbox-manager/core-extraction`

**목표.** Devbox Manager에 테스트 가능한 순수 로직 계층을 만든다. 앱 동작 변경 없음.

**선행.** PR 9 (manifest 스키마가 확정돼야 파서를 쓴다)

**왜 별도 PR인가.** PR 11~13이 전부 이 계층 위에서 동작한다. 테스트 없이 안전성 작업을
하면 검증할 방법이 없다. 현재 이 앱에는 Rust·프론트 테스트가 **0건**이다.

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/devbox-manager/src-tauri/src/core/mod.rs` | 신규 |
| `apps/devbox-manager/src-tauri/src/core/catalog.rs` | 신규 — 카탈로그 파싱 |
| `apps/devbox-manager/src-tauri/src/core/manifest.rs` | 신규 — manifest 파싱 |
| `apps/devbox-manager/src-tauri/src/core/version.rs` | 신규 — semver 파싱·비교 |
| `apps/devbox-manager/src-tauri/src/core/asset.rs` | 신규 — asset 선택 |
| `apps/devbox-manager/src-tauri/src/core/url_policy.rs` | 신규 — URL allowlist |
| `apps/devbox-manager/src-tauri/src/lib.rs` | `mod core;` 추가 |

**작업 순서**

1. `core/` 디렉터리를 만든다. `CONVENTIONS.md` §4의 모듈 구조를 따른다. `core/`는 OS 의존이
   없는 순수 로직이며 WSL에서 `cargo test`로 검증한다.

2. `catalog.rs` — §5.3 스키마를 `serde` 구조체로 정의하고 파싱한다.
   ```rust
   pub struct CatalogApp {
       pub id: String,
       pub display_name: String,
       pub product_name: String,
       pub identifier: String,
       pub cargo_package: String,
       pub app_dir: String,
       pub release: bool,
       pub manager_visible: bool,
       pub self_managed: bool,
   }
   ```
   - 알 수 없는 `schemaVersion`은 명확한 에러로 거부한다
   - 필드 누락도 에러로 거부한다 (조용한 기본값 금지)

3. `manifest.rs` — §5.4 스키마. 같은 원칙을 적용한다.

4. `version.rs` — `major.minor.patch[-prerelease]` 파서와 `Ord` 구현.
   - `v` 접두사는 여기서 다루지 않는다. release 계층에서만 정규화한다 (§6.2)
   - prerelease는 같은 `major.minor.patch`의 stable보다 **낮게** 정렬한다
   - `is_prerelease()`를 노출한다

5. `asset.rs` — `(manifest, app_id, mode)` → `AssetRef { name, sha256, size }`.
   **파일명 추측 로직을 넣지 않는다.** 없으면 명확한 에러를 낸다.

6. `url_policy.rs` — 허용 host와 경로 prefix 검사.
   ```rust
   const ALLOWED_HOSTS: &[&str] = &[
       "github.com",                  // release asset 최초 URL
       "objects.githubusercontent.com", // asset redirect 대상
   ];
   ```
   [설계] 허용 redirect host 목록을 확정한다. GitHub의 asset redirect 대상이 바뀔 수 있으므로
   상수로 두고 **주석에 근거와 확인 날짜를 남긴다.** 검사 항목: scheme이 `https`인가,
   host가 목록에 있는가, 경로가 `jihoon22-lee/devbox` 하위인가.

7. 각 모듈에 `#[cfg(test)] mod tests`를 작성한다. §6.6 목록을 커버한다.

**체크리스트**

- [ ] `core/`에 Windows 전용 코드가 없다
- [ ] `core/`에 네트워크·파일 IO가 없다 (전부 순수 함수)
- [ ] 카탈로그·manifest 파서가 알 수 없는 schemaVersion을 거부한다
- [ ] 버전 비교 테스트: `0.2.0 < 0.2.2 < 0.3.0`, `0.3.0-rc1 < 0.3.0`
- [ ] asset 선택 테스트: 없는 앱, 없는 mode, 정상
- [ ] URL allowlist 테스트: 허용 host, 비허용 host, `http`, 경로 이탈, 다른 레포
- [ ] 기존 command 동작을 바꾸지 않았다

**검증**

```bash
cargo test -p devbox-manager
cargo clippy -p devbox-manager --all-targets -- -D warnings
cargo fmt --all --check
```

기대: `running N tests`에서 N이 15 이상 (§6.6의 8개 항목을 각각 정상·비정상으로 커버).

```bash
# core에 금지 항목이 없는지
grep -rn "reqwest\|std::fs\|tauri::" apps/devbox-manager/src-tauri/src/core/
```

기대: 0건.

**완료 조건**

- `cargo test -p devbox-manager`가 §6.6 항목을 커버하는 테스트로 통과
- 앱 동작 변경 없음

---

### PR 11 — `fix/devbox-manager/per-app-versions`

**목표.** 하드코딩 앱 배열과 파일명 추측을 제거하고 카탈로그 + manifest를 소비한다.

**선행.** PR 10

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/devbox-manager/src-tauri/src/commands/manager.rs` | `latest()` → manifest 조회, `catalog()` command 추가 |
| `apps/devbox-manager/src-tauri/build.rs` 또는 `lib.rs` | 카탈로그 임베드 |
| `apps/devbox-manager/src/App.tsx` | `APPS` 상수 제거(`:6`), `findAsset` 제거(`:40`) |
| `apps/devbox-manager/src/api.ts` | `latest`/`installApp` 시그니처 변경, MOCK 갱신 |
| `apps/devbox-manager/src/types.ts` | manifest·카탈로그 타입 추가 |
| `apps/devbox-manager/src/App.test.tsx` | 신규 |
| `apps/devbox-manager/vite.config.ts` | vitest 설정 (없으면 추가) |

**작업 순서**

1. Rust에 command 두 개를 만든다.
   - `catalog() -> Vec<CatalogApp>` — 번들에 포함된 카탈로그를 반환
   - `available() -> ReleaseManifest` — GitHub에서 `release-manifest.json`만 받아 파싱

   [설계] 카탈로그를 빌드 시 임베드할지(`include_str!`) 릴리스 asset으로 받을지 정한다.
   **임베드를 권장한다.** Manager 자신의 버전이 아는 앱 목록이 명확해지고 오프라인에서도
   목록이 보인다. 새 앱은 Manager 업데이트로 반영된다.
   ```rust
   const CATALOG_JSON: &str = include_str!("../../../catalog.json");
   ```
   경로는 `apps/devbox-manager/src-tauri/src/` 기준이므로 `../../../catalog.json`이
   `apps/catalog.json`을 가리킨다. 빌드 시 실제 경로를 확인한다.

2. 프론트에서 `APPS` 배열과 `findAsset()`을 **삭제**한다. 목록은 `catalog()`, 버전과 asset은
   `available()`에서 온다.

3. 각 앱 행에 **앱별 버전**을 표시한다. release tag는 화면 상단에 한 번만 표시한다.
   - 설치된 버전 / 최신 버전 / 상태(최신·업데이트 가능·미설치)

4. `installApp`의 인자에서 `url`과 `version`을 제거한다. `install(appId, mode)`만 남긴다.
   Rust가 manifest에서 asset을 고른다 (§6.3).

5. `managerVisible: false`인 앱은 목록에서 제외한다.

6. 프론트 테스트를 추가한다. `isTauri()` false 경로의 MOCK으로 렌더링하고 확인한다.
   - 카탈로그 10개 중 `managerVisible` 대상 9개만 표시되는지
   - 앱별 버전이 각각 다르게 표시되는지 (code-pad `0.3.0`, port-manager `0.2.0`)
   - 업데이트 가능 판정이 앱별로 독립적인지

**체크리스트**

- [ ] `App.tsx`에 앱 이름 배열이 없다
- [ ] asset 이름을 문자열 조합으로 만드는 코드가 없다
- [ ] 앱마다 서로 다른 최신 버전이 표시된다
- [ ] `managerVisible: false`인 앱이 목록에 없다
- [ ] `installApp`이 url과 version을 받지 않는다
- [ ] 프론트 테스트가 추가됐다 (이 앱의 첫 프론트 테스트)

**검증**

```bash
cargo test -p devbox-manager
pnpm --filter ./apps/devbox-manager test
pnpm --filter ./apps/devbox-manager build
pnpm --filter ./apps/devbox-manager exec tsc --noEmit
```

```bash
grep -n 'port-manager\|"CodePad"\|_x64-setup\|browser_download_url' apps/devbox-manager/src/App.tsx apps/devbox-manager/src/api.ts
```

기대: 아무것도 출력하지 않는다.

**Windows 검증.** `pnpm tauri dev`로 실행한다.

1. 9개 앱이 나타난다 (자기 자신 제외)
2. code-pad는 `0.3.0`, port-manager는 `0.2.0`처럼 서로 다른 버전이 보인다
3. 설치·실행이 이전과 동일하게 동작한다

**완료 조건**

- §6.7의 마지막 두 항목 충족
- 프론트 테스트 통과

---

### PR 12 — `fix/devbox-manager/download-integrity`

**목표.** §6.3의 일곱 경계를 구현한다.

**선행.** PR 11

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/devbox-manager/src-tauri/src/core/download.rs` | 신규 — 검증 결과 타입, size·digest 판정 |
| `apps/devbox-manager/src-tauri/src/commands/manager.rs` | `download()` 전면 교체, `install()` 흐름 변경, `write_registry()` 원자화 |
| `apps/devbox-manager/src-tauri/Cargo.toml` | `sha2` 추가, `reqwest`에 `stream` feature 추가, `futures-util` 추가 |

**작업 순서**

1. `Cargo.toml`에 의존을 추가한다.
   ```toml
   sha2 = "0.10"
   futures-util = "0.3"
   reqwest = { version = "0.13.4", features = ["json", "stream"] }
   ```
   `Response::bytes_stream()`은 `stream` feature가 있어야 한다.

2. `download()`를 다음 순서로 다시 쓴다.
   1. `core::url_policy`로 요청 전 URL 검증
   2. redirect를 따라간 뒤 **`Response::url()`로 최종 URL을 다시 검증**
   3. `Content-Length`가 manifest size와 다르면 즉시 중단
   4. `download/<version>.partial`로 streaming 기록. 청크마다 SHA-256 업데이트하고
      누적 바이트가 manifest size를 넘으면 즉시 중단
   5. 완료 후 총 바이트와 digest를 manifest와 대조
   6. 불일치면 `.partial`을 삭제하고 에러
   7. 일치하면 최종 경로로 rename

3. `install()`에서 **검증 성공 이후에만** installer를 실행한다. 현재는 다운로드 직후 바로
   `std::process::Command::new(&dest).spawn()`한다 (`manager.rs:118` 부근).

4. `write_registry()`를 임시 파일 + rename으로 바꾼다.
   ```rust
   let tmp = path.with_extension("json.tmp");
   std::fs::write(&tmp, json)?;
   std::fs::rename(&tmp, &path)?;
   ```

5. 앱 시작 시 남아 있는 `.partial`을 정리한다.
   [설계] 재개(resume)를 지원할지 정한다. **첫 구현에서는 삭제 후 재시도를 권장한다.**
   HTTP Range 재개는 digest 계산 상태까지 이어받아야 해서 복잡도가 크다.

6. 실패 메시지에 URL 전체를 노출하지 않는다. 앱 ID와 실패 종류만 보여준다.

**체크리스트**

- [ ] URL allowlist가 요청 전과 redirect 후 **두 번** 적용된다
- [ ] 전체를 메모리에 올리지 않는다 (`resp.bytes()` 제거)
- [ ] `.partial`에 쓰고 검증 후 rename한다
- [ ] size 초과 시 조기 중단한다
- [ ] digest 불일치 시 파일을 남기지 않는다
- [ ] installer는 검증 후에만 실행된다
- [ ] registry가 임시 파일 + rename으로 기록된다
- [ ] 시작 시 고아 `.partial`을 정리한다

**검증**

```bash
cargo test -p devbox-manager
cargo clippy -p devbox-manager --all-targets -- -D warnings
```

순수 로직 테스트로 커버할 것:

- size mismatch 판정 (작을 때 / 클 때)
- digest mismatch 판정
- allowlist 통과·거부 (redirect 후 host 변경 포함)
- `.partial` 이름 생성 규칙

```bash
grep -n "resp.bytes()" apps/devbox-manager/src-tauri/src/commands/manager.rs
```

기대: 0건.

**Windows 검증.** 다음 세 시나리오를 수동으로 확인한다.

1. **정상 설치** — 성공하고 `.partial`이 남지 않는다
2. **다운로드 중 네트워크 차단** — 실패하고 이전 버전이 그대로 실행된다. 재시도하면 성공한다
3. **manifest의 sha256을 일부러 틀리게 수정한 뒤 설치** — 실패하고 파일이 저장되지 않는다.
   installer 모드에서는 **실행되지 않는다**

**완료 조건**

- §6.7 첫 항목("변조되거나 잘린 파일은 저장 완료나 실행 상태가 되지 않는다") 충족
- 위 세 시나리오 확인 완료

**함정**

- `reqwest`의 기본 redirect policy는 최대 10회를 자동으로 따라간다. 중간 hop이 아니라
  **최종 응답의 URL**을 검증해야 한다.
- Windows에서 `std::fs::rename`은 대상이 있으면 교체하지만, 대상 파일이 다른 프로세스에
  열려 있으면 실패한다. 실패를 사용자 메시지로 변환하고 재시도 경로를 둔다.
- `Content-Length`가 없는 응답이 있을 수 있다. 그 경우에도 **누적 바이트 상한 검사는
  반드시 동작해야 한다.**

---

### PR 13 — `feat/devbox-manager/atomic-update-rollback`

**목표.** §6.4의 layout과 rollback을 구현한다.

**선행.** PR 12

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/devbox-manager/src-tauri/src/core/layout.rs` | 신규 — 경로 계산, rollback 대상 선택 |
| `apps/devbox-manager/src-tauri/src/commands/manager.rs` | 설치 경로를 layout으로 이관, `rollback` command 추가 |
| `apps/devbox-manager/src/App.tsx` | 현재/이전 버전 표시, rollback UI |
| `apps/devbox-manager/src/api.ts` | rollback 호출 |
| `apps/devbox-manager/src/App.test.tsx` | rollback UI 테스트 추가 |

**작업 순서**

1. `core/layout.rs`에 §6.4의 경로 규칙을 **순수 함수**로 만든다. 실제 IO 없이 `PathBuf`
   계산만 하도록 해서 테스트 가능하게 유지한다.
   ```rust
   pub fn version_dir(base: &Path, app_id: &str, version: &str) -> PathBuf
   pub fn current_json(base: &Path, app_id: &str) -> PathBuf
   pub fn partial_file(base: &Path, app_id: &str, version: &str) -> PathBuf
   pub fn pick_rollback_target(current: &Current, installed: &[Version]) -> Option<Version>
   ```

2. `current.json` 스키마를 정한다.
   [설계] 최소 필드: `version`, `exePath`, `installedAt`, `previousVersion`.

3. 설치 흐름을 바꾼다.
   1. `versions/<new>/`에 완전히 준비
   2. 검증 통과
   3. `current.json`을 임시 파일에 쓰고 rename
   4. **이전 버전 디렉터리를 삭제하지 않는다**

4. `rollback(appId)` command를 만든다. `previousVersion`이 존재하고 그 디렉터리에 실행
   파일이 있으면 `current.json`을 되돌린다.

5. 보존 정책을 정한다. [설계] **직전 버전 1개 보존을 권장한다.** 그 이전 버전은 "정리 가능"
   으로 표시하되 자동 삭제하지 않는다.

6. UI에 현재 버전, 이전 버전, "이전 버전으로 되돌리기" 버튼을 추가한다. 이전 버전이 없으면
   버튼을 비활성화한다.

**체크리스트**

- [ ] `versions/<v>/` 구조로 설치된다
- [ ] `current.json`만 원자 교체된다
- [ ] 직전 정상 버전이 최소 하나 남는다
- [ ] rollback이 UI에서 가능하다
- [ ] 업데이트 중단 후에도 기존 버전이 실행된다
- [ ] `layout.rs`가 IO 없이 테스트된다

**검증**

```bash
cargo test -p devbox-manager
pnpm --filter ./apps/devbox-manager test
```

```bash
grep -rn "std::fs\|reqwest" apps/devbox-manager/src-tauri/src/core/layout.rs
```

기대: 0건 (순수 함수 유지).

**Windows 검증.**

1. 앱 A를 이전 버전으로 설치 → 새 버전으로 업데이트 → `versions/`에 둘 다 존재하는지 확인
2. rollback 실행 → `current.json`이 이전 버전을 가리키고 그 버전이 실행되는지 확인
3. 업데이트를 중간에 강제 종료 → 앱 A가 여전히 이전 버전으로 실행되는지 확인
4. 이전 버전이 없는 신규 설치에서 rollback 버튼이 비활성인지 확인

**완료 조건**

- §6.7 전 항목 충족

---

## 17.4 Stage 0.5 — 공용 프리미티브

Stage 0b와 병렬 진행할 수 있다. 단 **PR 1 → PR 16**, **PR 3 → PR 14** 순서는 지킨다.

### PR 14 — `refactor/crates/wsl-extraction`

**목표.** §7.1 경계대로 `crates/wsl`을 만들고 wsl-desktop과 run-manager가 소비하게 한다.
동작 변경 없음.

**선행.** PR 3 (병합으로 크레이트 범위가 확정된 뒤)

**추출 근거.** 병합 후 `decode_output`은 소비자가 1개가 되므로 그것만으로는 대상이 아니다.
이 크레이트의 근거는 **§10.2 ProjectProfile의 canonical identity**다. Windows 경로와 WSL
경로의 정규화 규칙이 하나여야 같은 프로젝트를 하나로 식별한다.

**변경 파일**

| 경로 | 작업 |
|---|---|
| `crates/wsl/Cargo.toml` | 신규 |
| `crates/wsl/src/lib.rs` | 신규 |
| `crates/wsl/src/distro.rs` | 신규 — `validate_distro_name` |
| `crates/wsl/src/argv.rs` | 신규 — `build_exec_argv`, `build_wslpath_argv` |
| `crates/wsl/src/path.rs` | 신규 — Windows↔WSL 경로 정규화 |
| `Cargo.toml` | `members`에 `crates/wsl` 추가 (기존 주석 해제) |
| `apps/wsl-desktop/src-tauri/Cargo.toml` | 의존 추가 |
| `apps/wsl-desktop/src-tauri/src/commands/terminal.rs` | argv 구성을 크레이트 호출로 |
| `apps/wsl-desktop/src-tauri/src/commands/dashboard.rs` | 같음 |
| `apps/run-manager/src-tauri/Cargo.toml` | 의존 추가 |
| `apps/run-manager/src-tauri/src/core/shell.rs` | argv 빌더와 distro 검증 이관 |

**작업 순서**

1. 크레이트를 만든다. **`windows` crate 등 OS 전용 의존을 넣지 않는다**
   (`CONVENTIONS.md` §4). 의존은 최소화한다.

2. **기존 테스트를 먼저 옮긴다.** 회귀 방지선이다.
   - `run-manager/core/shell.rs`의 argv 빌더 테스트와 distro 검증 테스트
   - `build_wslpath_conversion_argv` 관련 테스트

3. `distro.rs` — `validate_distro_name`. run-manager의 `core/shell.rs:308`을 옮긴다.
   빈 문자열, 제어 문자, argv 주입 가능한 문자를 거부한다.

4. `argv.rs` — 두 함수를 제공한다.
   ```rust
   pub fn build_exec_argv(distro: &str, cwd: Option<&str>, command: &str)
       -> Result<Vec<String>, WslError>;
   pub fn build_wslpath_argv(distro: &str, windows_path: &str)
       -> Result<Vec<String>, WslError>;
   ```
   run-manager의 `build_wslpath_conversion_argv`(`core/shell.rs:313`)를 그대로 옮긴다.

5. **run-manager의 `build_wsl_command`는 통째로 옮기지 않는다.** 그 함수는 marker·handshake·
   `WSLENV` 준비까지 한다. 그중 순수 argv 조립 부분
   (`wsl.exe -d <distro> [--cd <cwd>] -- bash -c ...`)만 `build_exec_argv`로 빼고,
   run-manager는 그것을 호출한 뒤 자기 정책을 덧붙인다.

6. `path.rs` — Windows↔WSL 경로 정규화 규칙.
   ```rust
   pub fn windows_to_wsl(path: &str) -> Result<String, WslError>;   // E:\a\b → /mnt/e/a/b
   pub fn wsl_to_windows(distro: &str, path: &str) -> Result<String, WslError>;
   pub fn canonical_project_key(windows: &str, wsl: Option<(&str, &str)>) -> String;
   ```
   [설계] `canonical_project_key`의 규칙을 확정한다. §10.2 ProjectProfile과 §15.6
   Repo Manager가 이 키로 "같은 프로젝트"를 판정한다. **드라이브 문자 대소문자, 후행
   슬래시, UNC(`\\wsl$\...`) 표기를 정규화 대상에 포함한다.**

   `wslpath` 실행이 필요한 경우와 문자열 규칙만으로 되는 경우를 구분한다. 문자열 규칙으로
   되는 것(`/mnt/<drive>/` 형태)은 프로세스 실행 없이 처리한다.

7. wsl-desktop과 run-manager가 크레이트를 쓰도록 바꾸고, 원본 함수와 그 테스트를 **삭제**한다.

**체크리스트**

- [ ] 크레이트에 `#[cfg(target_os = "windows")]` 코드가 없다
- [ ] 크레이트에 `Command::new` 호출이 없다
- [ ] 옮긴 함수의 테스트가 전부 크레이트로 이동했다
- [ ] 두 앱에서 원본 함수와 그 테스트가 **삭제**됐다
- [ ] run-manager의 marker·handshake 정책은 앱에 남아 있다
- [ ] `decode_output`·`parse_distro_list`는 **옮기지 않았다** (소비자 1개, §7.1)
- [ ] `canonical_project_key`에 테스트가 있다
- [ ] 루트 `Cargo.toml` `members`에 등록됐다
- [ ] 동작 변경 없음 (기능 추가 금지)

**검증**

```bash
cargo test -p wsl
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

```bash
# 크레이트에 금지 항목이 없는지
grep -rn "Command::new\|target_os" crates/wsl/src/
```

기대: 0건.

```bash
# 소비자 확인
grep -rn "crates/wsl" apps/*/src-tauri/Cargo.toml
```

기대: wsl-desktop, run-manager 두 줄.

```bash
# 중복이 사라졌는지
grep -rn "fn build_wslpath_conversion_argv\|fn validate_distro" apps/ --include=*.rs
```

기대: 0건.

**Windows 검증.**

1. wsl-desktop에서 distro로 터미널이 열리고 지정 cwd로 진입한다
2. run-manager의 WSL job이 이전과 동일하게 실행·종료된다
3. 경로에 공백이나 비ASCII가 포함된 프로젝트로 확인한다

**완료 조건**

- 옮긴 함수의 중복 0
- 두 앱 모두 이전과 동일하게 동작
- `cargo test --workspace` 통과

**함정**

- crate 이름. 기존 크레이트가 `filesystem`, `markdown`, `process`처럼 짧은 이름을 쓰므로
  `wsl`로 일관성을 유지한다. 앱에서는 code-pad가 하듯
  `devbox-wsl = { package = "wsl", path = "../../../crates/wsl" }` 별칭을 쓸 수 있다.
- `canonical_project_key`는 이 크레이트에서 **가장 중요한 함수**다. 규칙이 흔들리면
  ProjectProfile 전체가 흔들린다. 테스트를 넉넉히 쓴다.

---

### PR 15 — `refactor/crates/search-extraction`

**목표.** §7.2. `build_fts_query` 중복을 `crates/search`로 뽑는다. 동작 변경 없음.

**선행.** 없음 (다른 PR과 파일이 겹치지 않는다)

**변경 파일**

| 경로 | 작업 |
|---|---|
| `crates/search/Cargo.toml` | 신규 |
| `crates/search/src/lib.rs` | 신규 — `build_fts_query` |
| `Cargo.toml` | `members`에 `crates/search` 추가 |
| `apps/everything-plus/src-tauri/Cargo.toml` | 의존 추가 |
| `apps/everything-plus/src-tauri/src/core/db.rs` | `build_fts_query`(`:237`) 제거, 크레이트 사용 |
| `apps/knowledge-base/src-tauri/Cargo.toml` | 의존 추가 |
| `apps/knowledge-base/src-tauri/src/core/db.rs` | `build_fts_query`(`:122`) 제거, 크레이트 사용 |

**작업 순서**

1. **두 구현을 나란히 놓고 diff를 확인한다.** 조용히 한쪽을 지우지 않는다.
   ```bash
   sed -n '/fn build_fts_query/,/^}/p' apps/everything-plus/src-tauri/src/core/db.rs > /tmp/ep.rs
   sed -n '/fn build_fts_query/,/^}/p' apps/knowledge-base/src-tauri/src/core/db.rs > /tmp/kb.rs
   diff /tmp/ep.rs /tmp/kb.rs
   ```
   차이가 있으면 **더 엄격한(더 많이 이스케이프하는) 쪽을 채택**하고 커밋 메시지에 적는다.

2. 두 앱의 기존 테스트를 먼저 크레이트로 옮긴다.

3. 크레이트를 만든다. 의존 없음(순수 문자열 처리)이 이상적이다.
   ```rust
   /// 사용자 입력을 FTS5 MATCH 식으로 변환한다.
   /// 토큰 단위 prefix 매치이며, FTS5 특수문자는 인용부호로 감싸 무력화한다.
   pub fn build_fts_query(input: &str) -> String;
   ```

4. **스키마 DDL은 옮기지 않는다.** 두 앱의 테이블 구조가 다르다
   (`files_fts(name)` / `file_content_fts(content)` vs `docs_fts(title, body)`).
   공통화하면 각 앱의 스키마 진화를 막는다.

5. 두 앱에서 원본 함수와 그 테스트를 **삭제**한다.

**체크리스트**

- [ ] 두 구현의 diff를 확인하고 채택 근거를 커밋 메시지에 남겼다
- [ ] 기존 테스트가 크레이트로 이동했다
- [ ] 두 앱에서 원본 함수가 **삭제**됐다
- [ ] 스키마 DDL을 옮기지 않았다
- [ ] 빈 문자열, 특수문자(`"`, `*`, `-`, `^`, `:`), 다중 공백 테스트가 있다
- [ ] 동작 변경 없음

**검증**

```bash
cargo test -p search
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
grep -rn "fn build_fts_query" apps/ crates/ --include=*.rs
```

기대: `crates/search/src/lib.rs` 한 줄만.

```bash
grep -rn "search = {" apps/*/src-tauri/Cargo.toml
```

기대: everything-plus, knowledge-base 두 줄.

**Windows 검증.** 두 앱에서 검색을 실행해 결과가 이전과 같은지 확인한다. 특히 특수문자가
포함된 검색어(`foo"bar`, `a-b`, `*x`)로 확인한다.

**완료 조건**

- `build_fts_query` 중복 0
- 두 앱의 검색 결과가 이전과 동일

---

### PR 16 — `feat/packages/ui-tokens`

**목표.** 10개 앱이 공유하는 CSS 커스텀 프로퍼티 토큰을 만든다. 컴포넌트는 만들지 않는다.

**선행.** PR 1 (스택 선언이 정리된 뒤여야 방향이 맞다)

**분할 권장.** 10개 앱을 한 PR에서 다 바꾸면 리뷰가 불가능하다.

| PR | 범위 |
|---|---|
| 16a | `packages/tokens` 생성 + 앱 2개 적용 (가장 큰 run-manager, 가장 작은 life-log) |
| 16b | 앱 4개 |
| 16c | 나머지 4개 |

토큰 정의가 첫 두 앱에서 검증된 뒤 나머지를 적용하는 것이 안전하다.

**변경 파일 (16a 기준)**

| 경로 | 작업 |
|---|---|
| `packages/tokens/package.json` | 신규 (`@devbox/tokens`) |
| `packages/tokens/tokens.css` | 신규 |
| `packages/tokens/README.md` | 신규 — 토큰 목록과 사용법 |
| `apps/run-manager/package.json`, `apps/life-log/package.json` | 의존 추가 |
| `apps/run-manager/src/App.css`, `apps/life-log/src/App.css` | 하드코딩 값을 `var(--...)`로 |
| `CONVENTIONS.md` | §3 스타일 항목 확정 |

**작업 순서**

1. 현재 10개 앱의 CSS에서 실제 사용 중인 값을 수집한다. **새 팔레트를 발명하지 않는다.**
   ```bash
   grep -ho '#[0-9a-fA-F]\{3,8\}' apps/*/src/*.css | sort | uniq -c | sort -rn | head -40
   grep -ho 'border-radius: *[0-9.]*[a-z%]*' apps/*/src/*.css | sort | uniq -c | sort -rn
   grep -ho 'font-size: *[0-9.]*[a-z%]*' apps/*/src/*.css | sort | uniq -c | sort -rn
   ```

2. 빈도가 높은 값을 토큰으로 승격한다. [설계] 이름 규칙을 정한다. 권장:
   ```
   --db-color-bg, --db-color-surface, --db-color-border
   --db-color-text, --db-color-text-muted
   --db-color-accent, --db-color-danger, --db-color-warn, --db-color-ok
   --db-space-1 ... --db-space-6
   --db-radius-sm | -md | -lg
   --db-font-sans, --db-font-mono
   --db-text-sm | -base | -lg
   --db-focus-ring
   ```
   `--db-` 접두사로 앱 로컬 변수와 충돌하지 않게 한다.

3. `tokens.css`는 `:root`에 값을 정의한다. 현재 10개 앱이 모두 다크 기조이므로 다크를
   기본으로 둔다. [설계] 라이트 테마 지원 여부를 정한다. **이번 PR에서는 다크만** 하고,
   나중에 `[data-theme]`로 덮어쓸 수 있게 이름만 준비한다.

4. 각 앱의 `App.css` 최상단에서 import한다.
   ```css
   @import "@devbox/tokens/tokens.css";
   ```
   Vite가 `node_modules` 경로를 해석하므로 pnpm workspace 링크로 동작한다.

5. **한 번에 한 앱씩** 하드코딩 값을 토큰으로 교체한다. 시각적 변화가 생기는 값은 토큰에
   맞추지 말고 앱 로컬 변수로 남긴다. **이 PR의 목적은 통일이지 리디자인이 아니다.**

6. `CONVENTIONS.md` §3 스타일 항목을 확정한다 (PR 1에서 "예정"으로 적은 것을 확정으로).

**체크리스트**

- [ ] `packages/tokens`에 React 컴포넌트가 없다
- [ ] 토큰 값이 기존 CSS에서 수집한 것이다 (새로 발명하지 않았다)
- [ ] 적용 대상 앱이 전부 import한다
- [ ] 앱마다 남은 로컬 값은 의도적으로 남긴 것이다
- [ ] 시각적 회귀가 없다
- [ ] `pnpm-workspace.yaml`의 `packages/*`가 새 패키지를 잡는다

**검증**

```bash
pnpm install
pnpm -r build
pnpm -r test
pnpm -r exec tsc --noEmit
```

```bash
grep -l "@devbox/tokens" apps/*/src/App.css | wc -l
```

기대: 최종(16c 완료) `10`.

```bash
# 하드코딩 색상이 줄었는지 (0일 필요는 없다)
grep -ho '#[0-9a-fA-F]\{3,8\}' apps/*/src/*.css | wc -l
```

**Windows 검증.** 적용한 앱을 `pnpm tauri dev`로 띄워 시각적 회귀가 없는지 확인한다.
CSS가 가장 큰 run-manager와 code-pad를 우선 확인한다.

**완료 조건**

- 10개 앱이 토큰을 import하고 빌드가 통과한다
- 시각적 회귀가 없다

---

### PR 17 — `chore/workspace/csp-baseline`

**목표.** 10개 앱의 `"csp": null`을 명시적 정책으로 바꾼다.

**선행.** 없음

**분할 권장.** 그룹 단위로 나눈다.

| PR | 그룹 | 앱 |
|---|---|---|
| 17a | C — 로컬 데이터만 | port-manager, developer-toolbox, everything-plus, run-manager, wsl-desktop, life-log |
| 17b | A — 외부 콘텐츠 렌더 | code-pad, knowledge-base |
| 17c | B — 외부 응답 취급 | api-playground, devbox-manager |

**변경 파일**

| 경로 | 작업 |
|---|---|
| `apps/<app>/src-tauri/tauri.conf.json` | `security.csp` 설정 |
| `docs/architecture.md` | 보안 경계 절 보강 (PR 1에서 신설한 절에 실제 정책 추가) |

**작업 순서**

1. 그룹 C부터 시작한다. 가장 단순하고 회귀 위험이 낮다.
   ```json
   "security": {
     "csp": "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost"
   }
   ```
   [설계] `style-src`의 `'unsafe-inline'`이 필요한지 앱마다 확인한다. React가 인라인
   스타일을 쓰면 필요하다. 필요 없는 앱은 빼는 것이 낫다.

   [설계] Tauri v2의 IPC가 `connect-src`에 무엇을 요구하는지 실제 실행으로 확인한다.
   문서 값을 그대로 믿지 말고 콘솔 위반 메시지를 보고 최소 집합을 정한다.

2. 그룹 A는 mermaid가 생성한 SVG를 `innerHTML`로 삽입한다
   (`PreviewPane.tsx:44`, `MarkdownPreview.tsx:57`). `securityLevel: "strict"`이므로 스크립트는
   들어가지 않지만 SVG 안의 스타일 처리를 확인해야 한다. **먼저 그룹 C 정책을 적용해 보고
   콘솔 에러를 관찰**한 뒤 필요한 최소 예외만 추가한다.

3. 그룹 B의 api-playground는 응답을 텍스트로 렌더링하므로 그룹 C와 같은 정책으로 시작한다.
   devbox-manager는 릴리스 메타데이터만 다루므로 동일하다.

4. **개발 모드를 확인한다.** Vite dev 서버는 HMR에 WebSocket을 쓴다. `pnpm tauri dev`에서
   CSP가 이를 막지 않는지 확인한다. 막힌다면 [설계] dev/prod CSP를 분리할지 `connect-src`에
   dev 오리진을 허용할지 정한다. **먼저 실제로 막히는지 확인하고 대응한다.**

5. `docs/architecture.md`의 보안 경계 절에 각 그룹의 정책과 근거를 적는다.

**체크리스트**

- [ ] 10개 앱 전부 `csp`가 `null`이 아니다
- [ ] 그룹별 정책 차이가 문서에 근거와 함께 남았다
- [ ] 각 앱을 실제로 띄워 콘솔 CSP 위반이 없는지 확인했다
- [ ] 개발 모드(`tauri dev`)에서도 동작한다
- [ ] `capabilities/default.json`은 이 PR에서 건드리지 않는다 (별도 관심사)

**검증**

```bash
grep -c '"csp": null' apps/*/src-tauri/tauri.conf.json
```

기대: 전부 `0`.

```bash
python3 - <<'PY'
import json, glob
for f in sorted(glob.glob("apps/*/src-tauri/tauri.conf.json")):
    c = json.load(open(f))["app"]["security"]["csp"]
    print(f.split("/")[1], "->", (c or "NULL")[:60])
PY
```

기대: 10줄 전부 정책 문자열.

**Windows 검증.** **필수다. 빌드 통과만으로는 검증되지 않는다.** 10개 앱을 각각
`pnpm tauri dev`로 띄우고 DevTools 콘솔에 `Content Security Policy` 위반이 없는지 확인한다.
특히:

- code-pad: 마크다운 프리뷰 + mermaid 다이어그램 렌더링
- knowledge-base: 같은 경로 + 이미지가 포함된 노트
- api-playground: 요청 전송과 응답 표시
- run-manager: 로그 tail 화면
- life-log: 트레이에서 창 열기

**완료 조건**

- `grep -c '"csp": null'`이 전부 0
- 10개 앱 모두 콘솔 CSP 위반 없이 정상 동작

**함정**

- CSP 위반은 **조용히 기능을 죽인다.** 화면이 비거나 다이어그램이 안 그려지는 식으로
  나타난다. 반드시 실제 실행으로 확인한다.
- production 빌드와 dev 빌드의 동작이 다를 수 있다. 최소한 한 앱은 `pnpm tauri build`로
  만든 실행 파일에서도 확인한다.

---

## 17.5 Stage 1 — 정확성과 privacy

여기부터 상세도가 **작업 지시 수준**이다 (§17.0). 각 PR을 시작하기 전에 `[설계]` 항목을
결정한다.

### PR 18 — `feat/everything-plus/incremental-watcher`

- **목표.** §8. root별 `notify` watcher로 증분 인덱싱.
- **선행.** 없음
- **변경 파일.** `apps/everything-plus/src-tauri/src/watcher.rs`(신규),
  `core/db.rs`(514줄) 수정, `commands/indexing.rs` 수정, `lib.rs` 상태 등록
- **먼저 읽을 것.** `apps/code-pad/src-tauri/src/watcher.rs`(609줄). 같은 문제를 이미 풀었다.
- **[설계]** debounce 창 길이 / rename을 (delete+create)로 볼지 별도 event로 볼지 /
  overflow 시 전체 root 재스캔 vs 부분 재스캔 / full re-index와 incremental writer의 배타
  제어 방식(generation 카운터 권장)
- **작업 순서**
  1. root별 watcher 생명주기 구조체 (등록·해제·복원)
  2. event → DB 반영을 `core/`의 순수 함수로 분리
  3. debounce와 배치 커밋
  4. rename: 이전 경로 FTS row 삭제 + 새 row 삽입
  5. overflow·에러 시 reconciliation scan
  6. 앱 재시작 시 watcher 복원
  7. root별 상태(마지막 반영 시각, pending 수, 오류)를 UI에 노출
- **체크리스트**
  - [ ] event 처리 로직이 `core/`에서 테스트된다
  - [ ] watcher thread에서 파일 내용을 읽지 않는다
  - [ ] root 밖으로 canonicalize되는 경로를 반영하지 않는다
  - [ ] root 제거 시 watcher와 pending이 함께 해제된다
  - [ ] symlink/junction 정책이 `crates/filesystem`과 일치한다
- **검증.** `cargo test -p everything-plus`. Windows에서 §8.5 완료 조건 4개 수동 확인

### PR 19 — `feat/everything-plus/result-actions`

- **목표.** §8.4 후속 UX.
- **선행.** PR 18
- **[설계]** Code Pad로 열기의 실행 방식. §10.3 CLI argument 규약을 따르되 Code Pad 미설치
  시 fallback을 정한다
- **체크리스트**
  - [ ] keyboard navigation (↑↓, Enter, Esc)
  - [ ] 기본 앱으로 열기 / 폴더 열기 / 경로 복사
  - [ ] 경로에 공백·비ASCII가 있어도 동작
  - [ ] shell 문자열 조합으로 경로를 넘기지 않는다
- **검증.** `pnpm --filter ./apps/everything-plus test`. Windows 수동 확인

### PR 20 — `feat/life-log/idle-detection`

- **목표.** §9.2. `GetLastInputInfo` 기반 idle 감지와 session 분리.
- **선행.** PR 4 (병합으로 수집기가 life-log에 있다)
- **변경 파일.** `apps/life-log/src-tauri/src/core/sessionizer.rs` 확장,
  Windows 전용 호출은 command 레이어에 격리
- **[설계]** idle threshold 기본값 / idle session을 별도 저장할지 통계에서 제외할지
- **체크리스트**
  - [ ] idle 판정 로직이 `core/`에서 시간 입력만으로 테스트된다
  - [ ] lock/unlock, suspend/resume 경계에서 session을 닫고 새로 연다
  - [ ] threshold를 설정에서 바꿀 수 있다
  - [ ] Windows API 호출이 `#[cfg(target_os = "windows")]`로 격리된다
- **검증.** `cargo test -p life-log`. Windows에서 잠금 → 해제 → 절전 → 복귀 시나리오

### PR 21 — `feat/life-log/privacy-rules`

- **목표.** §9.2. process 제외, title 미저장·redaction.
- **선행.** PR 20
- **[설계]** redaction 규칙 표현 방식(regex 목록 vs 단순 패턴) / 기본 제외 목록 유무 /
  기존 데이터에 소급 적용할지
- **핵심 제약.** privacy rule은 **DB insert 전에** 적용한다. UI 필터가 아니다
- **체크리스트**
  - [ ] 규칙 적용이 `core/`에서 순수 함수로 테스트된다
  - [ ] 제외 대상 원문이 DB·로그·snapshot 어디에도 없다
  - [ ] 소급 적용 여부를 사용자가 선택한다
- **검증.** `cargo test -p life-log`. Windows에서 실제 DB 파일을 열어 원문 부재 확인

### PR 22 — `feat/life-log/auto-start`

- **목표.** §9.2. 시작프로그램 등록 상태 확인과 토글.
- **선행.** PR 21
- **[설계]** 등록 방식(레지스트리 Run 키 vs 시작 폴더 바로가기). run-manager가 이미 startup
  shortcut을 다루므로 **먼저 그 구현을 확인**한다. 두 번째 소비자가 되면 추출을 검토한다
  ```bash
  grep -rn "startup\|Startup\|Run\\\\" apps/run-manager/src-tauri/src/
  ```
- **체크리스트**
  - [ ] 현재 등록 상태를 앱 안에서 확인할 수 있다
  - [ ] 등록·해제가 되돌릴 수 있다
  - [ ] run-manager와 같은 방식을 쓴다
- **검증.** Windows 필수. 등록 후 레지스트리 또는 시작 폴더를 직접 확인

### PR 23 — `feat/knowledge-base/codemirror-editor`

- **목표.** §3.3 결정. `App.tsx:283`의 `<textarea>`를 CodeMirror 6로 교체한다.
- **선행.** 없음
- **변경 파일.** `apps/knowledge-base/package.json`(의존 추가),
  `apps/knowledge-base/src/components/MarkdownEditor.tsx`(신규), `App.tsx` 수정
- **먼저 읽을 것.** `apps/code-pad/src/editor/CodeEditor.tsx`와 그 테스트. 같은 설정을
  참고하되 **이 PR에서는 복사해 쓴다.** 추출은 PR 24에서 한다
- **[설계]** 필요한 extension 집합. 최소: markdown 언어, 검색, 히스토리, 기본 keymap.
  LSP는 넣지 않는다 (knowledge-base는 노트 앱이다)
- **체크리스트**
  - [ ] 마크다운 문법 하이라이팅이 동작한다
  - [ ] 기존 저장·프리뷰 흐름이 깨지지 않는다
  - [ ] 프리뷰의 mermaid 렌더가 이전과 동일하다
  - [ ] `<textarea>` 기반 테스트가 새 구조에 맞게 갱신됐다
- **검증.** `pnpm --filter ./apps/knowledge-base test`, `build`. Windows에서 노트 편집·저장·
  프리뷰 확인

### PR 24 — `refactor/packages/editor-extraction`

- **목표.** §11.2. code-pad(첫 소비자)와 knowledge-base(두 번째 소비자)의 공통 CodeMirror
  설정을 `packages/editor`로 추출한다.
- **선행.** PR 23
- **추출 범위.** CodeMirror extension setup, 언어 감지, 공통 keymap. **테마·전체 state·
  LSP 연동은 옮기지 않는다**
- **체크리스트**
  - [ ] 두 앱이 같은 패키지를 쓴다
  - [ ] code-pad의 LSP 연동이 앱에 남아 있다
  - [ ] 두 앱의 기존 에디터 테스트가 전부 통과한다
  - [ ] 동작 변경 없음
- **검증.** `pnpm -r test`, `pnpm -r build`. Windows에서 두 앱 편집 확인

### PR 25 — `feat/knowledge-base/incremental-watcher`

- **목표.** §11.2 3번. 외부 편집이 재시작 없이 반영.
- **선행.** PR 24
- **중요.** watcher의 **세 번째 소비자**다 (code-pad, everything-plus, knowledge-base).
  이 시점에 `crates/watcher` 추출을 진지하게 검토한다. 세 구현의 요구가 실제로 같은지
  비교한 결과를 PR 설명에 남긴다
- **[설계]** 추출 여부. 요구가 다르면 추출하지 않고 **근거를 문서에 남긴다**
- **체크리스트**
  - [ ] 외부 편집이 검색·태그에 반영된다
  - [ ] frontmatter 변경이 태그 인덱스에 반영된다
  - [ ] knowledge root 밖 경로를 반영하지 않는다
  - [ ] 추출 여부 판단 근거가 기록됐다
- **검증.** `cargo test -p knowledge-base`, `pnpm --filter ./apps/knowledge-base test`

---

## 17.6 Stage 2 — 앱 간 연동

### PR 26 — `feat/run-manager/integration-snapshot`

- **목표.** §10.1. **integration 계약의 파일럿 producer.**
- **선행.** PR 2 (identifier 확정), PR 6 (앱 ID)
- **경로.** `%LOCALAPPDATA%\devbox\integration\run-manager\v1\summary.json`
- **[설계]** `data` 내용. 최소: 실행 성공·실패 수, service uptime, 마지막 실행 시각.
  **secret과 환경변수 값은 포함하지 않는다**
- **[설계]** 갱신 주기. 주기적 기록 vs 상태 변화 시 기록 vs 둘 다
- **체크리스트**
  - [ ] 임시 파일 + rename으로 원자 기록
  - [ ] `schemaVersion`, `producer`, `producerVersion`, `generatedAt` 포함
  - [ ] 디렉터리가 없으면 생성한다
  - [ ] 기록 실패가 앱 동작을 막지 않는다
  - [ ] secret·환경변수 값이 없다
- **검증.** `cargo test -p run-manager`(직렬화·경로 계산). Windows에서 실제 파일 확인

### PR 27 — `feat/life-log/versioned-source-reader`

- **목표.** §10.1 consumer 측. `set_activity_db(path)`의 잔재를 완전히 제거하고 계약으로
  외부 source를 읽는다.
- **선행.** PR 26
- **변경 파일.** `apps/life-log/src-tauri/src/core/readers/`(계약 reader 신규),
  `commands/life.rs`, 프론트 source 설정 UI
- **[설계]** schema version이 다를 때의 표시 방식
- **체크리스트**
  - [ ] 다른 앱의 DB 파일을 직접 열지 않는다
  - [ ] schema version이 다르면 명확히 표시하고 부분 결과를 낸다
  - [ ] snapshot이 없거나 오래돼도 앱이 동작한다
  - [ ] freshness(마지막 갱신 시각)를 UI에 표시한다
- **검증.** `cargo test -p life-log`, `pnpm --filter ./apps/life-log test`

### PR 28 — `feat/workspace/project-profile-schema`

- **목표.** §10.2. ProjectProfile 스키마 확정. 아직 앱은 없다.
- **선행.** PR 14 (`crates/wsl`의 `canonical_project_key`)
- **성격.** 이 PR은 사실상 설계 문서 작업이다. §1 원칙대로
  `docs/superpowers/specs/YYYY-MM-DD-project-profile-design.md`를 먼저 쓴다
- **결정할 것**
  - `id` 생성 규칙과 canonical identity (Windows/WSL 양쪽에서 등록해도 하나)
  - 저장 위치와 형식 (Workbench 단일 writer이므로 Workbench의 app data)
  - 다른 앱에 전달할 최소 context
  - 스키마 버전 정책
  - **기존 두 저장소 흡수 계획** — wsl-desktop의 localStorage `wsld-projects`,
    life-log의 SQLite `projects` 설정
- **체크리스트**
  - [ ] canonical identity 규칙이 `crates/wsl`의 함수를 사용한다
  - [ ] 단일 writer 원칙이 명시됐다
  - [ ] 스키마가 JSON Schema 또는 Rust 타입으로 문서화됐다
  - [ ] 기존 두 저장소의 마이그레이션 경로가 있다

### PR 29 — `feat/knowledge-base/integration-snapshot`

- **목표.** §10.1 **두 번째 producer.**
- **선행.** PR 26
- **중요.** 두 번째 producer가 같은 envelope를 쓰는 시점이다. **여기서
  `crates/integration`을 추출한다** (§10.1). envelope 직렬화, atomic write, 경로 계산만 옮긴다
- **[설계]** `data` 내용. note 작성·수정 수와 시각. **본문은 넣지 않는다**
- **체크리스트**
  - [ ] `crates/integration`이 추출됐다
  - [ ] run-manager가 새 크레이트를 쓰도록 함께 수정됐다
  - [ ] note 본문이 포함되지 않는다

### PR 30 — `feat/life-log/project-attribution`

- **목표.** §11.1. 활동을 ProjectProfile에 귀속.
- **선행.** PR 27, PR 28
- **[설계]** 귀속 규칙. 창 제목·경로·git root 중 무엇을 근거로 하는가 / 중복 집계 방지 규칙
- **체크리스트**
  - [ ] 미귀속 활동을 별도로 표시한다
  - [ ] 귀속 근거를 사용자가 확인할 수 있다
  - [ ] 같은 event가 두 source에서 중복 집계되지 않는다
  - [ ] source 하나가 실패해도 나머지가 표시된다

---

## 17.7 Stage 3 — 기존 앱 깊이

### PR 31 — `feat/run-manager/service-observability`

- **목표.** §13.1.
- **선행.** 없음
- **변경 파일.** `apps/run-manager/src-tauri/src/lifecycle.rs`, `scheduler.rs`, `logs.rs`,
  프론트 서비스 상세 화면
- **[설계]** DB state와 실제 process 생존을 어떻게 대조할지 / definition과 instance를 화면에서
  어떻게 분리할지
- **체크리스트**
  - [ ] definition과 runtime instance가 화면에서 구분된다
  - [ ] DB state만으로 생존을 단정하지 않는다
  - [ ] PID/WSL identity 표시가 안전하다 (재사용 위험 고려)
  - [ ] health probe 이력과 backoff 단계가 보인다

### PR 32 — `feat/run-manager/definition-export`

- **목표.** §13.2 export 절반.
- **선행.** PR 31
- **[설계]** definition JSON schema version 정책
- **체크리스트**
  - [ ] secret 값이 export에 없다 ("secret configured" 표시만)
  - [ ] schema version이 포함된다

### PR 33 — `feat/code-pad/crash-recovery`

- **목표.** §12.1 + §12.5 부품의 최초 구현.
- **선행.** 없음
- **변경 파일.** `apps/code-pad/src-tauri/src/core/session.rs` 인근에 recovery 분리,
  `apps/code-pad/src/components/ChangeSetPreview.tsx`(신규)
- **[설계]** snapshot 주기와 전체 저장량 상한 / 비정상 종료 감지 방식
- **§12.5 준수 사항**
  - preview 컴포넌트의 입력을 "경로 → (before, after)" 목록으로 일반화한다
  - 항목 단위·전체 단위 승인을 모두 지원한다
  - 실제 적용은 컴포넌트 밖에서 한다
- **체크리스트**
  - [ ] 세션 파일과 recovery 파일이 분리됐다
  - [ ] 정상 저장·닫기 시 recovery가 제거된다
  - [ ] 자동 덮어쓰기를 하지 않는다
  - [ ] preview 컴포넌트가 Code Pad 문서 모델에 결합되지 않았다
- **검증.** `pnpm --filter ./apps/code-pad test`. Windows에서 강제 종료 후 복구 확인

### PR 34 — `feat/code-pad/problems-panel`

- **목표.** §12.2 전반.
- **체크리스트**
  - [ ] 열린 문서 진단이 한곳에 모인다
  - [ ] stale diagnostics를 계속 보여주지 않는다
  - [ ] server degraded/crash 상태가 함께 표시된다

### PR 35 — `feat/code-pad/navigation-history`

- **목표.** §12.2 후반. **선행.** PR 34
- **체크리스트**
  - [ ] definition/reference 이동에 back/forward가 있다

### PR 36 — `feat/api-playground/collections`

- **목표.** §11.3 1번.
- **[설계]** collection 저장 형식과 위치 / history와의 수명 정책 분리
- **체크리스트**
  - [ ] collection과 history의 정책이 분리됐다

### PR 37 — `feat/api-playground/environments`

- **목표.** §11.3 2번. **선행.** PR 36
- **체크리스트**
  - [ ] environment 전환이 원본 request template을 변경하지 않는다
  - [ ] `{{variable}}` 치환이 순수 함수로 테스트된다

### PR 38 — `feat/api-playground/secret-variables`

- **목표.** §11.3 3~4번 + `crates/secrets` 추출.
- **선행.** PR 37
- **중요.** run-manager의 DPAPI 구현
  (`apps/run-manager/src-tauri/src/platform/environment.rs`)이 첫 소비자다. 이 PR이 두 번째
  소비자이므로 **여기서 `crates/secrets`를 추출한다** (§11.3)
- **[설계]** DPAPI entropy 사용 여부 / 저장 형식
- **주의.** `CONVENTIONS.md` §4는 crates에 Windows 전용 코드를 금지한다. DPAPI는 Windows
  전용이므로 **크레이트에는 봉인 데이터 형식·직렬화·마스킹 규칙 등 순수 부분만** 넣고
  실제 `CryptProtectData` 호출은 각 앱의 platform 레이어에 남긴다. 또는 이 크레이트만
  예외로 하고 근거를 `CONVENTIONS.md`에 적는다 — **둘 중 하나를 PR 시작 전에 정한다**
- **체크리스트**
  - [ ] `crates/secrets` 추출, run-manager도 함께 전환
  - [ ] Windows 전용 코드 경계 결정이 문서화됐다
  - [ ] secret이 history에 평문으로 남지 않는다
  - [ ] curl 복사·오류 메시지에 secret이 나타나지 않는다
- **검증.** `cargo test --workspace`. Windows에서 DPAPI 실제 동작 확인

### PR 39 — `feat/run-manager/definition-import`

- **목표.** §13.2 import 절반 + §12.5 부품 추출.
- **선행.** PR 32, PR 33
- **중요.** 변경 preview의 **두 번째 실소비자**다. 여기서 `packages/diff-view`를 추출한다
- **체크리스트**
  - [ ] `packages/diff-view` 추출, code-pad도 함께 전환
  - [ ] import preview에서 충돌을 보여준다
  - [ ] WSL distro·cwd가 없으면 disabled draft로 들어온다

---

## 17.8 Stage 4 — Workbench

새 앱이다. §1 원칙에 따라 **설계 문서를 먼저 쓴다.** 코드보다 앞선다.

### 선행 조건 (전부 충족돼야 시작)

| 조건 | 어디서 |
|---|---|
| identifier 확정 | PR 2 |
| 앱 카탈로그 | PR 6 |
| `crates/wsl` (canonical identity) | PR 14 |
| ProjectProfile 스키마 | PR 28 |
| 최소 한 개 producer snapshot | PR 26 |
| Run Manager service 상태 API | PR 31 |
| 앱 실행 context 규약 | §10.3 — Workbench 착수 시 확정 |

### 순서

1. `docs/superpowers/specs/YYYY-MM-DD-workbench-design.md` 작성
   - 앱 간 ownership 확정 (누가 쓰고 누가 읽는가)
   - `Start Workspace` 각 단계의 실패·rollback 정책
   - "이미 실행 중이던 자원"과 "Workbench가 시작한 자원"의 구분 방법
   - idempotency key 설계
2. `pnpm create tauri-app` 스캐폴드 (`CONVENTIONS.md` §6 절차 준수. `--yes` 필수, 생성 직후
   4개 파일의 `--name` 교체, identifier는 `com.devbox.workbench`)
3. `apps/catalog.json`에 등록 → PR 7 검사가 자동으로 검증한다
4. ProjectProfile CRUD + **기존 두 저장소 흡수** (wsl-desktop localStorage, life-log settings)
5. read-only project health (Git, WSL distro, expected port, Run Manager service)
6. **wsl-desktop의 `gitStatus` 기능 이관 및 wsl-desktop에서 제거** (§3.1)
7. 앱 실행 context 전달
8. `Start Workspace`
9. `Stop What I Started`
10. 부분 실패와 rollback UX (§12.5의 `packages/diff-view` 재사용 검토)

### 완료 조건

- 시작 전부터 실행 중이던 process/service를 자동 종료하지 않는다
- 다른 앱의 DB를 직접 수정하지 않는다
- 앱이 없으면 Devbox Manager 설치 화면으로 안내한다
- 부분 시작 실패 후 상태를 사용자가 이해할 수 있다
- wsl-desktop에 프로젝트 목록·git 상태 코드가 남아 있지 않다

---

## 17.9 Stage 5 — 다음 독립 앱

1. **Webhook Lab** (§15.3) — 설계 문서 → 스캐폴드 → 카탈로그 등록 → MVP
2. **Dev Environment Doctor** (§15.4) — 먼저 Devbox Manager의 "환경 진단" 탭으로 구현하고,
   독립 실행 수요가 확인된 뒤에만 앱으로 승격
3. **Log Lens 또는 Repo Manager** (§15.5/§15.6) 중 실제 사용 수요가 높은 하나

각 앱 착수 전 확인:

- [ ] 기존 10개 앱의 P0·P0.5가 전부 끝났다
- [ ] 최소 두 개 producer의 integration snapshot이 실제로 소비되고 있다
- [ ] 새 앱이 기존 앱의 책임을 복제하지 않는다 (§16 대조)
- [ ] §2.4의 스택 추가 기준을 만족하지 않는 한 Tauri v2 + Rust + React로 만든다

---

## 18. 검증 전략

### 공통 완료 조건

- 순수 Rust 로직: `cargo test`
- Rust workspace: `cargo check`, `cargo clippy`, `cargo fmt --check`
- 프론트: `pnpm test`, `pnpm build`, `tsc --noEmit`
- 카탈로그 정합: `bash .github/scripts/check-catalog.sh` (PR 7 이후)
- Windows 전용 코드: Windows CI compile/test
- 실제 PTY, Job Object, DPAPI, updater, startup, foreground window, CSP: Windows smoke test

### 위험 기능별 필수 검증

| 기능 | 검증 항목 |
|---|---|
| identifier 변경 | 구 데이터 이전, 두 번 실행, 신규 설치, 파일 잠금 |
| 앱 병합 | 데이터 흡수 중복 방지, 기능 손실, 테스트 이동 |
| release | catalog 누락, stale artifact, 부분 matrix 실패, manifest 불일치 |
| updater | redirect, size/digest mismatch, 중단 복구, rollback |
| process 종료 | PID 재사용, descendant 종료 확인, timeout |
| WSL 종료 | marker/PID/PGID/SID 재검증, SIGTERM→SIGKILL |
| watcher | rename, delete, overflow, root 제거, junction/symlink |
| secret | DB, log, UI, clipboard, export redaction |
| LSP multi-file edit | stale hash, 겹치는 edit, workspace 밖 path, 부분 적용 방지 |
| integration snapshot | schema mismatch, partial write, stale producer |
| CSP | 각 앱 실제 실행 시 콘솔 위반 0, 기능 회귀 없음, prod 빌드 확인 |
| Workbench | 부분 시작 실패, 기존 실행 자원 보존, 중복 시작 |

### 리팩터링·병합 PR의 검증 원칙

PR 3·4(병합), PR 14·15·24·39(추출)는 **동작이 변하지 않았음**을 증명해야 한다.

1. 기존 테스트를 **먼저** 새 위치로 옮긴다 (구현보다 먼저)
2. 원본 함수와 그 테스트를 **삭제**한다 (남기면 두 벌이 된다)
3. 중복이 사라졌음을 `grep`으로 확인한다
4. **기능 추가를 같은 PR에 넣지 않는다**
5. 두 구현이 다를 때는 diff를 확인하고 채택 근거를 커밋 메시지에 남긴다

### 데이터 마이그레이션 PR의 검증 원칙

PR 2(identifier), PR 4(DB 흡수)는 사용자 데이터를 옮긴다. 다음을 **반드시 Windows에서**
확인한다.

1. 구 데이터가 있는 상태에서 실행 → 이전된다
2. **두 번 실행 → 중복되지 않는다** (마커가 동작하는지)
3. 구 데이터가 없는 상태에서 실행 → 정상 시작
4. 이전 실패 시 앱이 시작은 된다 (막히지 않는다)
5. 원본을 삭제하는지 보존하는지가 의도와 일치한다

### 로컬 제품 지표

원격 telemetry 없이도 다음 값은 진단 화면에서 품질 판단에 활용할 수 있다.

- Devbox Manager: 검증 실패, update rollback, catalog 불일치
- Everything+: watcher 반영 지연, reconciliation 횟수, 검색 p95
- Life Log: idle 분리 시간, privacy rule 적용 수, source freshness, 미귀속 활동 비율
- Code Pad: recovery 발생, stale LSP edit 거부, server timeout
- Run Manager: 실행 성공률, cancel 확인 시간, restart/backoff 상태
- Workbench: 단계별 시작 시간, 부분 실패, rollback 결과

이 값은 기본적으로 로컬에만 유지하고 외부 전송은 하지 않는다.

## 19. 외부 도구와의 비교 근거

다음 공식 자료는 기능 범위·전문 도구 handoff·설치 크기 판단을 위한 비교 근거다. 목록에
도구가 있다는 사실 자체는 devbox 기능의 제외 근거가 아니며, native-first 정책과 반복 작업
감축 효과를 먼저 평가한다.

- [PowerToys Command Palette](https://learn.microsoft.com/windows/powertoys/command-palette/overview)
- [PowerToys Run](https://learn.microsoft.com/windows/powertoys/run)
- [PowerToys Hosts File Editor](https://learn.microsoft.com/windows/powertoys/hosts-file-editor)
- [PowerToys utility 목록](https://learn.microsoft.com/windows/powertoys/grouppolicy)
- [DevToys](https://github.com/DevToys-app/DevToys)
- [Windows Terminal](https://learn.microsoft.com/windows/terminal/)
- [Windows Terminal layout 복원](https://learn.microsoft.com/windows/terminal/customize-settings/startup)
- [Docker Desktop](https://docs.docker.com/desktop/use-desktop/)
- [Docker Desktop Logs](https://docs.docker.com/desktop/use-desktop/logs/)
- [VS Code Tasks](https://code.visualstudio.com/docs/debugtest/tasks)
- [Tauri v2 sidecar (externalBin)](https://v2.tauri.app/develop/sidecar/)
- [Tauri v2 CSP](https://v2.tauri.app/security/csp/)

## 20. 최종 추천

```text
결정 고정 (스택·병합·네이밍)
  → 통폐합 12 → 10, identifier com.devbox.*
  → 버전 단일 원본 + 카탈로그 + manifest
  → Devbox Manager 배포 신뢰성
  → 공용 프리미티브 (crates/wsl, crates/search, 토큰, CSP)
  → 검색 실시간성 + 활동 privacy + Knowledge 편집기
  → devbox 공용 루트 integration + ProjectProfile
  → Life Log 프로젝트 집계
  → 기존 앱 관찰성·복구성
  → Workbench
  → Webhook Lab / Dev Environment Doctor
```

새 앱을 추가하기 전에 세 가지가 갖춰져야 한다.

1. 기존 10개 앱이 release manifest와 Manager에서 정확히 표현된다.
2. 최소 한 개의 versioned integration 흐름이 실제로 검증된다.
3. **공용 프리미티브가 존재한다.** `crates/wsl`의 canonical identity 없이 만든
   ProjectProfile은 같은 프로젝트를 서로 다르게 인식하고, 그 위에 선 Workbench는 기반이
   아니라 부채가 된다.

그리고 이번 개정이 확인한 것 하나를 덧붙인다.

> 10개 앱을 대조하며 발굴한 결핍 — 버전 원본 3벌, WSL 로직 중복, FTS 쿼리 빌더 2벌,
> 프로젝트 목록 2벌, CSS 10벌, CSP 10벌 — 은 하나도 "앱이 부족해서" 생기지 않았다.
> 전부 **앱 간 계약이 없어서** 생겼다. 새 언어나 새 UI 스택은 이 목록을 두 벌로 만든다.

이 기반이 갖춰지면 Workbench는 기능을 복제하는 11번째 앱이 아니라 기존 앱의 가치를 묶어
증폭하는 제품이 될 수 있다.

### 20.1 2026-08-22 현재 결론

위 전제는 v0.4.0~v0.4.1에서 충족됐다. 현재 저장소에는 13개 앱, release catalog/manifest,
`crates/wsl`·`search`·`integration`·`applink`·`launch`, ProjectProfile과 실제 inbound 계약이
있다. 다음 단계는 다시 기반만 만드는 것이 아니라 그 기반으로 사용자의 앱 전환과 수동
데이터 운반을 실제로 줄이는 것이다.

```text
native-first 지침 + API secret 안전성
  → catalog capability + snapshot 정리 + one-time handoff
  → 전 앱 context menu + WSL native workspace
  → 기존 13개 앱의 P1/P2 기능 강화
  → Devbox Launcher + Log Lens
  → 선택 P3 보강 + 제한된 Related Tools
  → offline Windows RC + v0.5.0
```

전체 기능·제외·상한·버전·검증 조건은
[`2026-08-22-v0.5.0-native-first-plan.md`](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)가
단일 원본이다.
