# P1-02 v0.7 잔재 제거 ① Control Center·설치 도구 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Control Center와 `installation-tools`(옛 Devbox Manager)에서 v0.7 설치본 발견·가져오기·정리·전환(cutover) 기능과 옛 앱 관리자 화면을 지운다. 새 설치는 "신규 설치 활성화 → 제품 상태 기록 → 확정" 한 경로만 남는다(D9, A3).

**Architecture:** 끝 상태를 기계적으로 검사하는 guard 스크립트 `check-no-legacy.py`를 먼저 만들어(범위: Control Center·installation-tools·관련 CI) 실패시키고, 삭제 목록을 지운 뒤 컴파일 오류가 가리키는 호출부를 아래 "정리 규칙"대로 고쳐 guard와 기존 테스트를 통과시킨다. Suite 배포 상태 기계(activation 단계 import·health·committed·recover, 업데이트·되돌리기·데이터 복원·제거)는 그대로 둔다. `import` 단계는 "설치됐지만 아직 활성화 전"이라는 뜻으로 남고 화면 문구만 바꾼다.

**Tech Stack:** Rust, React/TypeScript, Python(guard), GitHub Actions

**Spec:** `review.md` §6 A3 · `00-roadmap.md` D9·Q1

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 지우지 않는 것(유지 목록): `bootstrap.rs`의 설치·재설치·업데이트·데이터 복원·제거 경로, `bootstrap/{data_restore,interactive,registration,reinstall,uninstall,update}.rs`, `core/{data_checkpoint,data_restore,delivery,delivery_store,inventory,package_stage,suite_package,suite_removal}.rs`, `updates.rs`, `shortcuts.rs`, `commands.rs`, `command_receipts.rs`, `federation.rs`, `platform/hotkey.rs`, UI `Inventory`·`Recovery`·`Restore`·`Health`·`Updates`·`Commands`·`HostedLauncher`·`Tools`. installation-tools의 `doctor`·`dev_setup`·`related_tools`·지원 번들, `core::{custom_root,url_policy,related_tools,environment_capabilities,dev_setup_configuration,support_bundle}`.
- `apps/legacy-v0.7-catalog.json`과 `crates/launch`는 이 PR에서 지우지 않는다(P1-05). 이 PR 이후 Control Center·installation-tools는 그 파일을 참조하지 않는다.
- 제품 쪽 `*.migration` component와 제품 가져오기 코드는 P1-03(Knowledge·API Studio)·P1-04(Workspace) 범위다.
- 신규 설치 활성화의 안전 조건(네 제품이 저장소를 준비했고 바쁘지 않음을 기록한 owner evidence 4개)은 유지한다. 기록 수단만 바뀐다: v0.7 출처·백업 목록 없이 제품 health 보고의 store 요약만 기록하고, 전용 화면(`MigrationOwners`) 대신 기존 `Health` 화면의 "상태 기록" 버튼이 activation 단계에 따라 owner evidence(import) 또는 health(health)를 기록한다.
- catalog에서 route `migration`(feature `control-center.migration`)을 지우고 `catalogRevision`을 1 올린다.

## Review Focus

1. 새 설치(activation `import`)에서 `--suite-setup`으로 연 Control Center → "데이터 및 복구" 화면이 열리고 "신규 설치 활성화 준비"로 끝까지 확정할 수 있다. (Task 3 테스트)
2. 기존 v0.8.1 설치본(이미 committed) 업데이트 → 업데이트·되돌리기·확정 버튼이 그대로 동작한다(삭제가 update 경로를 건드리지 않음). (Task 3 테스트, 묶음 B3 PR의 Product foundation acceptance)
3. 지원 번들 → 옛 앱 DB·로그 목록 없이 진단·제품 목록·운영 로그만 담긴다. (Task 4)
4. 카탈로그 revision을 올린 뒤 fixture·테스트의 하드코딩된 revision이 남아 provenance 비교가 깨짐 → 전부 갱신. (Task 2)
5. `product-foundation.yml`이 삭제한 스크립트를 부름 → PR에서 workflow가 파일 없음으로 실패. (Task 5)

## Branch · PR

- 묶음: **B3** — 브랜치 `refactor/suite/archive-and-remove-v07`, PR 제목 `refactor(suite): archive history, remove v0.7 imports and unify dependencies`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(devbox-control-center): remove v0.7 discovery, import and cleanup`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## 삭제 목록

| 경로 | 비고 |
|---|---|
| `apps/devbox-control-center/src/{MigrationOwners,Backups,LegacyInventory,LegacyCleanup,LauncherImport,Cutover}.tsx` 및 각 `*.test.tsx` | migration route 화면 |
| `apps/devbox-control-center/src-tauri/src/{launcher_import,legacy_cleanup}.rs` | `migration_evidence.rs`는 지우지 않고 `owner_evidence.rs`로 바꾼다(Task 3) |
| `apps/devbox-control-center/src-tauri/src/core/{launcher_import,legacy_binary,legacy_installation,legacy_sources,cutover,migration,migration_plan}.rs` | `migration.rs`·`migration_plan.rs`는 1줄 파일 |
| `apps/devbox-control-center/src-tauri/src/bootstrap/cutover.rs` | 출처별 전환 검토 |
| `apps/devbox-control-center/src-tauri/src/platform/legacy_installer.rs` | 제어판 등록 조회 |
| `apps/devbox-control-center/src-tauri/resources/legacy-v0.7-*.json` | tauri.conf.json `resources`에서도 제거 |
| `crates/installation-tools/src/commands/{manager,local_quality}.rs` | 옛 앱 관리자 |
| `crates/installation-tools/src/core/{asset,batch,catalog,download,layout,local_quality,managed_install,manifest,removal,runtime_metadata}.rs` | 위 두 파일만 쓰던 모듈 |
| `crates/installation-tools/src/core/data_inspector.rs` | 옛 앱 `data.db` 조회. 번들이 쓰던 도우미는 `core/redaction.rs`로 옮김 |
| `.github/scripts/{windows-suite-legacy-cleanup.ps1,windows-legacy-installer-reference.ps1,legacy-v0.7-windows-packaged-smoke-config.json,legacy-v0.7-windows-installer-acceptance-config.json}` 및 이들을 읽는 `test-*.py`의 해당 테스트 | |
| `docs/features/control-center.migration.md` | |

## 정리 규칙(컴파일 오류가 날 때)

| 오류가 가리키는 것 | 처리 |
|---|---|
| 삭제한 모듈의 함수를 부르는 `match` 팔·`if` 분기(예: tools_host의 `cleanup`·`cutover`·`legacy`·`record_migration_owner`) | 분기째 지우고, 해당 메서드 이름을 허용 목록에서도 지운다 |
| `activate_install(…, reviewed: true)`와 `"activateReviewed"`·`"commitReviewed"`·`"reviewImportAgain"` | 호출과 액션 문자열을 지운다. `activate_install`의 `reviewed` 인자는 항상 `false`가 되므로 인자를 없애고 `reviewed`로만 들어가던 분기를 지운다 |
| `devbox_manager_lib::component::{legacy_installations,preview_legacy_portable,cleanup_legacy_portable}` | 호출하던 Control Center 코드와 함께 지운다 |
| installation-tools `component.rs`의 `inspect_local_quality`·`inspect_data_databases`·`preview_data_query`·`cancel_data_diagnostics`·`export_data_preview` | 허용 목록·dispatch에서 지운다 |
| 번들·doctor가 쓰던 `data_inspector::{redact_text, REDACTION_VERSION, is_link_or_reparse, safe_derived_path}` | `core/redaction.rs`로 옮겨 쓴다(아래 Task 4) |
| 프런트에서 쓰이지 않게 된 state·handler·import | `noUnusedLocals`가 알려 주는 대로 지운다. 쓰이지 않는 export는 `rg`로 찾아 지운다 |

---

### Task 1: 끝 상태 guard

**Files:** Create `.github/scripts/check-no-legacy.py`, `.github/scripts/test-check-no-legacy.py`

**Interfaces (Produces):** `check-no-legacy.py [--scope control-center|products|all]` — 금지 패턴이 범위 안에 있으면 경로:행을 출력하고 1로 끝난다. P1-03~P1-05가 범위와 패턴을 넓힌다.

- [ ] **Step 1: guard와 테스트 작성**

`.github/scripts/check-no-legacy.py`:

```python
#!/usr/bin/env python3
"""Fail when v0.7 import/cleanup code remains in a cleaned scope."""
from __future__ import annotations
import argparse, pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCOPES: dict[str, list[str]] = {
    "control-center": [
        "apps/devbox-control-center",
        "crates/installation-tools",
        "packages/control-center-features",
    ],
}
PATTERNS = [
    r"legacy[-_]v0\.7", r"legacy-v0\.7-catalog", r"\blegacy_cleanup\b", r"\blauncher_import\b",
    r"\bLegacyCleanup\b", r"\bLegacyInventory\b", r"\bLauncherImport\b", r"\bMigrationOwners\b",
    r"\bcutover_review\b", r"\bprepare_cutover\b", r"\brecord_migration_owner\b",
    r"\blegacy_installations\b", r"\bcleanup_legacy_portable\b", r"\binspect_data_databases\b",
    r"\binspect_local_quality\b", r"com\.devbox\.devboxmanager",
]
SUFFIXES = {".rs", ".ts", ".tsx", ".json", ".toml", ".mjs", ".ps1", ".py"}

def scan(paths: list[str]) -> list[str]:
    compiled = [re.compile(pattern) for pattern in PATTERNS]
    hits = []
    for relative in paths:
        base = ROOT / relative
        for path in ([base] if base.is_file() else base.rglob("*")):
            if not path.is_file() or path.suffix not in SUFFIXES or "node_modules" in path.parts or "target" in path.parts:
                continue
            for number, line in enumerate(path.read_text(encoding="utf-8", errors="ignore").splitlines(), 1):
                if any(pattern.search(line) for pattern in compiled):
                    hits.append(f"{path.relative_to(ROOT)}:{number}: {line.strip()[:120]}")
    return hits

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scope", default="all", choices=[*SCOPES, "all"])
    args = parser.parse_args()
    paths = [p for name, values in SCOPES.items() if args.scope in (name, "all") for p in values]
    hits = scan(paths)
    print("\n".join(hits) if hits else f"no v0.7 legacy code in scope {args.scope}")
    return 1 if hits else 0

if __name__ == "__main__":
    sys.exit(main())
```

`.github/scripts/test-check-no-legacy.py`:

```python
#!/usr/bin/env python3
import importlib.util, pathlib, tempfile, unittest

SPEC = importlib.util.spec_from_file_location("guard", pathlib.Path(__file__).with_name("check-no-legacy.py"))
guard = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(guard)

class GuardTest(unittest.TestCase):
    def test_flags_forbidden_names_and_ignores_other_files(self):
        with tempfile.TemporaryDirectory() as root:
            base = pathlib.Path(root)
            (base / "a.rs").write_text('fn x() { legacy_cleanup::run(); }\n')
            (base / "b.md").write_text("legacy_cleanup in docs is fine\n")
            (base / "c.ts").write_text("export const ok = 1;\n")
            original = guard.ROOT
            guard.ROOT = base
            try:
                hits = guard.scan(["."])
            finally:
                guard.ROOT = original
            self.assertEqual(len(hits), 1)
            self.assertTrue(hits[0].startswith("a.rs:1:"))

if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: 실패 확인** — Run: `python3 .github/scripts/test-check-no-legacy.py && python3 .github/scripts/check-no-legacy.py --scope control-center`
Expected: 테스트 PASS, guard는 FAIL(다수의 hit).

- [ ] **Step 3: CI 연결** — `.github/workflows/ci.yml`의 Python 검사 목록(`python3 .github/scripts/check-dependencies.py check` 줄들이 있는 step)에 두 줄을 추가한다: `python3 .github/scripts/test-check-no-legacy.py`, `python3 .github/scripts/check-no-legacy.py --scope control-center`. 이 PR이 끝날 때 통과해야 한다.

- [ ] **Step 4: 커밋** — `git add .github && git commit -m "test(devbox-control-center): add a v0.7 legacy guard"`

---

### Task 2: Control Center 화면과 카탈로그

**Files:** `apps/devbox-control-center/src/Content.tsx`(P0-04에서 만든 파일), 삭제 목록의 화면, `Restore.tsx`, `apps/products.json`, `crates/product-shell-tauri/src/lib.rs`, `packages/product-shell/src/index.tsx`

- [ ] **Step 1: 실패하는 테스트** — `apps/devbox-control-center/src/App.test.tsx`(P0-04에서 `it.each(routes)`로 바꾼 파일)에 추가한다.

```tsx
it("has no migration route and sends pending installs to recovery", () => {
  expect(catalog.features.some((feature) => feature.route === "migration")).toBe(false);
  expect(catalog.features.filter((feature) => feature.owner === "control-center").map((feature) => feature.route))
    .toEqual(["products", "updates", "components", "environment", "recovery", "shortcuts", "diagnostics", "tools"]);
});
```

  (`catalog`은 파일 상단의 `import catalog from "../../../apps/products.json";`. 없으면 추가한다.)

  `apps/devbox-control-center/src/Restore.test.tsx`에 추가한다(기존 테스트의 native mock·렌더 도우미를 그대로 쓴다. 도우미 이름은 파일 상단에서 확인).

```tsx
it("offers only the clean activation path for a pending install", async () => {
  // 기존 도우미로 installation.phase가 "import"인 inventory를 돌려주게 한 뒤 렌더한다.
  expect(await screen.findByRole("button", { name: "신규 설치 활성화 준비" })).toBeInTheDocument();
  expect(screen.queryByText("이전 검토를 반영한 설치 확정")).toBeNull();
  expect(screen.queryByText("데이터를 보존하고 이전 검토로 돌아가기")).toBeNull();
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter devbox-control-center exec vitest run src/App.test.tsx src/Restore.test.tsx` → FAIL.

- [ ] **Step 3: 구현**
  - 삭제 목록의 화면 6개와 테스트를 지운다.
  - `Content.tsx`: `MigrationOwners`·`Backups`·`LegacyInventory`·`LauncherImport`·`Cutover`의 lazy import와 사용을 지운다. `!available` 분기는 `<><Inventory {...props}/><Recovery {...props}/><Health {...props}/></>`만 렌더한다("데이터 이전 화면 열기" 버튼 삭제). 일반 분기의 `props.route==="migration"?…` 삼항을 지운다.
  - `Restore.tsx`: `Action` 유니온과 `actionLabels`에서 `reviewImportAgain`·`commitReviewed`를 지우고, 이 두 액션을 보여 주던 JSX를 지운다.
  - `apps/products.json`: `features`에서 `control-center.migration` 항목을 지우고 `catalogRevision`을 1 올린다. `rg -n "catalogRevision\"?:? ?[0-9]+|catalog_revision: [0-9]+|revision: [0-9]+" apps packages crates --glob '!**/node_modules/**'`로 테스트·fixture에 박힌 옛 값을 찾아 새 값으로 바꾼다(`packages/product-shell/fixtures/*.json` 포함).
  - `crates/product-shell-tauri/src/lib.rs` `run_with`의 setup route 매핑에서 `Phase::Import => "migration"`을 `Phase::Import => "recovery"`로 바꾼다.
  - `packages/product-shell/src/index.tsx`의 `deliveryState === "import"` 안내 문구를 "설치를 마무리하고 있습니다. Control Center의 데이터 및 복구 화면에서 활성화를 완료해 주세요. 일반 작업과 자동 실행은 대기합니다."로 바꾼다.

- [ ] **Step 4: 통과 확인** — Run: `pnpm --filter devbox-control-center exec vitest run && pnpm --filter @devbox/product-shell exec vitest run && cargo test -p product-shell-tauri --lib` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-control-center): remove the migration route and legacy screens"`

---

### Task 3: Control Center native

**Files:** 삭제 목록의 native 파일, `apps/devbox-control-center/src-tauri/src/{lib.rs,tools_host.rs,bootstrap.rs,bootstrap/interactive.rs,core/mod.rs}`, `apps/devbox-control-center/src-tauri/tauri.conf.json`

- [ ] **Step 1: 실패하는 테스트** — `tools_host.rs` 테스트 모듈(없으면 `#[cfg(test)] mod tests { use super::*; }`를 만든다)에 추가한다. 허용 판정이 `execute` 안의 지역 변수라면, 먼저 메서드 분류를 `fn component_for(method: &str) -> Option<&'static str>`(허용되지 않으면 `None`)로 뽑아낸 뒤 테스트한다.

```rust
    #[test]
    fn legacy_methods_are_no_longer_routed() {
        for method in [
            "legacy_cleanup_preview", "legacy_cleanup_apply", "legacy_cleanup_list",
            "cutover_review", "prepare_cutover", "legacy_inventory", "record_migration_owner",
        ] {
            assert_eq!(component_for(method), None, "{method}");
        }
        for method in ["suite_inventory", "record_suite_health", "check_suite_update", "restore_inventory"] {
            assert_eq!(component_for(method), Some("control-center.delivery"), "{method}");
        }
    }
```

  `bootstrap/interactive.rs` 테스트 모듈에 액션 목록 테스트를 추가한다(액션 match를 `fn known_action(name: &str) -> bool`로 뽑아 쓴다).

```rust
    #[test]
    fn only_clean_activation_remains() {
        assert!(known_action("activateClean") && known_action("commitClean"));
        for removed in ["activateReviewed", "commitReviewed", "reviewImportAgain"] {
            assert!(!known_action(removed), "{removed}");
        }
    }
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-control-center --lib tools_host bootstrap::interactive` → FAIL(함수 없음 또는 단언 실패).

- [ ] **Step 3: 삭제와 정리**
  - 삭제 목록의 native 파일을 `git rm`으로 지우고 `lib.rs`·`core/mod.rs`·`bootstrap.rs`의 `mod` 선언을 정리한다.
  - `tauri.conf.json`의 `bundle.resources`에서 `legacy-v0.7-*.json` 항목을 지운다.
  - `cargo check -p devbox-control-center` 오류를 "정리 규칙"대로 고친다. `tools_host::execute`는 `component_for`를 써서 `control-center.delivery`(suite_inventory·open_installation_folder·suite_recovery·restore_inventory·restore_action·record_suite_health·업데이트 5종) 또는 `control-center.tools`(그 밖의 manager tools)를 고르고, 둘 다 아니면 `ProblemCode::InvalidRequest`로 거부한다.
  - `bootstrap/interactive.rs`의 액션 match를 `known_action`과 같은 목록으로 맞춘다. 같은 파일의 설치 상태 계산에서 `no_legacy_data`·`no_legacy_installers`와 legacy catalog 읽기를 지우고 `clean`은 나머지 조건(`previous`·`imports`·`backup`·`failure`가 비었고 owner evidence 4개가 준비됨)만 본다. 응답 JSON의 `"reviewed"` 필드를 지운다.
  - `bootstrap.rs` `activate_install`: `reviewed` 인자와 `cutover::hold`, `bootstrap_legacy_registration_review_required` 검사(설치 프로그램 등록 조회), legacy catalog 폴더 존재 검사를 지운다. owner evidence 4개·`setup_selected`·`!busy`·`!review_required` 검사와 `imports`·`backup`이 비어 있어야 한다는 검사는 남긴다.
  - `migration_evidence.rs`를 `git mv`로 `owner_evidence.rs`로 바꾸고 아래처럼 줄인다: `capture_sources`·백업 목록(`catalog`, `suite::health::backups`, `suite::health::sources`) 호출을 지운다. `record(app, owner, deadline)`는 activation 단계가 `Import`이면 `OwnerEvidence { sources: None, summary: before.store, backups: Vec::new() }`를 `journal.record_owner`로, `Health`이면 지금처럼 `journal.record_health`로 기록하고, 그 밖의 단계는 `Err("suite_owner_phase_invalid")`. 단계 판정은 테스트할 수 있게 떼어 낸다.

```rust
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EvidenceKind {
    Owner,
    Health,
}

pub(crate) fn evidence_kind(
    phase: Option<product_contract::activation::Phase>,
) -> Result<EvidenceKind> {
    use product_contract::activation::Phase;
    match phase {
        Some(Phase::Import) => Ok(EvidenceKind::Owner),
        Some(Phase::Health) => Ok(EvidenceKind::Health),
        _ => Err("suite_owner_phase_invalid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::activation::Phase;

    #[test]
    fn the_activation_phase_selects_what_is_recorded() {
        assert_eq!(evidence_kind(Some(Phase::Import)), Ok(EvidenceKind::Owner));
        assert_eq!(evidence_kind(Some(Phase::Health)), Ok(EvidenceKind::Health));
        assert!(evidence_kind(Some(Phase::Committed)).is_err());
        assert!(evidence_kind(None).is_err());
    }
}
```

  - `tools_host.rs`: `record_migration_owner`를 지우고 `record_suite_health`가 `crate::owner_evidence::record`를 부르게 한다(허용 route는 `updates`·`recovery`). `apps/devbox-control-center/src/Health.tsx`는 이미 `record_suite_health`를 부르므로 문구만 "상태 기록"으로 통일하고, import 단계에서도 버튼이 보이게 조건을 확인한다(`Health`는 `Content.tsx`의 `!available` 분기에서 이미 렌더된다).

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-control-center --lib && cargo clippy -p devbox-control-center --all-targets -- -D warnings` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-control-center): drop v0.7 discovery, cutover and cleanup natives"`

---

### Task 4: installation-tools 정리

**Files:** 삭제 목록의 installation-tools 파일, `src/core/redaction.rs`(생성), `src/core/mod.rs`, `src/commands/{mod,doctor,diagnostics}.rs`, `src/core/support_bundle.rs`, `src/component.rs`, `packages/control-center-features/src/manager/*`

**Interfaces (Produces):** `core::redaction::{REDACTION_VERSION, redact_text(&str, &str) -> String, is_link_or_reparse(&Metadata) -> bool, safe_derived_path(&Path, &Path) -> bool}`(동작은 옮기기 전과 같음), `build_bundle(diagnosis, products: Vec<SupportProduct>, operations, cancel)`

- [ ] **Step 1: 실패하는 테스트** — `support_bundle.rs` 테스트 모듈의 기존 테스트를 새 서명으로 고치고 추가한다.

```rust
    #[test]
    fn bundle_lists_suite_products_and_no_legacy_sections() {
        let draft = build_bundle(
            vec![SupportDiagnostic { name: "git".into(), ok: true, detail: "git 2.50 /home/alice".into() }],
            vec![SupportProduct { id: "knowledge".into(), version: "0.9.0".into() }],
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let text = String::from_utf8(draft.bytes).unwrap();
        assert!(text.contains("\"products\""));
        assert!(!text.contains("\"databases\"") && !text.contains("\"catalog\""));
        assert!(!text.contains("/home/alice"));
        assert!(text.contains("\"schemaVersion\": 3"));
    }
```

  `doctor.rs`에 테스트를 추가한다(legacy 항목을 만드는 코드가 사라졌는지 소스로 고정한다. 찾는 문자열을 실행 중에 조립해 테스트 자신이 걸리지 않게 한다).

```rust
    #[test]
    fn doctor_has_no_v07_catalog_checks() {
        let source = include_str!("doctor.rs");
        for name in ["devbox-data", "catalog-ids", "runtime-metadata"] {
            let needle = format!("name: \"{name}\".into()");
            assert!(!source.contains(&needle), "{name}");
        }
        assert!(!source.contains(&["CATALOG", "_JSON"].concat()));
    }
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-installation-tools --lib` → FAIL.

- [ ] **Step 3: 구현**
  - `core/redaction.rs`를 만들고 `data_inspector.rs`에서 `REDACTION_VERSION`, `redact_text`, `is_link_or_reparse`, `safe_derived_path`와 이들이 쓰는 private 도우미·테스트를 그대로 옮긴다. `core/mod.rs`에 `pub mod redaction;`.
  - 삭제 목록의 installation-tools 파일을 지우고 `core/mod.rs`·`commands/mod.rs`를 정리한다.
  - `doctor.rs`: `CATALOG_JSON` 상수, `devbox-data`·`catalog-ids`·`runtime-metadata` 항목을 지운다(docker·git·wsl 등 환경 항목은 유지).
  - `support_bundle.rs`: 문서 구조를 `{schemaVersion: 3, generatedAtMs, redaction, diagnosis, products: [{id, version}], operations, omitted}`로 줄인다. `catalog`·`databases`·`logs` 절과 `LogMetadata`·`collect_log_metadata`·`current_source_revision`·`source_revision`을 지우고, `BundleDraft.source_revision`은 빈 문자열이 아니라 필드째 없앤다(미리보기와 내보내기 사이에 다시 계산할 원본이 없다). `SupportProduct { id: String, version: String }`를 추가한다.
  - `diagnostics.rs`: `inspect_data_databases`·`preview_data_query`·`cancel_data_diagnostics`·`export_data_preview`와 관련 state(`StoredQueryPreview` 등)를 지운다. `preview_support_bundle`은 `catalog()`·`data_root()` 대신 제품 카탈로그로 `SupportProduct { id, version: app.package_info().version.to_string() }` 네 개를 만들어 넘긴다(Suite 제품은 같은 버전). `export_support_bundle`의 source revision 비교를 지운다. `included_sections`는 `["diagnosis", "products", "operation-log"]`.
  - `component.rs`: 허용 목록과 dispatch에서 지운 메서드를 뺀다. `legacy_installations`·`preview_legacy_portable`·`cleanup_legacy_portable`·`LegacyPortablePreview` 재수출을 지운다.
  - `packages/control-center-features/src/manager`: `ToolsMode`를 `"doctor" | "dev-setup" | "related-tools"`로 줄이고 `"apps"` 탭·`local-quality`·Data Inspector UI와 그 state·handler를 지운다. `api.ts`·`types.ts`에서 쓰이지 않게 된 함수·타입(`inspectLocalQuality`, `inspectDataDatabases`, `previewDataQuery`, `exportDataPreview`, 앱 설치·제거·롤백 계열)을 지운다. `App.test.tsx`·`api.test.ts`에서 지운 기능의 테스트를 지운다. 지원 번들 mock의 `includedSections`를 `["diagnosis", "products", "operation-log"]`로.
  - `Cargo.toml`에서 쓰지 않게 된 의존성(`rusqlite`, `semver`, `reqwest`, `futures-util`, `devbox-launch`, `serde_yaml_ng` 등)을 `cargo machete`가 없으면 `rg -n "<crate>::" crates/installation-tools/src`로 하나씩 확인해 지운다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-installation-tools --lib && cargo test -p devbox-control-center --lib && pnpm --filter @devbox/control-center-features exec vitest run && pnpm --filter @devbox/control-center-features exec tsc --noEmit` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-control-center): reduce manager tools to diagnosis, setup and related tools"`

---

### Task 5: CI·문서 정리

- [ ] **Step 1: workflow**
  - `.github/workflows/product-foundation.yml`: `legacy_reference_only` 입력과 `legacy-reference` job, paths의 삭제된 스크립트 항목, 모든 `if:`의 `!inputs.legacy_reference_only &&` 조각을 지운다. "reviewed source cutover and legacy cleanup" 설명 문구를 "updated uninstall and restore/reinstall"로 바꾼다.
  - `.github/scripts/windows-suite-delivery.ps1`·`windows-suite-delivery-native.mjs`: 출처별 전환(`cutover`, `activateReviewed`, `commitReviewed`, `reviewImportAgain`)과 legacy 설치 fixture 단계를 지우고, 새 설치는 `activateClean` → 네 제품 health 기록 → `commitClean` 순서로 확정하게 바꾼다(기존 파일에 clean 경로 단계가 있으면 그것만 남긴다).
  - `check-source-cutover.py`는 P1-05에서 새 guard로 대체한다. 이 PR에서는 Control Center 관련 단언만 통과하도록 둔다.
- [ ] **Step 2: 문서** — `apps/devbox-control-center/README.md`와 `docs/windows-guide.md`의 "v0.7에서 이전"·기존 앱 정리 절을 지우고 "새 설치는 설치 후 Control Center의 데이터 및 복구 화면에서 활성화를 확정한다"로 바꾼다. `docs/features/control-center.migration.md`를 지운다.
- [ ] **Step 3: 확인** — Run: `python3 .github/scripts/check-no-legacy.py --scope control-center && python3 .github/scripts/test-product-foundation-workflow.py && python3 .github/scripts/test-windows-installer-acceptance-config.py && python3 .github/scripts/test-windows-packaged-smoke-config.py` → PASS(삭제한 legacy config를 읽던 테스트 케이스는 Task 5 Step 1에서 함께 지운다).
- [ ] **Step 4: 커밋** — `git add -A && git commit -m "ci(devbox-control-center): drop v0.7 acceptance and reference jobs"`

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. 이 PR은 `pnpm verify:all`로 검증한다(CI 검증기 변경 포함).
- [ ] 설치·업데이트 acceptance가 새 clean 경로로 통과하는지는 묶음 B3 PR의 `Product foundation acceptance` 결과로 확인하고 ledger에 남긴다(§4.7). (릴리스 후보 workflow는 제품 버전과 태그가 같아야 하므로 최종 릴리스 P3-01에서만 쓴다.)
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기 — P3-01 후보 점검표에서 확인): 회사 PC에 새로 설치 → Control Center가 "데이터 및 복구"로 열리고 활성화 준비 → 네 제품 열기·닫기 → 확정까지 된다.
