# P1-04 v0.7 잔재 제거 ③ Workspace·Runtime — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Workspace와 runtime-engine에서 v0.7 앱(Workbench·Code Pad·Repo Manager·Port Manager·Log Lens·WSL Desktop·Run Manager) 데이터 가져오기와 그 화면을 지운다. 첫 실행은 "빈 Workspace 시작" 버튼 없이 바로 프로젝트 화면이 뜬다(D9, UX §7).

**Architecture:** `core/legacy_*.rs` 중 일부는 현재 기능이 쓰는 저장 형식(템플릿·프로필·편집기 세션·LSP 설정)을 담고 있다. 그래서 **먼저 현재 기능이 쓰는 타입을 새 모듈로 옮기고(동작·JSON 형식 불변)**, 그다음 가져오기 진입점(스냅샷 획득·변환·검토·적용·기록)을 지운다. Registry의 v0.7 참조 필드는 "비어 있으면 읽고 다시 쓰지 않는" 호환 필드로 바꾼다. Suite `import` 단계에서 Workspace는 저장소 준비(`start_empty`)만 하는 최소 화면을 보인다(Control Center owner evidence용).

**Tech Stack:** Rust, React/TypeScript, Python(guard)

**Spec:** `review.md` §6 A3, §7 · `00-roadmap.md` D9·Q1

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 저장 형식 호환: v0.8.x가 쓴 registry·LSP 설정·편집기 세션·터미널 설정 파일은 그대로 읽혀야 한다. JSON 키 이름(`importedTemplates`, `importedProfiles`, `importedProfileBindings`, `wsl-desktop:*` 설정 키 등)은 바꾸지 않는다. Rust 타입·모듈 이름만 바꾼다.
- 템플릿으로 프로젝트 등록(`preview_template_profile_windows/wsl`), 템플릿 편집·보관(`save_template`, `archive_template`), 프로젝트 정의 가져오기(`crates/runtime-engine/src/core/imports.rs`, package.json·Cargo.toml → task)와 task 가져오기 대화상자(`ImportDialog`)는 v0.7과 무관한 현재 기능이다. 지우지 않는다.
- `workspace.migration` component는 `status`·`start_empty`만 남긴다(이름은 P1-14에서 `workspace.setup`으로 바꾼다).

## Review Focus

1. v0.8.1에서 만든 registry 파일(`legacyReferences: []`, 템플릿 1개, 템플릿으로 등록한 프로젝트 1개) → 그대로 읽히고, 저장하면 `legacyReferences` 키가 사라진다. (Task 2 테스트)
2. v0.7 가져오기를 실제로 썼던 registry(`legacyReferences`가 비어 있지 않음) → `registry_retired_import_data`로 멈추고 안내 문구가 나온다(조용히 버리지 않음). (Task 2)
3. 저장된 LSP 설정·실행 승인이 있는 상태에서 업데이트 → 설정 화면이 같은 값을 보여 준다(모듈 이동이 decode를 바꾸지 않음). (Task 2)
4. 첫 실행(registry 없음) → `start_empty`가 한 번만 자동 실행되고 프로젝트 등록 폼이 보인다. 실패하면 오류와 "다시 시도". (Task 4)
5. Suite `import` 단계에서 Workspace 열기 → 저장소 준비만 하고 "Control Center에서 설치 활성화를 완료해 주세요"가 보이며 파일·터미널·작업 화면은 열리지 않는다. (Task 4)

## Branch · PR

- 묶음: **B3** — 브랜치 `refactor/suite/archive-and-remove-v07`, PR 제목 `refactor(suite): archive history, remove v0.7 imports and unify dependencies`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(devbox-workspace): remove v0.7 imports`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## 옮길 타입(Task 2, 동작 불변)

| 지금 | 옮긴 뒤 | 남기는 것 | 지우는 것 |
|---|---|---|---|
| `core/legacy_templates.rs` | `core/templates.rs` | `ImportedTemplate`와 `validate`, `is_false` 등 직렬화 도우미 | 스냅샷→템플릿 변환·미리보기·적용 함수 |
| `core/legacy_profiles.rs` | `core/profiles.rs` | `ImportedProfile`, `ProfileBinding`과 검증 | 스냅샷→프로필 변환·미리보기·적용 함수 |
| `core/legacy_sessions.rs` | `core/editor_sessions.rs` | `StoredSession`(제품 편집기 세션 저장 형식)과 읽기·쓰기 | `Candidate`, `candidate()`(Code Pad 세션 변환) |
| `core/legacy_lsp.rs` | `lsp_host/config.rs` | `LspConfig`, `StoredConfig`, `decode`, `StoredConfig::decode`와 테스트 | 가져오기 receipt 관련 함수(`imports` 필드는 호환을 위해 남기되 더 이상 추가하지 않음) |
| `terminal_export.rs`의 `KEYS` | `terminal_profiles.rs`의 `pub(crate) const PREFERENCE_KEYS` | 7개 키 문자열 그대로 | 나머지 export worker 전부 |
| `core/legacy_inventory.rs`의 `digest` | `crate::definitions::digest` | (같은 SHA-256 hex 구현이 이미 있음) | 파일 전체 |

## 삭제 목록(Task 3)

| 경로 | 비고 |
|---|---|
| `apps/devbox-workspace/src-tauri/src/{legacy_imports,migration_ledger,window_import,terminal_import,terminal_export}.rs` | `lib.rs`의 `terminal_export::argument`·`run_worker` 분기도 제거 |
| `apps/devbox-workspace/src-tauri/src/files_host/recovery_import.rs`, `lsp_host/settings_import.rs`, `runtime_host/settings.rs` | 마지막 것은 Port Manager·Log Lens 설정 가져오기 |
| `apps/devbox-workspace/src-tauri/src/core/{legacy_inventory,legacy_snapshot,legacy_workspace,legacy_recovery,legacy_references}.rs` | Task 2에서 옮긴 뒤 남은 `legacy_*` 전부 |
| `files_host.rs` 안의 세션·복구 가져오기 메서드 | `preview/apply/cancel_session_import`, `list_session_history`, `preview_session_restore`, `*_recovery_import`, `list_recovery_history`, `preview_recovery_restore` |
| `crates/runtime-engine/src/core/{runtime_import,runtime_backup}.rs`, `storage/imports.rs`, `component/imports.rs` | `imported_log_descriptor` 등 imported 계열 공개 함수 포함. `data-migration` 의존성 제거 |
| `apps/devbox-workspace/src/{LegacyImports,LegacyLspImport,LegacyProfileImport,LegacyRecoveryImport,LegacyReferenceLookup,LegacySessionImport,LegacyTemplateImport,LegacyWindowImport,LegacyWorkspace,MigrationOnly,RuntimeImport,RuntimeSettingsImport,TerminalImport}.tsx`와 테스트, `legacySources.ts`, `src/migration/` | |
| `.github/scripts/windows-workspace-{session,template,terminal,runtime,window}-import.mjs` | workflow step도 제거 |

## 정리 규칙(컴파일 오류가 날 때)

| 오류가 가리키는 것 | 처리 |
|---|---|
| `component.rs`의 `migration_method` 목록 | `"workspace.migration"`은 `status`·`start_empty`만, `"workspace.registry"`는 `snapshot`만 남긴다. runtime·files·lsp·terminal 항목은 지운다 |
| `allowed()`의 가져오기 메서드(`prepare_legacy_snapshot`, `preview_profile_import`, `preview_template_import`, `legacy_workspace`, `resolve_legacy_reference`, `preview_legacy_workspace_windows`, `preview_imported_profile_windows/wsl`, `runtime_import_*`, `*terminal_import*`, `list_workspace_profiles`, 창 가져오기 메서드) | 목록에서 지운다. 테스트의 기대값도 고친다 |
| `project_owner.rs`의 `preview_legacy_workspace_windows`, `preview_imported_profile_*`, `resolve_legacy_reference` | 함수째 지운다. `preview_template_profile_*`와 `prepare_template_binding`은 남긴다 |
| `federation.rs`의 `legacy_references` 조회 | 해당 Call 분기를 "없음"(`Ok(Value::Null)` 또는 기존 not-found 오류 코드)으로 바꾼다 |
| runtime-engine `component.rs`의 import dispatch·migration_status 사용 | import 분기를 지우고, migration 요약은 import 계획 없이 "준비됨"으로 계산한다 |
| `terminal_profiles.rs`의 `migration_status` 사용 | import 대기 여부를 보던 조건을 지운다(항상 대기 없음) |
| 프런트에서 쓰이지 않게 된 state(`sessionImportBusy`, `recoveryImportBusy`, `lspImportBusy`, `onBusyChange` 등) | 지운다 |

---

### Task 1: guard 범위

- [ ] `check-no-legacy.py`의 `SCOPES`에 추가한다.

```python
    "workspace": [
        "apps/devbox-workspace",
        "packages/workspace-features",
        "crates/runtime-engine",
    ],
```

  `PATTERNS`에 추가한다: `r"\blegacy_(inventory|snapshot|workspace|recovery|references|imports)\b"`, `r"\bmigration_ledger\b"`, `r"\b(window|terminal|recovery)_import\b"`, `r"\bsettings_import\b"`, `r"\bruntime_import\b"`, `r"\bpreview_(session|profile|template|lsp_config)_import\b"`, `r"\bprepare_legacy_snapshot\b"`, `r"\bresolve_legacy_reference\b"`, `r"\bMigrationOnly\b"`, `r"\bLegacy(Imports|LspImport|ProfileImport|RecoveryImport|ReferenceLookup|SessionImport|TemplateImport|WindowImport|Workspace)\b"`, `r"\blegacySources\b"`, `r"\bimported_log_descriptor\b"`.
- [ ] `ci.yml` guard 줄에 `&& python3 .github/scripts/check-no-legacy.py --scope workspace`를 붙인다.
- [ ] Run: `python3 .github/scripts/check-no-legacy.py --scope workspace` → FAIL. 커밋: `git commit -am "test(devbox-workspace): extend the legacy guard to Workspace"`

---

### Task 2: 현재 기능이 쓰는 타입 옮기기

**Files:** "옮길 타입" 표의 파일, 이를 쓰는 `core/{registry,template_editor,mod}.rs`, `project_owner.rs`, `files_host.rs`, `lsp_host/settings.rs`, `terminal_profiles.rs`

**Interfaces (Produces):** `core::templates::ImportedTemplate`, `core::profiles::{ImportedProfile, ProfileBinding}`, `core::editor_sessions::StoredSession`, `lsp_host::config::{LspConfig, StoredConfig, decode}`, `terminal_profiles::PREFERENCE_KEYS`, `Registry.retired_references: Vec<serde_json::Value>`(비공개)

- [ ] **Step 1: 실패하는 테스트** — `core/registry.rs` 테스트 모듈에 추가한다.

```rust
    #[test]
    fn v09_registries_load_and_drop_the_empty_reference_list_on_save() {
        let json = r#"{"schemaVersion":1,"revision":3,"projects":[],"worktrees":[],"legacyReferences":[]}"#;
        let registry: Registry = serde_json::from_str(json).unwrap();
        registry.validate().unwrap();
        let saved = serde_json::to_string(&registry).unwrap();
        assert!(!saved.contains("legacyReferences"), "{saved}");
        let reread: Registry = serde_json::from_str(&saved).unwrap();
        assert_eq!(reread, registry);
    }

    #[test]
    fn registries_that_still_hold_v07_references_are_refused() {
        let json = r#"{"schemaVersion":1,"revision":3,"projects":[],"worktrees":[],"legacyReferences":[{"kind":"project","id":"old"}]}"#;
        let registry: Registry = serde_json::from_str(json).unwrap();
        assert_eq!(registry.validate(), Err("registry_retired_import_data"));
    }
```

  (`schemaVersion` 값은 파일의 현재 상수로 맞춘다. `validate`의 오류 타입이 `&'static str`가 아니면 그 타입의 해당 값으로 비교한다.)

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-workspace --lib core::registry` → FAIL.

- [ ] **Step 3: 구현**
  - "옮길 타입" 표대로 `git mv`하고 남길 것만 남긴다. 옮긴 모듈의 기존 테스트 중 남긴 타입의 테스트는 함께 옮기고, 변환 함수 테스트는 지운다.
  - `core/registry.rs`: `pub legacy_references: Vec<LegacyReference>`를 아래로 바꾸고 `validate()` 앞부분에 검사를 추가한다.

```rust
    /// v0.7 reference mappings are retired. Registries written by v0.8
    /// carry an empty list; it is accepted on read and never written again.
    #[serde(default, rename = "legacyReferences", skip_serializing)]
    retired_references: Vec<serde_json::Value>,
```

```rust
        if !self.retired_references.is_empty() {
            return Err("registry_retired_import_data");
        }
```

  `Registry`를 직접 만드는 코드(테스트 포함)의 `legacy_references: …`는 `retired_references: Vec::new()`로 바꾼다(필드가 private이면 `Registry::empty()` 같은 기존 생성자를 쓰거나, 같은 모듈 안에서만 만든다).
  - `imported_templates`·`imported_profiles`·`imported_profile_bindings` 필드 타입 경로를 새 모듈로 바꾼다(JSON 키 불변).
  - `apps/devbox-workspace/src/issues.ts`(또는 `native.ts`의 issue 사전)에 `registry_retired_import_data: "v0.7에서 가져온 프로젝트 정보가 남아 있어 열 수 없습니다. v0.8.1에서 프로젝트를 다시 등록한 뒤 업데이트해 주세요."`를 추가한다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-workspace --lib` → PASS(옮긴 모듈 테스트 포함).

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-workspace): move current storage types out of legacy modules"`

---

### Task 3: 가져오기 제거(native)

**Files:** 삭제 목록의 native 항목, `component.rs`, `project_owner.rs`, `files_host.rs`, `federation.rs`, `runtime_host.rs`, `lib.rs`, `crates/runtime-engine/src/{component,storage,core}/mod.rs`, `crates/runtime-engine/Cargo.toml`

- [ ] **Step 1: 실패하는 테스트** — `component.rs` 테스트 모듈에 추가한다(기존 `allowed` 테스트 옆).

```rust
    #[test]
    fn v07_import_methods_are_gone() {
        let removed = [
            ("workspace.migration", "overview", "prepare_legacy_snapshot"),
            ("workspace.migration", "overview", "preview_profile_import"),
            ("workspace.migration", "overview", "preview_template_import"),
            ("workspace.registry", "overview", "resolve_legacy_reference"),
            ("workspace.registry", "overview", "preview_imported_profile_wsl"),
            ("workspace.runtime", "tasks", "runtime_import_prepare"),
            ("workspace.files", "files", "preview_session_import"),
            ("workspace.files", "files", "preview_recovery_import"),
            ("workspace.lsp", "files", "preview_lsp_config_import"),
            ("workspace.terminal", "terminal", "start_terminal_import"),
        ];
        for (component, route, method) in removed {
            assert!(!allowed(component, route, method), "{component}.{method}");
            assert!(!migration_method(component, method), "{component}.{method}");
        }
        assert!(allowed("workspace.migration", "overview", "start_empty"));
        assert!(migration_method("workspace.migration", "start_empty"));
        assert!(allowed("workspace.registry", "overview", "save_template"));
        assert!(allowed("workspace.registry", "overview", "preview_template_profile_wsl"));
    }
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-workspace --lib component::tests::v07_import_methods_are_gone` → FAIL.

- [ ] **Step 3: 삭제와 정리** — 삭제 목록의 native 파일을 `git rm`하고, `files_host.rs`의 세션·복구 가져오기 메서드를 지운다. `cargo check -p devbox-workspace -p devbox-runtime-engine` 오류를 "정리 규칙"대로 고친다. `crates/runtime-engine/Cargo.toml`에서 `data-migration`(별칭 `devbox-data-migration`) 의존을 지운다. `apps/devbox-workspace/src-tauri/Cargo.toml`에서도 쓰지 않게 된 `data-migration` 의존을 지운다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-workspace --lib && cargo test -p devbox-runtime-engine --lib && cargo clippy -p devbox-workspace -p devbox-runtime-engine --all-targets -- -D warnings` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-workspace): remove v0.7 import natives"`

---

### Task 4: 프런트엔드

**Files:** 삭제 목록의 화면, `apps/devbox-workspace/src/{Workspace,RegistryGate}.tsx`와 테스트

- [ ] **Step 1: 실패하는 테스트** — `apps/devbox-workspace/src/RegistryGate.test.tsx`(없으면 생성; `native.ts`를 `vi.mock`으로 대체하는 기존 테스트 방식을 따른다)에 추가한다.

```tsx
it("starts an empty registry automatically once", async () => {
  let started = false;
  native.nativeCall.mockImplementation(async (_component: string, method: string) => {
    if (method === "start_empty") { started = true; return {}; }
    throw new Error(`unexpected ${method}`);
  });
  native.registryCall.mockImplementation(async (method: string) => {
    if (method === "snapshot") return started ? readyRegistry() : setupRegistry();
    throw new Error(`unexpected ${method}`);
  });
  render(<RegistryGate context={null} onContextChanged={async () => {}} onReady={() => {}} editing={false} />);
  expect(await screen.findByLabelText("프로젝트 경로")).toBeInTheDocument();
  expect(native.nativeCall).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "빈 Workspace 시작" })).toBeNull();
});
```

  (`native`는 `vi.hoisted`로 만든 `{ nativeCall: vi.fn(), registryCall: vi.fn() }`, `setupRegistry()`/`readyRegistry()`는 파일 안에 만드는 도우미로 각각 phase "setup"과 "selected"가 되는 snapshot 응답을 돌려준다. 실제 응답 모양은 `RegistryGate.tsx`의 `refresh`가 읽는 필드를 보고 맞춘다. 입력 label은 `id="workspace-project-path"` 입력의 실제 label 문구로 바꾼다.)

  `apps/devbox-workspace/src/Workspace.test.tsx`(있는 파일)에 추가한다.

```tsx
it("shows only store preparation while the suite awaits activation", async () => {
  renderWorkspace({ deliveryState: "import" }); // 파일의 기존 렌더 도우미로 description.deliveryState를 바꿔 렌더한다
  expect(await screen.findByText("Control Center에서 설치 활성화를 완료해 주세요.")).toBeInTheDocument();
  expect(screen.queryByText(/기존 데이터를 검토하고 가져올 수 있습니다/)).toBeNull();
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter devbox-workspace exec vitest run src/RegistryGate.test.tsx src/Workspace.test.tsx` → FAIL.

- [ ] **Step 3: 구현**
  - 삭제 목록의 화면·테스트·`legacySources.ts`·`src/migration/`을 지운다.
  - `RegistryGate.tsx`: prop `migrationOnly`를 `setupOnly`로 바꾼다. `status.phase === "setup"`이면 effect에서 `start_empty`를 한 번 자동 호출한다(`useRef<boolean>`로 한 번만, 실패하면 오류를 보이고 "다시 시도" 버튼만 남긴다). "빈 Workspace 시작" 버튼과 안내 문장을 지운다.
  - `Workspace.tsx`: 삭제한 화면의 lazy import·state·prop을 지운다. `!productDataAvailable(description)` 분기에서 `deliveryState === "import"`이면 아래를 렌더한다.

```tsx
<section aria-label="Workspace 준비">
  <p role="status">Control Center에서 설치 활성화를 완료해 주세요.</p>
  <Suspense fallback={null}><RegistryGate setupOnly context={description.context} onContextChanged={refreshContext} onReady={markReady} editing={false}/></Suspense>
</section>
```

- [ ] **Step 4: 통과 확인** — Run: `pnpm --filter devbox-workspace --filter @devbox/workspace-features exec vitest run && pnpm --filter devbox-workspace exec tsc --noEmit` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-workspace): remove import screens and start the registry automatically"`

---

### Task 5: CI·문서

- [ ] `product-foundation.yml`에서 `windows-workspace-{session,template,terminal,runtime,window}-import.mjs`를 부르는 step과 paths 항목을 지우고 스크립트를 지운다. `test-product-foundation-workflow.py`를 맞춘다.
- [ ] `apps/devbox-workspace/README.md`의 v0.7 가져오기 절을 지운다.
- [ ] Run: `python3 .github/scripts/check-no-legacy.py --scope workspace && python3 .github/scripts/test-product-foundation-workflow.py` → PASS. 커밋: `git add -A && git commit -m "ci(devbox-workspace): drop import acceptance scripts"`

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): v0.8.1에서 쓰던 Workspace(프로젝트·템플릿·LSP 설정이 있는 상태)를 P3-01 후보 setup으로 업데이트한 뒤 열어 프로젝트 목록·템플릿·LSP 설정·터미널 글꼴 크기가 그대로인지 본다.
