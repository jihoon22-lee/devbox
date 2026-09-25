# P1-06 Cargo 의존성 통일과 빌드 프로필 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 외부 의존성 버전·feature를 루트 `[workspace.dependencies]` 한 곳에서 정하고, `windows` crate를 0.61.3 하나로 줄이며, 개발 빌드의 debug 정보를 줄여 빌드 시간과 `target` 크기를 낮춘다(P4).

**Architecture:** 두 개 이상의 member가 쓰는 외부 crate는 루트에 버전과 **모든 member feature의 합집합**을 적고, member는 `{ workspace = true }`(필요하면 `optional = true`)만 쓴다. 합집합은 전체 빌드에서 이미 Cargo가 만들던 조합이므로 동작이 바뀌지 않고, `cargo test -p <crate>`처럼 일부만 빌드할 때 feature 조합이 달라 다시 컴파일되던 일이 사라진다. `windows` 0.58을 쓰는 세 crate(`git`, `devbox-editor-engine`, `devbox-activity-engine`)는 0.61 API로 옮긴다. 규칙이 다시 깨지지 않게 `check-workspace-deps.py`를 CI에 넣는다.

**Tech Stack:** Cargo, Rust, Python

**Spec:** `review.md` §5 P4

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 새 외부 의존성을 추가하지 않는다. 버전은 **올리지 않고** 지금 lockfile의 해석 결과에 맞춘다(예외: `windows` 0.58 → 0.61.3, `sha2` 0.10 → 0.11.0 한 곳).
- 내부 path 의존성(`devbox-filesystem = { package = "filesystem", path = … }` 등)은 이 PR에서 바꾸지 않는다(별칭이 crate마다 달라 `workspace = true`로 합치면 코드 이름이 바뀐다).
- `[profile.release]`(`lto = true`, `strip = true`)는 그대로 둔다.

## Review Focus

1. 통일 뒤 어떤 crate가 원래 쓰지 않던 feature를 얻어 동작이 달라짐 → 합집합은 전체 빌드에서 이미 켜져 있던 조합이다. `cargo tree -e features -i <crate>`로 전후를 비교해 PR 본문에 적는다. (Task 1)
2. `windows` 0.61 이전 뒤 Windows 전용 코드 컴파일 오류 → Linux 개발 환경에서 보이지 않는다. Windows target check와 CI `Rust (Windows)`로 확인한다. (Task 2)
3. `sha2` 0.11로 올린 installation-tools의 digest 출력 형식이 바뀜 → 기존 테스트의 hex 기대값이 그대로 통과해야 한다. (Task 1)
4. 새 member가 버전을 직접 적어 규칙이 다시 깨짐 → `check-workspace-deps.py`가 CI에서 실패시킨다. (Task 3)
5. `debug = "line-tables-only"`로 panic backtrace의 파일:행이 사라짐 → line table은 남으므로 backtrace 위치는 유지된다(변수 값 디버깅만 줄어든다). (Task 3)

## Branch · PR

- 묶음: **B3** — 브랜치 `refactor/suite/archive-and-remove-v07`, PR 제목 `refactor(suite): archive history, remove v0.7 imports and unify dependencies`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `chore(workspace): unify dependency versions and features`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: `[workspace.dependencies]`

**Files:** 루트 `Cargo.toml`, 외부 의존을 가진 모든 member `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1: 기준 기록** — Run:

```bash
source ~/.cargo/env
python3 - <<'EOF' > /tmp/deps-before.txt
import re, collections
text = open("Cargo.lock").read()
pkgs = re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', text)
count = collections.Counter(name for name, _ in pkgs)
print("duplicated", sum(1 for v in count.values() if v > 1))
for name in ["windows", "windows-sys", "sha2", "reqwest", "tokio", "rusqlite"]:
    print(name, sorted(v for n, v in pkgs if n == name))
EOF
cat /tmp/deps-before.txt
```

- [ ] **Step 2: 합집합 계산** — 아래 스크립트로 crate별 버전·feature 합집합을 뽑는다(결과를 Step 3 표와 대조한다).

```bash
python3 - <<'EOF'
import tomllib, pathlib, collections
members = tomllib.load(open("Cargo.toml", "rb"))["workspace"]["members"]
table = collections.defaultdict(lambda: {"versions": set(), "features": set(), "no_default": set(), "users": 0})
for member in members:
    manifest = tomllib.load(open(pathlib.Path(member) / "Cargo.toml", "rb"))
    sections = [manifest.get("dependencies", {}), manifest.get("dev-dependencies", {}), manifest.get("build-dependencies", {})]
    for target in manifest.get("target", {}).values():
        sections += [target.get("dependencies", {}), target.get("dev-dependencies", {})]
    for deps in sections:
        for name, spec in deps.items():
            if isinstance(spec, dict) and ("path" in spec or "workspace" in spec):
                continue
            entry = table[name]
            entry["users"] += 1
            if isinstance(spec, str):
                entry["versions"].add(spec)
            else:
                entry["versions"].add(spec.get("version", "?"))
                entry["features"].update(spec.get("features", []))
                if spec.get("default-features") is False:
                    entry["no_default"].add(member)
for name, entry in sorted(table.items()):
    if entry["users"] > 1:
        print(f"{name}: versions={sorted(entry['versions'])} features={sorted(entry['features'])} no_default={sorted(entry['no_default'])}")
EOF
```

- [ ] **Step 3: 루트에 추가** — 루트 `Cargo.toml`의 `[workspace.package]` 아래에 추가한다. 버전은 Step 2 결과 중 lockfile이 실제로 해석한 버전을 적는다(아래는 현재 기준 예시이며, Step 2 결과와 다르면 Step 2를 따른다). `default-features = false`인 member가 하나라도 있는 crate는 루트에 `default-features = false`를 두고, 기본 feature가 필요한 member는 `features = [...]`로 기본 feature를 명시한다.

```toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11.0"
uuid = { version = "1.24.0", features = ["v4", "serde"] }
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-dialog = "2"
tauri-plugin-opener = "2"
tauri-plugin-clipboard-manager = "2"
tokio = { version = "1.53.1", features = ["fs", "io-std", "io-util", "macros", "net", "process", "rt", "sync", "time"] }
rusqlite = { version = "0.32", features = ["bundled", "backup", "hooks", "limits", "modern_sqlite"] }
reqwest = { version = "0.13.4", features = ["brotli", "deflate", "form", "gzip", "json", "multipart", "rustls", "stream", "zstd"] }
windows = { version = "0.61.3", features = [
    "Win32_Foundation", "Win32_Security", "Win32_Storage_FileSystem", "Win32_System_Com",
    "Win32_System_Console", "Win32_System_Diagnostics_ToolHelp", "Win32_System_JobObjects",
    "Win32_System_Pipes", "Win32_System_Registry", "Win32_System_SystemInformation",
    "Win32_System_Threading", "Win32_UI_Input_KeyboardAndMouse", "Win32_UI_Shell",
    "Win32_UI_WindowsAndMessaging",
] }
regex = "1.13.1"
base64 = "0.22.1"
dirs = "6.0.0"
chrono = { version = "0.4", default-features = false, features = ["alloc", "std"] }
tempfile = "3"
notify = "8.2.0"
sysinfo = "0.39.6"
url = "2"
futures-util = { version = "0.3.31", default-features = false, features = ["sink", "std"] }
zeroize = { version = "1", features = ["zeroize_derive"] }
getrandom = "0.4.3"
```

  - `windows` feature 목록은 Step 2가 뽑은 **모든 member의 windows feature 합집합**으로 채운다(0.58을 쓰던 세 crate의 feature 포함).
  - `chrono`처럼 member마다 `clock` 같은 추가 feature가 필요한 crate는 member에서 `chrono = { workspace = true, features = ["clock"] }`로 더한다.
  - `tauri`에 `tray-icon`을 넣는 것은 합집합이다. tray를 쓰지 않는 crate도 전체 빌드에서 이미 이 feature로 컴파일되고 있다.

- [ ] **Step 4: member 바꾸기** — 각 member의 해당 줄을 `name = { workspace = true }`로 바꾼다. 원래 `optional = true`면 `{ workspace = true, optional = true }`, 합집합에 없는 feature를 원래 켰다면 그 feature를 `features = […]`로 남긴다(합집합에 넣었다면 지운다). `sha2 = "0.10"`(installation-tools)도 `workspace = true`로.

- [ ] **Step 5: 확인** — Run: `cargo metadata --format-version 1 >/dev/null && cargo check --workspace --all-targets && cargo test -p devbox-installation-tools --lib && python3 .github/scripts/check-dependencies.py check` → PASS. `check-dependencies.py`가 member manifest의 버전 문자열을 읽어 검사한다면 `workspace = true` 항목을 루트 표에서 해석하도록 고친다(스크립트 안의 manifest 읽는 함수에 "`workspace = true`면 `[workspace.dependencies][name]`을 본다" 분기를 추가하고 `test-check-dependencies.py`에 케이스를 추가한다).

- [ ] **Step 6: 커밋** — `git add -A && git commit -m "chore(workspace): declare shared dependencies once"`

---

### Task 2: `windows` 0.61 단일화

**Files:** `crates/git`, `crates/editor-engine`, `crates/activity-engine`의 `Cargo.toml`과 Windows 전용 소스(`#[cfg(windows)]`, `platform/`)

- [ ] **Step 1: Windows target 준비** — Run: `rustup target add x86_64-pc-windows-msvc`. 이후 `cargo check --target x86_64-pc-windows-msvc -p git -p devbox-editor-engine -p devbox-activity-engine`로 확인한다. C 컴파일이 필요한 의존(`rusqlite` bundled 등) 때문에 build script가 실패하면, 그 crate는 Step 4에서 CI `Rust (Windows)` 결과로 확인한다(PR 본문에 적는다).

- [ ] **Step 2: 버전 변경** — 세 crate의 `windows = { version = "0.58", … }`를 `windows = { workspace = true }`로 바꾼다.

- [ ] **Step 3: 컴파일 오류를 규칙대로 고친다** — 0.58 → 0.61의 흔한 변화:

| 0.58 코드 | 0.61 코드 |
|---|---|
| `windows::Win32::Foundation::BOOL` 반환을 `.as_bool()`로 검사 | 대부분 `windows::core::Result<()>`를 반환한다. `?`나 `.is_ok()`로 바꾼다. 여전히 `BOOL`인 함수는 `windows::core::BOOL`을 import한다 |
| `HANDLE(isize)` 생성·비교 | `HANDLE(*mut c_void)`. `HANDLE(value as _)`, 비교는 `handle.is_invalid()` |
| `CloseHandle(h)`의 `BOOL` | `CloseHandle(h)?` 또는 `let _ = CloseHandle(h);` |
| `PCWSTR::from_raw(ptr)` | 같음. `w!("...")` 매크로 경로는 `windows::core::w` |
| `GetLastError()`의 `WIN32_ERROR` | `windows::core::Error::from_win32()` 또는 `GetLastError()`가 `WIN32_ERROR`를 반환하는 그대로 사용 |

  같은 저장소의 0.61 사용 예(`crates/filesystem`, `crates/process`, `crates/suite-runtime`)를 참고해 같은 관용구로 맞춘다.

- [ ] **Step 4: 확인** — Run: `cargo check --workspace --all-targets && cargo check --target x86_64-pc-windows-msvc -p git -p devbox-editor-engine`(가능한 crate만) → PASS. `Cargo.lock`에서 `windows` 0.58이 사라졌는지 본다: `grep -A1 '^name = "windows"$' Cargo.lock`(0.62가 tauri 등 외부 의존으로 남는 것은 괜찮다).

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "chore(workspace): move remaining crates to windows 0.61"`

---

### Task 3: 규칙 검사·프로필·문서

**Files:** Create `.github/scripts/check-workspace-deps.py`, `.github/scripts/test-check-workspace-deps.py`; Modify 루트 `Cargo.toml`, `.github/workflows/ci.yml`, `docs/verification.md`

- [ ] **Step 1: 실패하는 테스트** — `test-check-workspace-deps.py`

```python
#!/usr/bin/env python3
import importlib.util, pathlib, tempfile, textwrap, unittest

SPEC = importlib.util.spec_from_file_location("deps", pathlib.Path(__file__).with_name("check-workspace-deps.py"))
deps = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(deps)

class WorkspaceDepsTest(unittest.TestCase):
    def write(self, root, relative, text):
        path = pathlib.Path(root) / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text))

    def test_members_must_use_workspace_versions(self):
        with tempfile.TemporaryDirectory() as root:
            self.write(root, "Cargo.toml", """
                [workspace]
                members = ["a"]
                [workspace.dependencies]
                serde = "1"
            """)
            self.write(root, "a/Cargo.toml", """
                [package]
                name = "a"
                [dependencies]
                serde = "1.0.200"
                local = { path = "../local" }
            """)
            self.assertEqual(deps.violations(pathlib.Path(root)), ["a/Cargo.toml: serde must use workspace = true"])

    def test_workspace_usage_passes(self):
        with tempfile.TemporaryDirectory() as root:
            self.write(root, "Cargo.toml", """
                [workspace]
                members = ["a"]
                [workspace.dependencies]
                serde = "1"
            """)
            self.write(root, "a/Cargo.toml", """
                [package]
                name = "a"
                [dependencies]
                serde = { workspace = true }
            """)
            self.assertEqual(deps.violations(pathlib.Path(root)), [])

if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: 실패 확인** — Run: `python3 .github/scripts/test-check-workspace-deps.py` → FAIL(모듈 없음).

- [ ] **Step 3: 구현** — `check-workspace-deps.py`

```python
#!/usr/bin/env python3
"""External crates declared in [workspace.dependencies] must be used through workspace = true."""
from __future__ import annotations
import pathlib, sys, tomllib

def sections(manifest: dict):
    yield from (manifest.get(key, {}) for key in ("dependencies", "dev-dependencies", "build-dependencies"))
    for target in manifest.get("target", {}).values():
        yield from (target.get(key, {}) for key in ("dependencies", "dev-dependencies", "build-dependencies"))

def violations(root: pathlib.Path) -> list[str]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    shared = set(workspace.get("dependencies", {}))
    found = []
    for member in workspace["members"]:
        path = root / member / "Cargo.toml"
        manifest = tomllib.loads(path.read_text())
        for deps in sections(manifest):
            for name, spec in deps.items():
                if name not in shared:
                    continue
                if isinstance(spec, dict) and spec.get("workspace") is True:
                    continue
                found.append(f"{path.relative_to(root)}: {name} must use workspace = true")
    return found

def windows_versions(root: pathlib.Path) -> set[str]:
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    return {package["version"] for package in lock["package"] if package["name"] == "windows"}

if __name__ == "__main__":
    root = pathlib.Path(__file__).resolve().parents[2]
    problems = violations(root)
    if "0.58.0" in windows_versions(root):
        problems.append("Cargo.lock: windows 0.58 must not return")
    print("\n".join(problems) or "workspace dependencies OK")
    sys.exit(1 if problems else 0)
```

- [ ] **Step 4: 프로필** — 루트 `Cargo.toml`의 `[profile.release]` 위에 추가한다.

```toml
# Keep file:line in backtraces while skipping full variable debug info.
[profile.dev]
debug = "line-tables-only"

[profile.test]
debug = "line-tables-only"
```

- [ ] **Step 5: CI·문서** — `ci.yml` Python 검사 목록에 `python3 .github/scripts/test-check-workspace-deps.py`와 `python3 .github/scripts/check-workspace-deps.py`를 추가한다. `docs/verification.md`의 "자원" 절에 한 단락을 추가한다: "`target`이 커지면 `cargo install cargo-sweep` 후 한 달에 한 번 `cargo sweep --time 30`으로 30일 넘게 쓰지 않은 산출물을 지운다. 전체 삭제(`cargo clean`)는 다음 빌드가 오래 걸리므로 마지막 수단이다."

- [ ] **Step 6: 통과 확인** — Run: `python3 .github/scripts/test-check-workspace-deps.py && python3 .github/scripts/check-workspace-deps.py && cargo test --workspace --lib --no-run` → PASS.

- [ ] **Step 7: 커밋** — `git add -A && git commit -m "chore(workspace): guard shared dependencies and slim dev debug info"`

---

### Task 4: PR 완료

- [ ] Step 1의 스크립트를 다시 실행해 `/tmp/deps-after.txt`를 만들고, 전후(`duplicated` 수, `windows` 버전 목록)를 PR 본문에 표로 적는다.
- [ ] `00-roadmap.md` §4.4–§4.9. 전체 빌드 설정 변경이므로 `pnpm verify:all`.
