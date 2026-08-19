# 앱 간 연동 — 인바운드 계약과 생태계 확장 설계

- 상태: v0.4.1 범위(§1의 Path/Workspace 라우팅·§3·§5.1) 구현 반영 (Windows 실기 검증 대기); v0.5.0 범위(§1의 Profile/Query 라우팅·§2·§4·§5.2) 제안·보류
- 작성일: 2026-08-17
- 범위: 저장소 전체 — 신규 `crates/applink`, `crates/launch`, `crates/integration`, `apps/catalog.json`, 13개 앱
- 관련: [UX 개선 설계](./2026-08-15-ux-improvements-design.md) §4.2, [wsl-desktop 터미널 설계](./2026-08-17-wsl-desktop-terminal-design.md) §4.4
- 근거: `docs/product-opportunities.md` §10.1(versioned read-only snapshot), §12.4(앱 간 연결)

## 0. 배경 — 링크가 작성돼 있으나 작동하지 않는다

repo-manager와 workbench는 다른 앱을 실행하며 인자를 넘긴다.

```text
// v0.4.1 release-gate target mapping
repo-manager -> code-pad:    OpenTarget::Workspace { path }
repo-manager -> wsl-desktop: OpenTarget::Path { path }
repo-manager -> workbench:   OpenTarget::Path { path }

workbench -> wsl-desktop:    OpenTarget::Path { path } // concrete WSL path, then Windows path
workbench -> code-pad:       OpenTarget::Workspace { path } // Windows workspace path
```

`OpenTarget::Profile`과 그에 따른 터미널 레이아웃 선택은 v0.5.0 범위다(§4.4). v0.4.1은
프로필 id를 wsl-desktop에 보내지 않고, 실제로 열 수 있는 구체적인 `Path`만 보낸다.
v0.4.1의 `Path` 타깃에는 distro 정보가 없으므로 WSL Desktop은 앱에서 선택된 distro를 사용하고,
선택값이 없으면 기본 distro를 사용한다. 특정 프로필의 distro와 레이아웃을 복원하는 동작은 v0.5.0으로 미룬다.

위 매핑 이전의 v0.4.0 구현에는 다음 문제가 있었다. **argv를 읽는 앱이 하나도 없었다.** 저장소 전체에서 `std::env::args` 소비처는
run-manager의 `--background` 하나뿐이다(`apps/run-manager/src-tauri/src/lib.rs:108`).
code-pad·wsl-desktop·workbench의 `main.rs`/`lib.rs`에는 인자 처리가 **전혀 없다.**

> v0.4.0 당시 repo-manager에서 "CodePad" 버튼을 누르면 **빈 Code Pad가 열렸다.**
> Workbench의 "Start Workspace"는 wsl-desktop과 code-pad를 띄웠지만
> **둘 다 아무 프로젝트도 모르는 상태로** 떴다.

### 0.1 v0.4.0 기준 현재 연동 실태 (역사적 기준선)

다음 표와 설명은 v0.4.1 수정 전의 v0.4.0 상태를 기록한 역사적 관찰이며, 현재 구현 상태를
뜻하지 않는다. 현재 릴리스 범위는 §5.1, 이후 기능은 §5.2의 v0.5.0 범위를 따른다.

```
repo-manager   ──launch(path)─────────► code-pad       ✗ 인자 무시
repo-manager   ──launch(path)─────────► wsl-desktop    ✗ 인자 무시
repo-manager   ──launch(path)─────────► workbench      ✗ 인자 무시
workbench      ──launch(--profile)────► wsl-desktop    ✗ 인자 무시
workbench      ──launch(--workspace)──► code-pad       ✗ 인자 무시
workbench      ──snapshot v1──────────► run-manager    ✓
life-log       ──snapshot v1──────────► run-manager    ✓
workbench      ──data.db 직접 읽기────► life-log       ⚠ 자체 정책 위반
knowledge-base ──write snapshot v1────► (소비자 없음)   ⚠ 고아 producer
```

- **완전 고립 5개 앱**: api-playground, developer-toolbox, everything-plus, port-manager, webhook-lab
- workbench가 life-log의 SQLite를 직접 연다(`workspace.rs:424-457`) —
  `docs/architecture.md:62-66`이 금지한 바로 그 행위이고, `docs/architecture.md:50`의
  "외부 DB 직접 조회 없음"이라는 서술과도 어긋난다
- knowledge-base는 아무도 읽지 않는 스냅샷을 쓴다
- life-log는 `crates/integration`을 쓰지 않고 `core/readers.rs`에 같은 계약을 중복 구현했다

### 0.2 빠진 것은 "인바운드 계약"이다

메커니즘 재고를 정리하면:

| 있는 것 | 크레이트 | 방향 |
|---|---|---|
| 앱을 실행한다 | `crates/launch` | 아웃바운드 |
| 내 상태를 남이 읽게 한다 | `crates/integration` | 아웃바운드 |
| **이미 떠 있는 앱에게 "이 상태로 가라"고 말한다** | **없다** | **인바운드** |

이 문서는 그 인바운드 계약을 정의한다.

### 0.3 설계 기준 — 생태계 확장

**14번째 앱을 추가할 때 기존 13개 앱을 한 줄도 고치지 않아도 연결되어야 한다.**

이 기준이 아래 세 결정을 낳는다:

1. argv 계약은 **타입이 있고 전방 호환**이어야 한다 (§1)
2. "어떤 앱이 무엇을 받는가"는 **카탈로그가 선언**하고 UI가 그것에서 생성돼야 한다 (§2)
3. 스냅샷 소비자는 producer id를 하드코딩하지 말고 **발견**해야 한다 (§4)

---

## 1. `crates/applink` — 인바운드 계약

### 1.1 왜 새 크레이트인가

`crates/launch`에 넣지 않는다. 수신 앱 13개가 "설치 경로를 해석하는 코드"에 의존하게 되기
때문이다. 계층을 나눈다:

```
crates/applink   계약만 (타입 + parse + build)   ← 수신 앱 13개가 의존
crates/launch    설치 해석 + 실행                ← applink 에 의존, 발신 앱만 의존
```

### 1.2 타입

```rust
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenTarget {
    Path { path: String, line: Option<u32>, column: Option<u32> },
    Profile { id: String },
    Workspace { path: String },
    Query { text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub target: OpenTarget,
    pub from: Option<String>,   // 보낸 앱의 카탈로그 id — 로깅·되돌아가기용
}

pub fn parse_argv(args: &[String]) -> Result<Option<OpenRequest>, ParseError>;
pub fn build_argv(req: &OpenRequest) -> Vec<String>;
```

플래그:

| 플래그 | 타깃 |
|---|---|
| `--path <p>` `[--line N]` `[--column N]` | `Path` |
| `--profile <id>` | `Profile` |
| `--workspace <p>` | `Workspace` |
| `--query <text>` | `Query` |
| `--from <app-id>` | 공통 |
| (맨 앞 위치 인자) | `Path` — 하위 호환 |

### 1.3 전방 호환 규칙 — 생태계 확장의 핵심

새 타깃 종류를 추가했을 때 **이미 설치된 구버전 앱**이 그것을 받는 상황이 반드시 생긴다.
(devbox-manager는 앱을 개별적으로 업데이트하므로 버전이 섞인다.)

| 상황 | 처리 |
|---|---|
| 모르는 플래그 | **무시한다.** 오류가 아니다 |
| 알려진 타깃 플래그가 하나도 없음 | `Ok(None)` → 앱은 평소대로 뜬다 |
| 알려진 플래그인데 값이 없거나 형식 오류 | `Err(ParseError)` → 로그만 남기고 평소대로 뜬다 |
| 맨 앞 위치 인자 | `Path`로 해석 |

즉 **새 발신자가 구버전 수신자에게 `--query`를 보내면 오늘과 동일한 동작(빈 앱)으로
degrade한다.** 크래시하거나 오류 대화상자를 띄우지 않는다. 새 기능이 구버전을 깨뜨리지
않는다는 보장이 있어야 카탈로그에 앱을 자유롭게 추가할 수 있다.

위치 인자를 `Path`로 받는 이유: 이미 배포된 repo-manager 빌드가 그 형태로 보낸다
(`commands.rs:151`). 사용자가 repo-manager를 업데이트하지 않아도 링크가 살아난다.

### 1.4 각 앱의 수신 라우팅

| 앱 | 받는 타깃 | 동작 |
|---|---|---|
| code-pad | `Path`(+line/column), `Workspace` (v0.4.1) | 파일 열기 / 워크스페이스 열기 |
| wsl-desktop | `Path` (v0.4.1), `Profile` (v0.5.0) | 그 경로에서 터미널 / 프로필 레이아웃 (터미널 설계 §4.4) |
| workbench | `Path` | 해당 프로필 선택, 없으면 생성 초안 |
| knowledge-base | `Path`, `Query` | 노트 열기 / 검색 |
| everything-plus | `Query` | 검색어로 시작 |
| repo-manager | `Path` | 해당 저장소 선택 |

이 표는 §2의 `accepts` 선언과 1:1 대응한다.

---

## 2. 카탈로그 capability — 메뉴를 선언에서 생성한다

### 2.1 문제

repo-manager는 대상 앱을 **하드코딩**한다:

```rust
// apps/repo-manager/src-tauri/src/commands.rs:148
if !matches!(app_id.as_str(), "code-pad" | "wsl-desktop" | "workbench") {
    return Err("알 수 없는 앱".into());
}
```

그리고 UI의 버튼 세 개도 하드코딩이다(`apps/repo-manager/src/App.tsx:105-107`).
14번째 앱이 "경로를 받을 수 있다"고 해도, repo-manager를 고치지 않으면 나타나지 않는다.
같은 문제가 "다른 앱으로 열기"를 넣고 싶은 모든 앱에서 반복된다.

### 2.2 선언

`apps/catalog.json`의 각 항목에 추가한다:

```json
{
  "id": "code-pad",
  "productName": "Code Pad",
  "identifier": "com.devbox.codepad",
  ...
  "accepts": ["path", "workspace"],
  "produces": ["code-pad/v1"]
}
```

이것이 만드는 것:

- repo-manager의 하드코딩 allowlist가 사라진다 — `accepts`에 `path`가 있는지 보면 된다
- **"다른 앱으로 열기" 메뉴가 카탈로그에서 생성된다.** 어떤 앱이든 "나는 경로를 가지고
  있다"만 선언하면, `accepts`에 `path`가 있고 **실제로 설치된** 앱들이 메뉴에 자동으로
  나타난다
- **14번째 앱은 `catalog.json` 항목 하나로 12개 앱의 메뉴에 등장한다**

### 2.3 런타임 카탈로그 배포

현재 `catalog.json`은 devbox-manager만 **빌드 타임에** `include_str!`로 갖는다
(`manager.rs:19`, `doctor.rs:11`). 게다가 `apps/devbox-manager/src/api.ts:6-18`이 13개 항목
전체를 TS로 손수 중복해 두고 있다. 다른 앱은 접근 경로가 아예 없다.

설계:

1. devbox-manager가 설치/업데이트마다 `<common_root>/catalog.json`
   (`%LOCALAPPDATA%\devbox\catalog.json`)에 **런타임 사본을 원자적으로 쓴다.**
   `crates/integration::write_atomic`의 tmp+rename 패턴을 재사용한다
2. `crates/applink`가 조회 API를 제공한다:
   ```rust
   pub struct AppRef { pub id: String, pub display_name: String, pub accepts: Vec<String> }

   /// 이 타깃 종류를 받고, 실제로 설치돼 있는 앱들
   pub fn installed_targets(kind: &str) -> Vec<AppRef>;
   ```
   런타임 카탈로그를 읽고 `crates/launch::resolve_installed`로 실제 설치된 것만 남긴다
3. **폴백**: 런타임 사본이 없으면(단독 설치, Manager 미실행) 각 앱이 빌드 타임
   `include_str!` 사본을 바닥값으로 쓴다. 런타임 사본이 있으면 그쪽이 이긴다
4. `api.ts:6-18`의 손수 중복은 제거하고 카탈로그에서 파생시킨다

폴백이 있어야 하는 이유: 앱은 devbox-manager 없이 단독 설치될 수 있다. 그 경우 자기 자신은
알지만 다른 앱의 설치 여부는 모른다 — `resolve_installed`가 `None`을 반환하므로 메뉴가
비어 있을 뿐, 오류는 아니다.

### 2.4 UI 생성

컨텍스트 메뉴(UX 개선 설계 §1)의 "다른 앱으로 열기" 섹션만 이것으로 생성한다.
**나머지 항목은 100% 각 앱 소유다.**

```
┌─────────────────────────┐
│ 경로 복사               │  ← 앱 고유
│ 탐색기에서 열기         │  ← 앱 고유
├─────────────────────────┤
│ 다른 앱으로 열기     ▸  │  ← 카탈로그에서 생성 (공통)
│   Code Pad              │
│   WSL Desktop           │
│   Workbench             │
├─────────────────────────┤
│ 삭제                    │  ← 앱 고유 (danger)
└─────────────────────────┘
```

---

## 3. 이미 떠 있는 인스턴스 — single-instance 포워딩

앱이 이미 실행 중일 때 다시 실행하면 두 번째 창이 뜬다. 저장소에 선례가 있다 —
run-manager(`lib.rs:52-63`)와 life-log(`lib.rs:39-41`)가 `tauri-plugin-single-instance`를
쓴다. 다만 둘 다 인자를 크로스앱 라우팅에 쓰지 않는다.

```rust
.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
    match devbox_applink::parse_argv(&args) {
        Ok(Some(req)) => {
            app.state::<PendingOpen>().set(req.clone());
            let _ = app.emit("devbox://open", req);
        }
        Ok(None) => {}                       // 타깃 없음 — 창만 띄운다
        Err(e) => eprintln!("applink: {e}"),  // 형식 오류 — 무시하고 창만 띄운다
    }
    // 창 복원 + 포커스 (최소화돼 있을 수 있다)
}))
```

**콜드 스타트도 같은 코드 경로를 쓴다.** `setup`에서 `std::env::args()`를 파싱해
`PendingOpen` 관리 상태에 넣고, 프론트가 마운트 시 `take_pending_open` 커맨드로 가져간다.
이벤트를 emit하면 프론트가 아직 리스너를 걸지 않아 놓칠 수 있으므로 **pull 방식**으로
경합을 없앤다. (wsl-desktop의 `attach_session`이 같은 문제를 같은 방식으로 푼다 —
터미널 설계 §2.6.)

| 상황 | 동작 |
|---|---|
| 앱이 꺼져 있음 | 콜드 스타트 → `PendingOpen` → 프론트가 마운트 시 pull |
| 앱이 떠 있음 | single-instance 핸들러 → 이벤트 + `PendingOpen` → 창 포커스 |
| 앱이 떠 있고 타깃이 유효하지 않음 (경로 없음 등) | 창은 포커스하고, 프론트가 사용자에게 사유 표시. 조용히 무시하지 않는다 |
| 앱이 떠 있고 타깃 없음 | 창만 포커스 |

---

## 4. 스냅샷 버스 — 정리와 자동 발견

### 4.1 정리

| 작업 | 위치 | 효과 |
|---|---|---|
| life-log가 `projects` 스냅샷을 발행, workbench가 consumer로 | `workspace.rs:424-457`의 직접 SQLite 읽기 삭제 | `docs/architecture.md:62-66` 정책 위반 해소 |
| life-log가 knowledge-base 스냅샷의 consumer가 됨 | `product-opportunities.md §11.1`에 이미 "Knowledge의 note 작성·수정 수"로 계획돼 있음 | 고아 producer 해소 |
| life-log의 중복 구현 삭제 | `apps/life-log/src-tauri/src/core/readers.rs` 제거 → `crates/integration` 사용 | 계약 단일화 |
| wsl-desktop을 producer로 | Docker 컨테이너 + 열린 터미널 발행 | **UX 개선 설계 §4.3의 "workbench가 WSL Desktop의 Docker/포트를 자동 반영"은 이것 없이는 구현 불가.** wsl-desktop은 현재 `write_snapshot`을 전혀 호출하지 않는다 |

wsl-desktop producer 구현 시 `core/parsers.rs`의 Docker 포트 파싱이 구조화된 호스트 포트를
내도록 함께 수정한다 — 현재 `ContainerInfo.ports`는 파싱되지 않은 원시 문자열이다
(`core/models.rs:18`).

### 4.2 자동 발견

지금은 consumer가 producer id를 하드코딩한다. 새 producer가 생겨도 아무도 모른다.

```rust
// crates/integration
pub struct SnapshotRef {
    pub producer: String,
    pub version: u32,
    pub generated_at: String,
    pub path: PathBuf,
}

/// <common_root>/integration/*/v*/summary.json 을 스캔한다.
pub fn discover() -> Vec<SnapshotRef>;
```

life-log의 "Data sources" 패널(`apps/life-log/src/App.tsx:233-235`)이 하드코딩 목록 대신
발견된 모든 producer를 표시한다 — **새 앱이 producer가 되면 자동으로 등장한다.**
기존 `SourceStatus`(available / schemaVersion / producerVersion / generatedAt / freshnessMs /
error)는 그대로 쓰고, 목록만 발견 결과로 채운다.

`read_snapshot`은 이미 파일 없음을 `Ok(None)`으로, producer/schema 불일치를 `Err`로
처리한다(`crates/integration/src/lib.rs:76-97`). 발견 기반으로 바뀌어도 이 계약은 그대로다.

---

## 5. 릴리스 분리

### 5.1 v0.4.1 (핫픽스) — 끊긴 링크만 되살린다

포함:
- `crates/applink` 신설 (§1.2, §1.3)
- **이미 인자를 받고 있는 3개 앱만** 수신 구현 — code-pad, wsl-desktop, workbench
- 그 3개 앱에 `tauri-plugin-single-instance` + `PendingOpen` (§3)
- `crates/launch`가 `build_argv`를 쓰도록 변경

v0.4.1 발신 타깃은 다음과 같이 고정한다.

- repo-manager → Code Pad: `OpenTarget::Workspace { path }`
- repo-manager → WSL Desktop/Workbench: `OpenTarget::Path { path }`
- Workbench → WSL Desktop: `OpenTarget::Path { path }` (구체적인 WSL 경로 우선, Windows 경로 폴백)
- Workbench → Code Pad: `OpenTarget::Workspace { path }` (비어 있지 않은 Windows 경로만)

제외:
- 카탈로그 capability 스키마 (§2)
- 런타임 카탈로그 배포 (§2.3)
- 스냅샷 정리·자동 발견 (§4)
- 나머지 10개 앱의 수신

**컷 라인 근거**: `--path`·`--workspace`는 v0.4.1에서 실제로 보낼 수 있는 타깃이다.
Workbench → WSL Desktop은 프로필 id가 아니라 구체적인 `Path`를 보낸다. 수신을 붙이는 것은 새 기능이 아니라
**출시된 기능의 완성**이다. 반면 카탈로그 capability와
스냅샷 정리는 새 표면이므로 핫픽스에 넣지 않는다.

`--profile`을 통한 프로필 선택과 터미널 레이아웃 동작은 v0.5.0 §4.4로 명시적으로 미룬다.

repo-manager의 하드코딩 allowlist(`commands.rs:148`)는 §2의 카탈로그로 대체되므로
핫픽스에서는 손대지 않는다.

### 5.2 v0.5.0

§2 전체 → §4 전체 → 나머지 앱 수신. 순서 근거: 카탈로그가 있어야 컨텍스트 메뉴의
"다른 앱으로 열기"가 생성되고(UX 개선 설계 §1), 그것이 나머지 앱 수신의 소비처다.

다음 수동 검증은 v0.5.0 범위이며 v0.4.1 릴리스 게이트가 아니다.

- 카탈로그에 가짜 14번째 앱 항목을 추가했을 때 다른 앱의 "열기" 메뉴에 자동 등장하고,
  설치돼 있지 않으면 보이지 않는지 확인한다.
- workbench가 life-log의 SQLite를 직접 열지 않고 스냅샷 계약을 사용하는지 확인한다
  (`grep -rn "lifelog" apps/workbench`).

---

## 6. 테스트 계획

| 대상 | 방법 |
|---|---|
| `parse_argv` | 알려진 플래그 전부. **모르는 플래그 무시**(전방 호환 회귀 방지 — 명시적으로 단언). 타깃 없음 → `Ok(None)`. 값 누락 → `Err`. 맨 앞 위치 인자 → `Path` |
| `build_argv` ↔ `parse_argv` | 왕복 테스트. 모든 `OpenTarget` 변형 |
| 카탈로그 파싱 | `accepts`가 없는 구버전 항목 → 빈 목록으로 폴백(에러 아님) |
| `installed_targets` | 설치되지 않은 앱은 제외되는지. 런타임 사본 우선, 없으면 빌드타임 폴백 |
| `discover()` | 스냅샷 0개/1개/N개. 손상된 JSON은 건너뛰고 나머지를 반환하는지 |
| single-instance | 콜드 스타트와 두 번째 인스턴스가 **같은 `PendingOpen` 경로**를 쓰는지 |

**실기 검증** (Windows):
- repo-manager에서 "CodePad" → Code Pad가 **그 저장소를 열고** 뜬다
- Code Pad를 띄워둔 상태에서 다시 "CodePad" → 새 인스턴스가 아니라 **기존 창이 포커스되며
  같은 workspace/저장소가 열린다**
- Workbench "Start Workspace" → wsl-desktop이 프로필의 **구체적인 경로**에서 뜬다

---

## 7. 구현 순서

| # | 작업 | 릴리스 |
|---|---|---|
| 1 | `crates/applink` 타입 + `parse_argv`/`build_argv` + 테스트 | v0.4.1 |
| 2 | code-pad·wsl-desktop·workbench 수신 + single-instance | v0.4.1 |
| 3 | `crates/launch`가 `build_argv` 사용 | v0.4.1 |
| 4 | `catalog.json` `accepts`/`produces` + 파서 | v0.5.0 |
| 5 | 런타임 카탈로그 배포 + `installed_targets` + `api.ts` 중복 제거 | v0.5.0 |
| 6 | repo-manager allowlist 제거 → 카탈로그 기반 | v0.5.0 |
| 7 | life-log `projects` producer → workbench 직접 DB 읽기 삭제 | v0.5.0 |
| 8 | life-log가 knowledge-base consumer + `core/readers.rs` 삭제 | v0.5.0 |
| 9 | wsl-desktop producer (Docker/터미널) + 포트 구조화 | v0.5.0 |
| 10 | `discover()` + life-log Data sources 패널 | v0.5.0 |
| 11 | 나머지 앱 수신 라우팅 (§1.4) | v0.5.0 |

1과 2는 한 PR로 묶는다 — 계약과 첫 소비자를 분리하면 검증이 안 된다.
