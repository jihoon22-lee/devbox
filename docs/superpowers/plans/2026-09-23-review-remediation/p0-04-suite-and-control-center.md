# P0-04 Suite 연결·Control Center 결함 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 네 가지 결함을 고친다. (B2) 업데이트 다운로드 캐시가 쌓이다가 업데이트를 막는 문제, (B3·D14) 업데이트 때마다 제품 연결이 풀리고 매번 수동 승인해야 하는 문제 — 같은 설치의 제품은 기본 자동 연결, (B4) Control Center '단축키' 메뉴의 개발용 placeholder, (B5) 조기 거부 오류가 항상 "작업 상태를 확인할 수 없습니다"로 보이는 문제.

**Architecture:**
- 캐시 정리는 이식 가능한 순수 파일 함수(`core/update_cache.rs`)로 만들고 Windows 전용 `updates.rs`가 호출한다.
- 연결 선호는 v2 파일(`suite-connection-v2.json`, `{schema:2, product, installation, mode:"auto"|"off"}`)로 바꾼다. generation을 저장하지 않으므로 업데이트 뒤에도 유지되고, 파일이 없으면 자동 연결한다. 판단 로직은 Linux에서 테스트할 수 있게 `src/preference.rs`에 둔다.
- Control Center의 화면 조합(`Content`)을 파일로 분리해 테스트가 실제 조합을 렌더하게 한다.
- 조기 거부 문구는 프런트 `problemMessage`에서 해결한다(host 세 곳을 건드리지 않음).

**Tech Stack:** Rust, React 19 + TypeScript, Vitest

**Spec:** `review.md` §3 B2·B3·B4·B5 · `00-roadmap.md` D14

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 피어 식별(파이프 서버·클라이언트의 실행 파일 확인)은 바꾸지 않는다. 자동 연결은 "승인 단계"만 생략한다.
- 수동 연결(미리보기 → 승인) 경로는 유지한다(자동 연결을 끈 뒤 다시 켜는 용도).

## Review Focus

1. 업데이트 후 첫 실행 → 제품 연결이 자동으로 켜진다(업데이트 전에 연결을 끄지 않았다면). (Task 2 테스트)
2. 사용자가 연결을 끈 뒤 재시작 → 계속 꺼져 있다. 다른 설치 폴더로 옮긴 뒤에는 다시 자동이다. (Task 2)
3. 개발 빌드·portable ZIP → 연결을 시도하지 않고 화면에 이유를 보여준다. (Task 3)
4. 업데이트 설치 파일이 실행 중이라 이전 캐시를 지우지 못함 → 오류 없이 넘어가고 다음 확인 때 다시 지운다. (Task 1)
5. Control Center의 모든 메뉴 → 개발용 placeholder 문구가 나오지 않는다. (Task 4)

## Branch · PR

- 묶음: **B1** — 브랜치 `fix/suite/bootstrap-privacy-autosave-connection`, PR 제목 `fix(suite): pin toolchains and fix activity privacy, Knowledge autosave and Suite connections`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `fix(suite): keep products connected across updates and fix Control Center shortcuts view`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `apps/devbox-control-center/src-tauri/src/core/update_cache.rs` | 생성 | 캐시 정리 |
| `apps/devbox-control-center/src-tauri/src/core/mod.rs` | 수정 | `pub mod update_cache;` |
| `apps/devbox-control-center/src-tauri/src/updates.rs:232-262` | 수정 | `directory()` 교체 |
| `apps/devbox-control-center/src/Updates.tsx` | 수정 | 쓰이지 않는 문구 제거 |
| `crates/suite-runtime/src/preference.rs` | 생성 | 선호 v2와 자동 연결 판단 |
| `crates/suite-runtime/src/lib.rs` | 수정 | 모듈 선언, `Link`, `resume`, Status/Approve/Disconnect |
| `crates/suite-runtime/src/platform/connection_preference.rs` | 수정 | v2 파일 읽기·쓰기 |
| `packages/product-shell/src/SuiteConnection.tsx` | 수정 | 자동 연결 UI |
| `packages/product-shell/src/SuiteConnection.test.tsx` | 생성 | UI 테스트 |
| `packages/product-shell/package.json` | 수정 | `./shortcut-settings` export |
| `packages/product-shell/src/RouteView.tsx` | 수정 | placeholder 문구 중립화 |
| `apps/devbox-control-center/src/Content.tsx` | 생성 | `main.tsx`의 `Content` 이동 + shortcuts route |
| `apps/devbox-control-center/src/main.tsx` | 수정 | `Content` import |
| `apps/devbox-control-center/src/App.test.tsx` | 수정 | 실제 조합 렌더 |
| `packages/product-shell/src/operation.ts`, `operation.test.ts` | 수정 | 조기 거부 문구 |

---

### Task 1: 업데이트 캐시 정리 (B2)

**Interfaces (Produces):** `core::update_cache::{prune_releases(cache: &Path, keep: &str) -> io::Result<usize>, prune_partials(release: &Path) -> io::Result<usize>}`

- [ ] **Step 1: 실패하는 테스트** — `apps/devbox-control-center/src-tauri/src/core/update_cache.rs`

```rust
//! Update download cache housekeeping. Only the reviewed release is kept.
use std::fs;
use std::io;
use std::path::Path;

fn is_release_id(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Remove every other release directory. A directory that cannot be removed
/// (for example a setup that is still running) is skipped and retried later.
pub fn prune_releases(cache: &Path, keep: &str) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(cache)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let kind = entry.file_type()?;
        if name != keep && is_release_id(name) && kind.is_dir() && !kind.is_symlink()
            && fs::remove_dir_all(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Remove interrupted downloads. Downloads are single-flight, so no partial
/// file is in use when a download or launch starts.
pub fn prune_partials(release: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(release)? {
        let entry = entry?;
        let partial = entry.file_name().to_str().is_some_and(|name| name.starts_with(".partial-"));
        if partial && entry.file_type()?.is_file() && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> String {
        format!("{:064x}", n)
    }

    #[test]
    fn keeps_only_the_reviewed_release_and_unrelated_entries() {
        let cache = tempfile::tempdir().unwrap();
        for n in 1..=3 {
            fs::create_dir(cache.path().join(id(n))).unwrap();
            fs::write(cache.path().join(id(n)).join("Devbox_setup.exe"), b"x").unwrap();
        }
        fs::create_dir(cache.path().join("not-a-release")).unwrap();
        assert_eq!(prune_releases(cache.path(), &id(2)).unwrap(), 2);
        let mut names: Vec<_> = fs::read_dir(cache.path()).unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec![id(2), "not-a-release".to_string()]);
    }

    #[test]
    fn removes_interrupted_partials_only() {
        let release = tempfile::tempdir().unwrap();
        fs::write(release.path().join(".partial-1"), b"x").unwrap();
        fs::write(release.path().join(".partial-2"), b"x").unwrap();
        fs::write(release.path().join("Devbox_0.9.0_x64-setup.exe"), b"x").unwrap();
        assert_eq!(prune_partials(release.path()).unwrap(), 2);
        assert!(release.path().join("Devbox_0.9.0_x64-setup.exe").exists());
    }
}
```

`core/mod.rs`에 `pub mod update_cache;`를 추가한다. `apps/devbox-control-center/src-tauri/Cargo.toml`의 `[dev-dependencies]`에 `tempfile`이 없으면 `tempfile = "3"`을 추가한다.

- [ ] **Step 2: 통과 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-control-center --lib core::update_cache` → PASS.

- [ ] **Step 3: `updates.rs`의 `fn directory` 교체** (232–262행)

```rust
fn directory(review: &Review) -> Result<PathBuf> {
    let parent = dirs::data_local_dir().ok_or("update_cache_unavailable")?;
    ensure_no_links(&parent).map_err(|_| "update_cache_unsafe")?;
    let cache = parent.join(format!("com.devbox.v08.suite-downloads.i{}", review.key));
    for path in [&cache, &cache.join(&review.id)] {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("update_cache_unavailable"),
        }
        ensure_no_links(path).map_err(|_| "update_cache_unsafe")?;
        if path == &cache {
            crate::core::update_cache::prune_releases(&cache, &review.id)
                .map_err(|_| "update_cache_unavailable")?;
        }
    }
    let release = cache.join(&review.id);
    crate::core::update_cache::prune_partials(&release).map_err(|_| "update_cache_unavailable")?;
    Ok(release)
}
```

(릴리스 디렉터리가 생기기 전에 이전 릴리스를 지우도록 `prune_releases`를 cache 생성 직후에 부른다. 이전의 16개·4개 제한 검사는 필요 없어져 삭제한다.)

- [ ] **Step 4: 문구 정리** — `apps/devbox-control-center/src/Updates.tsx`의 `messages`에서 `update_cache_review_required` 항목을 지운다(`rg -n update_cache_review_required apps` 결과가 없어야 한다).

- [ ] **Step 5: 확인·커밋** — Run: `cargo test -p devbox-control-center --lib` → PASS. (`updates.rs`는 Windows 전용이므로 컴파일 확인은 CI `Rust (Windows)`에서 된다.)

```bash
git add apps/devbox-control-center
git commit -m "fix(devbox-control-center): prune old update downloads instead of blocking updates"
```

---

### Task 2: 연결 선호 v2와 기본 자동 연결 (B3·D14)

**Interfaces:**
- Produces (`crates/suite-runtime/src/preference.rs`, 모든 플랫폼):
  - `pub const FILE: &str = "suite-connection-v2.json"; pub const LEGACY_FILE: &str = "suite-connection-v1.json";`
  - `pub enum Mode { Auto, Off }` (serde `"auto"`, `"off"`)
  - `pub struct Preference { schema: u32, product: String, installation: String, mode: Mode }` + `new`, `parse`
  - `pub fn should_auto_connect(preference: Option<&Preference>, product: &str, installation: &str) -> bool`
- Status JSON(연결 명령 `status`·`approve`·`disconnect` 공통 응답): `{ "connected": bool, "generation": string|null, "mode": "auto"|"off", "issue": string|null }`

- [ ] **Step 1: 실패하는 테스트** — `crates/suite-runtime/src/preference.rs`

```rust
//! Suite connection preference (pure). Products of one installation connect
//! automatically unless the user turned the connection off for it.
use serde::{Deserialize, Serialize};

pub const FILE: &str = "suite-connection-v2.json";
pub const LEGACY_FILE: &str = "suite-connection-v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Auto,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preference {
    pub schema: u32,
    pub product: String,
    pub installation: String,
    pub mode: Mode,
}

impl Preference {
    pub fn new(product: &str, installation: &str, mode: Mode) -> Self {
        Self { schema: 2, product: product.into(), installation: installation.into(), mode }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "suite_preference_invalid")?;
        if value.schema != 2 || value.product.is_empty() || value.installation.is_empty() {
            return Err("suite_preference_invalid");
        }
        Ok(value)
    }
}

/// A preference written for another product or installation does not apply.
pub fn should_auto_connect(preference: Option<&Preference>, product: &str, installation: &str) -> bool {
    match preference {
        Some(value) if value.product == product && value.installation == installation => value.mode == Mode::Auto,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connects_by_default_and_after_updates() {
        assert!(should_auto_connect(None, "workspace", "install-a"));
        let auto = Preference::new("workspace", "install-a", Mode::Auto);
        assert!(should_auto_connect(Some(&auto), "workspace", "install-a"));
    }

    #[test]
    fn stays_off_only_for_the_same_product_and_installation() {
        let off = Preference::new("workspace", "install-a", Mode::Off);
        assert!(!should_auto_connect(Some(&off), "workspace", "install-a"));
        assert!(should_auto_connect(Some(&off), "workspace", "install-b"));
        assert!(should_auto_connect(Some(&off), "knowledge", "install-a"));
    }

    #[test]
    fn parses_v2_and_rejects_v1_files() {
        let bytes = serde_json::to_vec(&Preference::new("workspace", "install-a", Mode::Off)).unwrap();
        assert_eq!(Preference::parse(&bytes).unwrap().mode, Mode::Off);
        assert!(Preference::parse(br#"{"schema":1,"product":"workspace","installation":"a","generation":"g"}"#).is_err());
    }
}
```

`crates/suite-runtime/src/lib.rs` 상단 `pub mod platform;` 아래에 `pub mod preference;`를 추가한다.

- [ ] **Step 2: 통과 확인** — Run: `source ~/.cargo/env && cargo test -p suite-runtime --lib preference` → PASS.

- [ ] **Step 3: 플랫폼 파일 읽기·쓰기 교체** — `platform/connection_preference.rs`
  - 옛 `const FILE`, `struct Preference`와 `impl Preference`(`matches`, `for_scope`)를 삭제하고 `use crate::preference::{Preference, FILE, LEGACY_FILE};`를 쓴다.
  - `read`: 파일 이름은 그대로 `root.join(FILE)`. 파싱 부분(`let value: Preference = serde_json::from_slice…` 이후 schema 검사 블록)을 `let value = Preference::parse(&bytes)?;`로 바꾼다(`PRODUCTS` 포함 검사는 `if !product_contract::installation::PRODUCTS.contains(&value.product.as_str()) { return Err("suite_preference_invalid"); }`로 유지).
  - `write` 시그니처를 `pub(crate) fn write(app: &tauri::AppHandle, preference: &Preference) -> Result<(), &'static str>`로 바꾸고 본문을 아래로 교체한다(디렉터리 생성·식별 확인은 기존과 같음).

```rust
pub(crate) fn write(app: &tauri::AppHandle, preference: &Preference) -> Result<(), &'static str> {
    let root = root(app)?;
    if !root.try_exists().map_err(|_| "suite_preference_unavailable")? {
        ensure_no_links(root.parent().ok_or("suite_preference_unavailable")?)
            .map_err(|_| "suite_preference_unavailable")?;
        std::fs::create_dir(&root).map_err(|_| "suite_preference_unavailable")?;
    }
    ensure_no_links(&root).map_err(|_| "suite_preference_unavailable")?;
    let (_handle, identity) =
        open_filesystem_object(&root, true).map_err(|_| "suite_preference_unavailable")?;
    let _directories = super::component_scope::pin_directories(&root)?;
    let bytes = serde_json::to_vec(preference).map_err(|_| "suite_preference_invalid")?;
    devbox_filesystem::atomic_write(root.join(FILE), &bytes)
        .map_err(|_| "suite_preference_unavailable")?;
    match std::fs::remove_file(root.join(LEGACY_FILE)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("suite_preference_unavailable"),
    }
    if filesystem_identity(&root, true).ok() != Some(identity) {
        return Err("suite_preference_changed");
    }
    Ok(())
}
```

- [ ] **Step 4: `lib.rs` 상태와 흐름 수정**
  - `struct Link`의 `remembered: bool`을 `mode: crate::preference::Mode,`와 `issue: Option<&'static str>,`로 바꾸고, `Default`에서 `mode: crate::preference::Mode::Auto, issue: None,`로 초기화한다.
  - 공용 응답 함수를 `#[cfg(windows)]`로 추가한다.

```rust
#[cfg(windows)]
fn status_value(state: &Link) -> serde_json::Value {
    serde_json::json!({
        "connected": state.approved.is_some(),
        "generation": state.approved.as_ref().map(|scope| &scope.id),
        "mode": state.mode,
        "issue": state.issue,
    })
}
```

  - `resume`의 `spawn_blocking` 본문(299–306행)을 아래로 교체하고, 그 뒤 `let Ok(Ok(Some(scope))) = captured else { return; };`를 결과별 처리로 바꾼다.

```rust
    let captured = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let scope = capture_own(product, version)?;
        let preference = platform::connection_preference::read(&storage_app).unwrap_or(None);
        if !crate::preference::should_auto_connect(preference.as_ref(), product, &scope.installation_key) {
            return Ok::<_, &'static str>(None);
        }
        Ok(Some(Arc::new(scope)))
    })
    .await;
    let scope = match captured {
        Ok(Ok(Some(scope))) => scope,
        Ok(Ok(None)) => {
            if let Ok(mut state) = state.lock() {
                state.mode = crate::preference::Mode::Off;
            }
            return;
        }
        Ok(Err(issue)) => {
            if let Ok(mut state) = state.lock() {
                state.issue = Some(issue);
            }
            return;
        }
        Err(_) => return,
    };
```

  - `resume` 끝의 `state.remembered = true;` → `state.mode = crate::preference::Mode::Auto; state.issue = None;`
  - `Method::Status`(521–525행) → `Ok(status_value(&state.lock().map_err(|_| "suite_busy")?))`
  - `Method::Approve`: `platform::connection_preference::write(&app, product, remember.then_some(scope.as_ref()))?;`를 아래로 바꾸고, `state.remembered = remember;`를 `state.mode = crate::preference::Mode::Auto; state.issue = None;`로 바꾼다. `remember`는 쓰지 않으므로 `let _ = remember;`를 둔다(요청 형식 호환). 반환값은 `Ok(status_value(&state))`(drop 전에 계산)로 바꾼다.

```rust
            platform::connection_preference::write(
                &app,
                &crate::preference::Preference::new(product, &scope.installation_key, crate::preference::Mode::Auto),
            )?;
```

  - `Method::Disconnect`: `platform::connection_preference::write(&storage_app, product, None)?;`를 아래로 바꾸고 `state.remembered = false;` → `state.mode = crate::preference::Mode::Off;`. 반환값은 `Ok(status_value(&…))`가 되도록, spawn_blocking이 `(state.bus.take(), status_value(&state))`를 돌려주게 바꾼다.

```rust
                let installation = match state.approved.as_ref() {
                    Some(scope) => Some(scope.installation_key.clone()),
                    None => capture_own(product, host_version(&storage_app)).ok().map(|scope| scope.installation_key),
                };
                if let Some(installation) = installation {
                    platform::connection_preference::write(
                        &storage_app,
                        &crate::preference::Preference::new(product, &installation, crate::preference::Mode::Off),
                    )?;
                }
```

  - `rg -n "remembered" crates/suite-runtime` 결과가 주석 외에 없어야 한다.

- [ ] **Step 5: 확인·커밋** — Run: `cargo test -p suite-runtime --lib` → PASS(Windows 전용 경로는 CI `Rust (Windows)`에서 컴파일된다).

```bash
git add crates/suite-runtime
git commit -m "fix(suite): connect products of one installation automatically and keep it across updates"
```

---

### Task 3: 연결 화면 (product-shell)

- [ ] **Step 1: 실패하는 테스트** — `packages/product-shell/src/SuiteConnection.test.tsx`

```tsx
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, isTauri: () => true }));

import SuiteConnection from "./SuiteConnection";
import { fixtureDescription } from "./api";
import catalog from "../../../apps/products.json";

const description = fixtureDescription("workspace");
type ConnectionArgs = { request: { header: { requestId: string }; method: { kind: string } } };
function reply(value: unknown, args: ConnectionArgs) {
  const requestId = args.request.header.requestId;
  return { operation: { provenance: { product: "workspace", component: "workspace.commands", requestId, revision: catalog.catalogRevision }, outcome: { state: "succeeded" } }, value };
}
beforeEach(() => tauri.invoke.mockReset());
afterEach(cleanup);

it("shows the automatic connection and turns it off", async () => {
  tauri.invoke.mockImplementation(async (_cmd: string, args: ConnectionArgs) => {
    const value = args.request.method.kind === "disconnect"
      ? { connected: false, generation: null, mode: "off", issue: null }
      : { connected: true, generation: "g1", mode: "auto", issue: null };
    return reply(value, args);
  });
  render(<SuiteConnection description={description} route="overview" />);
  expect(await screen.findByText("이 설치의 제품이 연결되어 있습니다.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "자동 연결 끄기" }));
  expect(await screen.findByText("자동 연결이 꺼져 있습니다.")).toBeTruthy();
});

it("explains why a development build cannot connect", async () => {
  tauri.invoke.mockImplementation(async (_cmd: string, args: ConnectionArgs) =>
    reply({ connected: false, generation: null, mode: "auto", issue: "suite_package_unavailable" }, args));
  render(<SuiteConnection description={description} route="overview" />);
  await waitFor(() => expect(screen.getByText(/설치형 Suite로 실행할 때만/)).toBeTruthy());
});
```

(`reply`는 요청 헤더의 requestId로 provenance를 맞춘다. `fixtureDescription`의 handshake로 요청이 만들어진다.)

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/product-shell exec vitest run src/SuiteConnection.test.tsx` → FAIL.

- [ ] **Step 3: 구현** — `SuiteConnection.tsx`를 아래로 교체한다.

```tsx
import {lazy,Suspense,useCallback,useEffect,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {makeRequest,nativeMode,type Description} from "./api";
import {isOperation,type Operation} from "./operation";
import catalog from "../../../apps/products.json";
const ShortcutSettings=lazy(()=>import("./ShortcutSettings"));
interface Review {token:string;version:string;root:string;installationId:string;generation:string;products:{product:string;available:boolean}[]}
export interface ConnectionStatus {connected:boolean;generation:string|null;mode:"auto"|"off";issue:string|null}
const labels:Record<string,string>=Object.fromEntries(catalog.products.map(product=>[product.id,product.label]));
const issues:Record<string,string>={
  suite_package_unavailable:"설치형 Suite로 실행할 때만 제품을 연결할 수 있습니다. 개발 빌드나 따로 푼 ZIP은 연결하지 않습니다.",
  suite_image_unavailable:"실행 파일 위치를 확인하지 못해 제품을 연결하지 않았습니다.",
};
export default function SuiteConnection({description,route}:{description:Description;route:string}){
  const [review,setReview]=useState<Review|null>(null),[status,setStatus]=useState<ConnectionStatus|null>(null);
  const [busy,setBusy]=useState(false),[issue,setIssue]=useState("");
  const call=useCallback(async<T,>(method:object):Promise<T>=>{
    if(!nativeMode)throw new Error("브라우저 미리보기에서는 제품을 연결할 수 없습니다.");
    const header=makeRequest(description.handshake,route,Date.now(),description.context);
    header.deadlineMs=Date.now()+29000;
    const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
    const response=await invoke<{operation:Operation;value:T}>("plugin:suite|connection",{request:{header,method}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("제품 연결 응답을 확인할 수 없습니다.");
    return response.value;
  },[description,route]);
  const perform=async(action:()=>Promise<void>)=>{
    if(busy)return;setBusy(true);setIssue("");
    try{await action();}catch{setIssue("제품 연결을 완료하지 못했습니다. 설치 상태를 확인한 뒤 다시 시도해 주세요.");}
    finally{setBusy(false);}
  };
  useEffect(()=>{
    if(!nativeMode)return;
    let active=true;
    void call<ConnectionStatus>({kind:"status"}).then(value=>{if(active)setStatus(value);}).catch(()=>{if(active)setIssue("제품 연결 상태를 확인하지 못했습니다.");});
    return()=>{active=false;};
  },[call]);
  return <section aria-label="제품 연결">
    <h2>제품 연결</h2>
    <p>같은 설치 폴더의 제품은 자동으로 서로 연결되어 명령과 선택한 작업을 주고받습니다.</p>
    {status?.connected?<>
      <p role="status">이 설치의 제품이 연결되어 있습니다.</p>
      <button disabled={busy} onClick={()=>void perform(async()=>{setStatus(await call<ConnectionStatus>({kind:"disconnect"}));setReview(null);})}>자동 연결 끄기</button>
    </>:<>
      {status?.issue&&<p role="status">{issues[status.issue]??"이 실행 파일은 제품 연결을 사용할 수 없습니다."}</p>}
      {status?.mode==="off"&&<p role="status">자동 연결이 꺼져 있습니다.</p>}
      <button disabled={busy||!nativeMode} onClick={()=>void perform(async()=>{
        const current=await call<ConnectionStatus>({kind:"status"});setStatus(current);
        if(!current.connected)setReview(await call<Review>({kind:"preview"}));
      })}>이 설치 확인</button>
    </>}
    {review&&!status?.connected&&<div>
      <p>Devbox {review.version}</p><p>{review.root}</p>
      <ul>{review.products.map(product=><li key={product.product}>{labels[product.product]??product.product} · {product.available?"연결 가능":"파일 확인 필요"}</li>)}</ul>
      <button disabled={busy} onClick={()=>void perform(async()=>{setStatus(await call<ConnectionStatus>({kind:"approve",token:review.token,remember:true}));setReview(null);})}>연결 켜기</button>
      <button disabled={busy} onClick={()=>setReview(null)}>취소</button>
    </div>}
    {status?.connected&&<Suspense fallback={<p role="status">단축키 설정을 불러오고 있습니다…</p>}><ShortcutSettings description={description} route={route}/></Suspense>}
    {busy&&<p role="status">제품 연결을 확인하고 있습니다…</p>}{issue&&<p role="alert">{issue}</p>}
    {!nativeMode&&<p>브라우저 미리보기에서는 실제 제품을 연결하지 않습니다.</p>}
  </section>;
}
```

- [ ] **Step 4: 통과 확인·커밋** — Run: `pnpm --filter @devbox/product-shell exec vitest run` → PASS.

```bash
git add packages/product-shell/src/SuiteConnection.tsx packages/product-shell/src/SuiteConnection.test.tsx
git commit -m "fix(suite): show automatic product connection state and reasons"
```

---

### Task 4: Control Center 단축키 화면과 실제 조합 테스트 (B4)

- [ ] **Step 1: `Content` 분리** — `apps/devbox-control-center/src/main.tsx`의 lazy import들과 `function Content(...)`를 새 파일 `apps/devbox-control-center/src/Content.tsx`로 옮기고 `export default function Content`로 내보낸다. `main.tsx`는 아래만 남긴다.

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import {ProductShell} from "@devbox/product-shell";
import "./App.css";
import Content from "./Content";
ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><ProductShell product="control-center" renderContent={Content}/></React.StrictMode>);
```

- [ ] **Step 2: 실패하는 테스트** — `apps/devbox-control-center/src/App.test.tsx`를 아래로 교체한다.

```tsx
import { afterEach, expect, it } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { ProductShell } from "@devbox/product-shell";
import catalog from "../../products.json";
import Content from "./Content";

afterEach(() => { cleanup(); window.history.replaceState(null, "", "/"); });
const routes = catalog.features.filter((feature) => feature.owner === "control-center").map((feature) => feature.route);

it.each(routes)("renders a real view for the %s route", async (route) => {
  window.history.replaceState(null, "", `/?route=${route}`);
  const { container } = render(<ProductShell product="control-center" renderContent={Content} />);
  await screen.findByRole("navigation", { name: "제품 화면" });
  await waitFor(() => expect(screen.queryByText(/불러오고 있습니다/)).toBeNull());
  expect(screen.queryByText("기능 이전을 준비하고 있습니다")).toBeNull();
  expect(screen.queryByText("이 화면을 찾을 수 없습니다.")).toBeNull();
  if (route === "products") await assertNoA11yViolations(container);
});

it("shows global shortcut settings on the shortcuts route", async () => {
  window.history.replaceState(null, "", "/?route=shortcuts");
  render(<ProductShell product="control-center" renderContent={Content} />);
  expect(await screen.findByText("전역 단축키")).toBeTruthy();
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm --filter devbox-control-center exec vitest run src/App.test.tsx` → shortcuts 관련 테스트 FAIL.

- [ ] **Step 4: 구현**
  - `packages/product-shell/package.json`의 `exports`에 `"./shortcut-settings": "./src/ShortcutSettings.tsx",`를 추가한다.
  - `Content.tsx` 상단 lazy import에 `const ShortcutSettings=lazy(()=>import("@devbox/product-shell/shortcut-settings"));`를 추가하고 `RouteView` lazy import를 삭제한다.
  - route 분기 삼항식에서 `:feature?<RouteView description={props.description} feature={feature}/>:null` 부분을 아래로 바꾼다(`const feature=…` 선언도 삭제).

```tsx
:props.route==="shortcuts"?<ShortcutSettings description={props.description} route={props.route}/>
:<p role="status">이 화면을 찾을 수 없습니다.</p>
```

  - `packages/product-shell/src/RouteView.tsx:19`의 `<div className="shell-empty">…</div>`를 `<div className="shell-empty"><h2>이 화면은 아직 제공되지 않습니다</h2></div>`로 바꾼다(`renderContent` 없이 셸을 쓰는 테스트 전용 경로). `packages/product-shell/src/shell.test.tsx` 등에서 옛 문구를 기대하면 새 문구로 바꾼다(`rg -n "기능 이전을 준비하고 있습니다" packages apps`).

- [ ] **Step 5: 통과 확인·커밋** — Run: `pnpm --filter devbox-control-center exec vitest run && pnpm --filter @devbox/product-shell exec vitest run` → PASS.

```bash
git add apps/devbox-control-center/src packages/product-shell
git commit -m "fix(devbox-control-center): show shortcut settings and test every catalog route"
```

---

### Task 5: 조기 거부 오류 문구 (B5)

- [ ] **Step 1: 실패하는 테스트** — `packages/product-shell/src/operation.test.ts`의 `describe` 안에 추가:

```ts
  it("maps a host's early rejection for this product to its message", () => {
    const rejected = { code: "invalid-request", provenance: { product: expected.product, component: `${expected.product}.dispatch`, requestId: "rejected", revision: 1 } };
    expect(problemMessage(rejected, expected)).toBe("작업 요청을 확인할 수 없습니다.");
    expect(problemMessage({ ...rejected, provenance: { ...rejected.provenance, product: "other" } }, expected))
      .toBe("작업 상태를 확인할 수 없습니다.");
  });
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/product-shell exec vitest run src/operation.test.ts` → FAIL.

- [ ] **Step 3: 구현** — `operation.ts`의 `problemMessage`를 교체한다.

```ts
export function problemMessage(value: unknown, expected: Provenance): string {
  if (!record(value) || Object.keys(value).sort().join(",") !== "code,provenance" || !code(value.code)) return messages.unavailable;
  const provenance = value.provenance;
  // Hosts reject malformed calls before they can echo the request; they use a
  // fixed "rejected" provenance for their own product.
  if (record(provenance) && provenance.requestId === "rejected" && provenance.product === expected.product) {
    return messages[value.code];
  }
  return matchesProvenance(provenance, expected) ? messages[value.code] : messages.unavailable;
}
```

- [ ] **Step 4: 통과 확인·커밋** — Run: `pnpm --filter @devbox/product-shell exec vitest run` → PASS.

```bash
git add packages/product-shell/src/operation.ts packages/product-shell/src/operation.test.ts
git commit -m "fix(suite): show the actual reason for early host rejections"
```

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] Windows 실기 체크리스트(사용자 확인 대기로 기록):
  1. Suite 설치본에서 네 제품을 켜고, 각 제품의 '제품 연결' 패널이 별도 승인 없이 "연결되어 있습니다"인지 확인한다.
  2. Workspace에서 '자동 연결 끄기' 후 재시작 → 꺼진 상태가 유지되는지, '이 설치 확인 → 연결 켜기'로 다시 켜지는지 확인한다.
  3. Control Center > 단축키 메뉴에 "전역 단축키" 화면이 나오는지 확인한다.
  4. (P3-01 공개 뒤 회사 PC를 Control Center로 업데이트할 때) 업데이트 후 연결이 유지되는지 확인한다. `%LOCALAPPDATA%\com.devbox.v08.suite-downloads.*`에 최신 릴리스 폴더 하나만 남는지는 v0.9.0이 다음 업데이트를 받을 때 확인한다.
