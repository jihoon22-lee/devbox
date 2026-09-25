# P0-08 describe 1회화와 activation 캐시 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 제품 명령 한 번에 IPC 두 번과 설치 파일 IO 두 세트가 드는 구조를 없앤다. 렌더러는 셸이 받은 description을 재사용하고, native `describe`는 메인 스레드를 떠나며, activation 파일은 바뀌었을 때만 다시 검증한다(P1).

**Architecture:** `@devbox/product-shell/api`에 제품별 description 캐시(`currentDescription`, `publishDescription`, `invalidateDescription`)를 두고 `ProductShell`이 로드·context 갱신 때 게시, 언마운트 때 무효화한다. 네 제품 transport와 Workspace `nativeCall`은 `describe` 대신 `currentDescription`을 쓴다. native는 `ShellState`에 `ActivationCache`를 두어 manifest·marker의 (길이, 수정 시각)이 같으면 이전 검증 결과를 돌려준다. `describe`·`route_status`는 async command가 된다.

**Tech Stack:** Rust(Tauri v2), TypeScript, Vitest

**Spec:** `review.md` §5 P1

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 캐시는 권한을 넓히지 않는다: 모든 요청은 여전히 native `authorize`(세션·route·context·deadline·replay·activation)를 통과해야 한다.
- activation 캐시는 파일 길이·수정 시각이 하나라도 바뀌거나 stat이 실패하면 전체 검증을 다시 한다. 오류 결과는 캐시하지 않는다.
- context가 바뀌는 경로(Workspace 프로젝트 선택 → `refreshContext`)는 기존대로 description을 다시 받고 캐시를 교체한다.

## Review Focus

1. Workspace에서 프로젝트를 바꾼 직후의 파일·터미널 호출 → 새 context로 나간다(캐시 교체). (Task 2)
2. 첫 화면이 뜨기 전에 transport가 먼저 불림(예: 시작 직후 lifecycle 조회) → `describe`를 한 번만 부르고 이후 호출이 공유한다. (Task 2)
3. `describe`가 실패한 뒤 재시도 → 실패한 Promise가 캐시에 남지 않아 다시 부른다. (Task 2)
4. Control Center가 activation을 Import → Committed로 바꿈 → 제품의 다음 요청부터 Committed 규칙이 적용된다(파일 길이·시각 변경 감지). (Task 1)
5. 테스트 간 교차 오염: 한 테스트가 캐시한 description이 다음 테스트로 새지 않는다(`ProductShell` 언마운트 시 무효화, 테스트 도우미 `resetDescriptionCache`). (Task 2)

## Branch · PR

- 묶음: **B2** — 브랜치 `fix/suite/files-webhooks-logs-describe`, PR 제목 `fix(suite): cloud files, webhook bodies, operation logs and one describe per session`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `perf(suite): reuse the shell description and cache activation checks`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/product-shell-tauri/src/installation.rs` | 수정 | `ActivationCache` |
| `crates/product-shell-tauri/src/lib.rs` | 수정 | `ShellState.activation`, async `describe`/`route_status`, 캐시 사용 |
| `packages/product-shell/src/api.ts` | 수정 | description 캐시 API |
| `packages/product-shell/src/index.tsx` | 수정 | 게시·무효화 |
| `packages/product-shell/src/api.test.ts` | 생성 | 캐시 테스트 |
| `apps/devbox-api-studio/src/transport.ts` | 수정 | `currentDescription` |
| `apps/devbox-knowledge/src/transport.ts` | 수정 | `currentDescription` |
| `apps/devbox-control-center/src/Tools.tsx` | 수정 | `currentDescription` |
| `apps/devbox-workspace/src/native.ts` | 수정 | `nativeCall`이 `currentDescription` 사용 |
| `apps/devbox-workspace/src/Workspace.tsx` | 수정 | 모듈 변수 `displayed` 제거 |

---

### Task 1: native activation 캐시와 async describe

**Files:** `crates/product-shell-tauri/src/installation.rs`, `crates/product-shell-tauri/src/lib.rs`

**Interfaces (Produces):** `installation::ActivationCache::default()`, `ActivationCache::activation(&self, executable: &Path, version: &str) -> Result<Option<Activation>, &'static str>`

- [ ] **Step 1: 실패하는 테스트** — `installation.rs` 테스트 모듈에 추가한다(기존 `Fixture` 사용).

```rust
    #[test]
    fn activation_cache_reuses_a_verified_marker_until_it_changes() {
        use product_contract::activation::Phase;
        let root = Fixture::new();
        let executable = root.generation("one");
        let marker = |phase: &str, revision: u64| {
            format!(
                r#"{{"schemaVersion":1,"installationId":"fixture","generation":"one","operationId":"fixture-operation","revision":{revision},"phase":"{phase}"}}"#
            )
        };
        let path = root.0.join("devbox-activation.json");
        fs::write(&path, marker("import", 0)).unwrap();
        let cache = ActivationCache::default();
        let phase = |cache: &ActivationCache| cache.activation(&executable, "0.8.0").unwrap().unwrap().phase;
        assert_eq!(phase(&cache), Phase::Import);
        assert_eq!(phase(&cache), Phase::Import);
        assert_eq!(cache.full_reads(), 1);
        fs::write(&path, marker("committed", 1)).unwrap();
        assert_eq!(phase(&cache), Phase::Committed);
        assert_eq!(cache.full_reads(), 2);
        fs::remove_file(&path).unwrap();
        assert!(cache.activation(&executable, "0.8.0").is_err());
        let direct = root.0.join("devbox-workspace.exe");
        fs::write(&direct, b"fixture").unwrap();
        assert!(cache.activation(&direct, "0.8.0").unwrap().is_none());
    }
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p product-shell-tauri --lib installation` → 컴파일 실패(`ActivationCache` 없음).

- [ ] **Step 3: 구현** — `installation.rs`의 `activation` 함수 아래에 추가한다.

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

fn stamp(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().map(|metadata| Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// Every authorization consults the activation marker. Re-verify the
/// manifest and marker only when either file's length or modification time
/// changes; errors are never cached.
#[derive(Default)]
pub(crate) struct ActivationCache {
    entry: std::sync::Mutex<Option<(Stamp, Stamp, product_contract::activation::Activation)>>,
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

impl ActivationCache {
    pub(crate) fn activation(
        &self,
        executable: &Path,
        version: &str,
    ) -> Result<Option<product_contract::activation::Activation>> {
        let Some(root) = generation_root(executable) else {
            return Ok(None);
        };
        let stamps = stamp(&root.join("devbox-installation.json"))
            .zip(stamp(&root.join("devbox-activation.json")));
        if let (Some((manifest, marker)), Ok(entry)) = (stamps, self.entry.lock()) {
            if let Some((cached_manifest, cached_marker, value)) = entry.as_ref() {
                if *cached_manifest == manifest && *cached_marker == marker {
                    return Ok(Some(value.clone()));
                }
            }
        }
        #[cfg(test)]
        self.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result = activation(executable, version)?;
        if let (Some((manifest, marker)), Some(value)) = (stamps, result.as_ref()) {
            if let Ok(mut entry) = self.entry.lock() {
                *entry = Some((manifest, marker, value.clone()));
            }
        }
        Ok(result)
    }

    #[cfg(test)]
    fn full_reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }
}
```

`lib.rs`
- `ShellState`에 `activation: installation::ActivationCache,` 필드를 추가하고 `setup`의 `app.manage(ShellState { … })`에 `activation: Default::default(),`를 넣는다.
- `describe` 안의 `installation::activation(&state.executable, &state.version)`와 `authorize_inner` 안의 같은 호출을 `state.activation.activation(&state.executable, &state.version)`으로 바꾼다.
- `#[tauri::command] fn describe(`를 `#[tauri::command] async fn describe(`로, `fn route_status(`를 `async fn route_status(`로 바꾼다(반환형은 이미 `Result`라 async command 조건을 만족한다). 두 함수 본문은 그대로 둔다: `local_main`의 WebView2 URL 조회는 이제 worker 스레드에서 UI 스레드로 동기 호출되며, UI 스레드가 이 command를 처리하지 않으므로 교착이 없다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p product-shell-tauri --lib && cargo clippy -p product-shell-tauri --all-targets -- -D warnings` → PASS.

- [ ] **Step 5: 커밋**

```bash
git add crates/product-shell-tauri
git commit -m "perf(suite): cache verified activation markers and describe off the main thread"
```

---

### Task 2: 렌더러 description 캐시

**Files:** `packages/product-shell/src/api.ts`, `index.tsx`, `api.test.ts`(생성), 네 제품 transport, `apps/devbox-workspace/src/Workspace.tsx`

**Interfaces (Produces):** `currentDescription(product: ProductId): Promise<Description>`, `publishDescription(description: Description): void`, `invalidateDescription(product: ProductId): void`, `resetDescriptionCache(): void`(테스트용)

- [ ] **Step 1: 실패하는 테스트** — `packages/product-shell/src/api.test.ts`

```ts
import { afterEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: native.invoke }));

import { currentDescription, fixtureDescription, invalidateDescription, publishDescription, resetDescriptionCache } from "./api";

afterEach(() => { resetDescriptionCache(); native.invoke.mockReset(); });

describe("description cache", () => {
  it("shares one describe call across concurrent and later calls", async () => {
    native.invoke.mockResolvedValue(fixtureDescription("knowledge"));
    const [first, second] = await Promise.all([currentDescription("knowledge"), currentDescription("knowledge")]);
    await currentDescription("knowledge");
    expect(first).toBe(second);
    expect(native.invoke).toHaveBeenCalledTimes(1);
  });

  it("uses the published description until it is invalidated", async () => {
    const published = { ...fixtureDescription("workspace"), context: null };
    publishDescription(published);
    expect(await currentDescription("workspace")).toBe(published);
    expect(native.invoke).not.toHaveBeenCalled();
    invalidateDescription("workspace");
    native.invoke.mockResolvedValue(fixtureDescription("workspace"));
    await currentDescription("workspace");
    expect(native.invoke).toHaveBeenCalledTimes(1);
  });

  it("does not keep a failed describe", async () => {
    native.invoke.mockRejectedValueOnce(new Error("offline")).mockResolvedValue(fixtureDescription("api-studio"));
    await expect(currentDescription("api-studio")).rejects.toThrow();
    await expect(currentDescription("api-studio")).resolves.toMatchObject({ product: { id: "api-studio" } });
    expect(native.invoke).toHaveBeenCalledTimes(2);
  });

  it("keeps products apart", async () => {
    publishDescription(fixtureDescription("knowledge"));
    native.invoke.mockResolvedValue(fixtureDescription("api-studio"));
    expect((await currentDescription("api-studio")).product.id).toBe("api-studio");
  });
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/product-shell exec vitest run src/api.test.ts` → FAIL(내보낸 함수 없음).

- [ ] **Step 3: 구현**

`packages/product-shell/src/api.ts` — `describe` 함수 아래에 추가한다.

```ts
const descriptions = new Map<ProductId, Promise<Description>>();

/** Transports reuse the description the shell is showing instead of paying a
 * describe IPC per command. Native authorization still checks every request. */
export function currentDescription(product: ProductId): Promise<Description> {
  const cached = descriptions.get(product);
  if (cached) return cached;
  const loading = describe(product);
  descriptions.set(product, loading);
  loading.catch(() => { if (descriptions.get(product) === loading) descriptions.delete(product); });
  return loading;
}

export function publishDescription(description: Description): void {
  descriptions.set(description.product.id as ProductId, Promise.resolve(description));
}

export function invalidateDescription(product: ProductId): void { descriptions.delete(product); }

/** Test helper: forget every cached description. */
export function resetDescriptionCache(): void { descriptions.clear(); }
```

`packages/product-shell/src/index.tsx` (`ProductShell`)
- import에 `publishDescription`, `invalidateDescription`을 추가한다.
- 첫 `useEffect`의 성공 콜백을 `(value) => { if (active && id === loadId.current) { publishDescription(value); setDescription(value); } }`로 바꾸고, cleanup을 `return () => { active = false; loadId.current += 1; invalidateDescription(product); };`로 바꾼다.
- `refreshContext`의 `setDescription(next);` 앞에 `publishDescription(next);`를 추가한다.

제품 transport — `describe`를 `currentDescription`으로 바꾼다.
- `apps/devbox-api-studio/src/transport.ts`: import의 `describe`를 `currentDescription`으로, `const description = await describe("api-studio");`를 `const description = await currentDescription("api-studio");`로.
- `apps/devbox-knowledge/src/transport.ts`: 같은 방식(`"knowledge"`).
- `apps/devbox-control-center/src/Tools.tsx`: 같은 방식(`"control-center"`).
- `apps/devbox-workspace/src/native.ts`: import에 `currentDescription`을 추가하고 `nativeCall`의 `const description = await describe("workspace");`를 `const description = await currentDescription("workspace");`로. 파일에서 `describe`를 더 쓰지 않으면 import에서 뺀다.

`apps/devbox-workspace/src/Workspace.tsx`
- `let displayed: Description | undefined;`를 삭제하고 `NativeContent`의 `displayed = description;` 줄을 삭제한다.
- `configureProductTransport` 콜백을 아래로 바꾼다(`connected` 한 번 설정은 유지).

```tsx
    configureProductTransport(async <T,>(component: string, method: string, args: Record<string, unknown>) => {
      const snapshot = await currentDescription("workspace");
      const ownerRoute = component === "workspace.files" || component === "workspace.lsp" ? "files"
        : component === "workspace.source" ? "source" : component === "workspace.dependencies" ? "dependencies"
        : component === "workspace.terminal" ? "terminal" : component === "workspace.runtime" ? "tasks" : component === "workspace.logs" ? "logs"
        : component === "workspace.processes" || component === "workspace.process-actions" ? "runtime" : "overview";
      return componentCall<T>(snapshot, component, method, args, ownerRoute);
    }, description.handshake.installationId);
```

  import에 `currentDescription`을 추가하고 더 이상 쓰지 않는 `type Description` import를 정리한다(파일의 다른 곳에서 쓰면 남긴다).

- 기존 테스트가 describe 호출 횟수에 기대는지 확인한다: `rg -n "plugin:product-shell\|describe" apps/*/src packages/*/src --glob "*.test.*"`. 테스트 파일 사이에 캐시가 남지 않도록, 각 파일의 `afterEach`에 `resetDescriptionCache()`를 추가한다(`@devbox/product-shell/api`에서 import). `ProductShell`을 렌더하는 테스트는 `cleanup()`으로 언마운트될 때 이미 무효화된다.

- [ ] **Step 4: 통과 확인**

Run: `pnpm --filter @devbox/product-shell exec vitest run && pnpm --filter devbox-api-studio --filter devbox-knowledge --filter devbox-workspace --filter devbox-control-center exec vitest run`
Expected: PASS. (필터 이름은 각 `apps/*/package.json`의 `name`으로 확인한다.)

- [ ] **Step 5: 커밋**

```bash
git add packages/product-shell/src apps/devbox-api-studio/src apps/devbox-knowledge/src apps/devbox-control-center/src apps/devbox-workspace/src
git commit -m "perf(suite): reuse the shell description for product commands"
```

---

### Task 3: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9를 수행한다.
- [ ] PR 본문 "Windows 실기 확인" 절(사용자 확인 대기):
  1. 설치본 Knowledge에서 검색 인덱싱을 시작하고 작업 관리자로 CPU를 본다(이전보다 낮거나 같음).
  2. Workspace에서 프로젝트를 바꾼 뒤 파일 열기·터미널 열기가 새 프로젝트로 동작한다.
  3. Control Center에서 업데이트 확인 → 제품 목록 표시가 정상이다.
