# P0-01 작업 기반 정리 (계획서·검증 정책·툴체인·ledger) — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 이 PR은 **모든 PR의 선행 조건**이다. 이 PR이 머지되기 전에는 다른 PR을 시작하지 않는다.

**Goal:** 계획서를 저장소에 넣고, 저장소 규칙을 이번 작업 방식(D4 과제 단위 테스트, D26 workthrough 폐지, Windows 11 전용)에 맞게 고치고, Rust·Node 버전을 고정하고, 진행 원장(ledger) 이슈를 만든다.

**Architecture:** 문서·설정·CI 변경만 있다. 제품 코드는 바꾸지 않는다. CI의 툴체인 설정은 `test-github-actions-runtime.py`가 고정값과 일치하는지 검사하도록 바꿔, 이후 누가 버전을 바꿔도 한곳(`rust-toolchain.toml`, `.nvmrc`)만 고치면 되게 한다.

**Tech Stack:** Markdown, GitHub Actions YAML, Python 3, `gh` CLI

**Spec:** `00-roadmap.md` §2 결정 D4·D7·D26·D27·D28, Q1–Q3

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- Rust는 `1.98.1`(2026-09-01 stable), Node는 `24`, pnpm은 기존 `9.0.0`(corepack) 그대로.
- 이 PR은 `rust-toolchain*` 경로를 바꾸므로 `resolve-ci-scope.py`가 전체 범위(all)를 선택한다. CI가 길다(Windows 캐시 재생성 포함 40–60분).

## Review Focus

1. 워크플로 어딘가에 `node-version: 22` 또는 `dtolnay/rust-toolchain@stable`이 남음 → `test-github-actions-runtime.py`가 실패해야 한다. (Task 4)
2. `rust-toolchain.toml` 채널과 cargo-deny의 `rust-version`이 어긋남 → 테스트 실패. (Task 4)
3. 계획서 폴더가 main 체크아웃에 untracked로 남아 이후 `git pull`을 막음 → Task 1에서 이동(mv)으로 해결.
4. 정책 문구 변경 후에도 "test 실행 금지" 문장이 CONVENTIONS/verification.md에 남아 에이전트가 모순된 지시를 받음 → Task 3 Step 3의 `rg` 검사.
5. ledger 이슈 번호가 이후 PR 본문에 들어가지 않음 → 이슈 번호를 `00-roadmap.md` §6 상단에 기록(Task 5).

## Branch · PR

- 묶음: **B1** — 브랜치 `fix/suite/bootstrap-privacy-autosave-connection`, PR 제목 `fix(suite): pin toolchains and fix activity privacy, Knowledge autosave and Suite connections`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `chore(workspace): add review remediation plan, verification policy and pinned toolchains`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 |
|---|---|
| `docs/superpowers/plans/2026-09-23-review-remediation/**` | 추가(main 체크아웃의 untracked 폴더를 이동) |
| `CLAUDE.md` | 생성 — Claude Code가 AGENTS.md를 읽도록 import |
| `AGENTS.md` | 수정 — 14–16행, 27–40행, 67–68행 |
| `CONVENTIONS.md` | 수정 — §1 타깃 OS, §5 검증 bullet, §8 v0.8 정책 절, §11 workthrough bullet |
| `docs/verification.md` | 수정 — 13–26행 표와 첫 bullet |
| `rust-toolchain.toml` | 생성 |
| `.nvmrc` | 생성 |
| `package.json` | 수정 — `engines` 추가 |
| `.github/workflows/*.yml` (5개) | 수정 — Rust/Node 고정 |
| `.github/scripts/test-github-actions-runtime.py` | 수정 — 고정값 일치 검사 |

---

### Task 1: worktree 생성과 계획서 이동

- [ ] **Step 1: 준비**

```bash
cd /home/jihoon/projects/devbox
git status --short          # 계획서 폴더만 untracked(?? docs/superpowers/plans/2026-09-23-review-remediation/)여야 한다
git fetch origin main
git worktree add /home/jihoon/projects/.worktrees/devbox-bootstrap-privacy-autosave-connection \
  -b fix/suite/bootstrap-privacy-autosave-connection origin/main
```

- [ ] **Step 2: 계획서 이동** — 복사가 아니라 이동한다. main 체크아웃에 남으면 머지 후 `git pull`이 "untracked working tree files would be overwritten"으로 실패한다.

```bash
mkdir -p /home/jihoon/projects/.worktrees/devbox-bootstrap-privacy-autosave-connection/docs/superpowers/plans
mv /home/jihoon/projects/devbox/docs/superpowers/plans/2026-09-23-review-remediation \
   /home/jihoon/projects/.worktrees/devbox-bootstrap-privacy-autosave-connection/docs/superpowers/plans/
cd /home/jihoon/projects/.worktrees/devbox-bootstrap-privacy-autosave-connection
git status --short          # 계획서 폴더가 ?? 로 보여야 한다
```

- [ ] **Step 3: 커밋**

```bash
git add docs/superpowers/plans/2026-09-23-review-remediation
git commit -m "docs(workspace): add review remediation roadmap and PR plans"
```

이후 모든 PR은 이 worktree(머지 후에는 main)의 계획서를 정본으로 읽는다.

---

### Task 2: CLAUDE.md와 AGENTS.md

**Files:**
- Create: `CLAUDE.md`
- Modify: `AGENTS.md:3-6`, `AGENTS.md:14-16`, `AGENTS.md:27-40`, `AGENTS.md:67-68`

- [ ] **Step 1: CLAUDE.md 생성** — Claude Code는 `CLAUDE.md`를 자동으로 읽고 `@경로`로 다른 파일을 가져온다.

```markdown
# CLAUDE.md

이 저장소의 작업 지침 원장은 AGENTS.md와 CONVENTIONS.md다. 아래 import로 AGENTS.md를 함께 읽는다.
진행 중인 리뷰 후속 작업은 `docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md`를 따른다.

@AGENTS.md
```

- [ ] **Step 2: AGENTS.md 3–6행 교체**

```markdown
Devbox는 Windows 11용 Tauri v2·React 19·TypeScript·Rust 모노레포다.
현재 소스는 네 제품(Workspace·API Studio·Knowledge·Control Center)이다. 공개 상태는 GitHub Release를 확인한다.
원격은 `https://github.com/jihoon22-lee/devbox`다.
```

- [ ] **Step 3: AGENTS.md 14–16행 교체** (v0.8 원장 문단)

```markdown
- 진행 중인 작업의 원장은 리뷰 후속 ledger 이슈와
  `docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md`다. PR 단위·순서·게이트는
  로드맵 §4–§6을 따른다. v0.8 원장 #541·#542는 닫힌 역사 기록이다.
```

- [ ] **Step 4: AGENTS.md 27–40행 교체** (검증 시점 규칙 4개 bullet → 아래 3개)

```markdown
- **과제 단위로 필요한 테스트만 먼저 실행하고, 전체 검증은 PR 끝에 한 번 한다.** 과제를 구현할 때는
  실패하는 테스트를 먼저 쓰고 그 테스트와 직접 영향받는 테스트만 실행한다
  (`cargo test -p <crate> --lib <module>`, `pnpm --filter <package> exec vitest run <file>`).
  clippy·전체 build·`pnpm verify:affected`·Windows/WSL 실기 검증은 PR의 모든 과제가 끝난 뒤 모아 실행한다.
  `pnpm verify:all`은 release 준비·CI 검증기 변경·명시적 전체 감사에만 쓴다.
- 커밋은 과제 단위로 한다. push와 PR 생성은 PR의 상세 검증을 마친 뒤 한 번 한다
  (초안 PR은 CI를 실행하지 않는다). 완료 검증이 실패하면 확인된 수정을 먼저 모두 마치고
  실패·영향 범위만 다시 실행한다. 통과한 무관한 검사는 반복하지 않는다.
- 선행 PR에 의존하는 작업은 선행 PR 머지 후 시작한다. 파일이 겹치지 않는 독립 PR은 앞 PR의 CI를
  기다리는 동안 시작할 수 있다.
```

- [ ] **Step 5: AGENTS.md 67–68행 교체** (workthrough 규칙, D26)

```markdown
- 작업 기록은 PR 본문과 ledger 이슈 댓글로 남긴다. 새 `workthrough/` 파일은 만들지 않는다.
  결정·영향·검증 결과·미실행 실기 항목을 PR 본문에 적고, 머지 후 ledger에 요약을 남긴다.
  컨텍스트 인계는 CONVENTIONS §11, 개인 설정은 [Codex setup](./docs/codex-setup.md)을 따른다.
```

- [ ] **Step 6: 커밋**

```bash
git add CLAUDE.md AGENTS.md
git commit -m "docs(workspace): adopt task-level tests and ledger records in agent guide"
```

---

### Task 3: CONVENTIONS.md와 docs/verification.md

**Files:**
- Modify: `CONVENTIONS.md:28`, `CONVENTIONS.md:171-186`, `CONVENTIONS.md:267-285`, `CONVENTIONS.md:386-389`
- Modify: `docs/verification.md:13-22`

- [ ] **Step 1: CONVENTIONS.md**
  - 28행 표: `| 타깃 OS | Windows 10/11 (WebView2 내장) |` → `| 타깃 OS | Windows 11 (WebView2 내장) |`
  - 171–186행(§5의 "검증은 커밋 횟수에…"부터 "실행 시점·범위는 [검증 운영]…"까지 4개 bullet)을 아래로 교체한다.

```markdown
- 과제(task) 단위로 실패하는 테스트를 먼저 쓰고, 그 테스트와 직접 영향받는 테스트만 실행한다.
  예: `cargo test -p devbox-activity-engine --lib core::privacy`,
  `pnpm --filter @devbox/knowledge-features exec vitest run src/activity`.
  Windows 전용(`#[cfg(windows)]`) 테스트는 WSL에서 실행되지 않으므로 CI `Rust (Windows)` 잡이 검증한다.
- clippy·전체 build·`pnpm verify:affected`·Windows/WSL 실기 검사는 PR의 모든 과제가 끝난 뒤 한 번 모아
  실행한다. 실패하면 확인된 수정들을 먼저 마치고 실패 항목과 수정의 영향 범위만 다시 실행한다.
- 커밋은 과제 단위로 하고 push는 PR 상세 검증 후 한 번 한다. 최종 CI와 필수 수용 조건은 유지한다.
  실행 시점·범위는 [검증 운영](./docs/verification.md)을 따른다.
```

  - 267–285행 `### v0.8 통합 PR 정책 (일반 PR 단위 규칙보다 우선)` 절 전체를 아래로 교체한다.

```markdown
### 리뷰 후속 작업 PR 정책

- 원장은 리뷰 후속 ledger 이슈이고 PR 목록·순서·게이트는
  `docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md`가 정한다.
  PR 하나는 로드맵 §6의 묶음 하나(계획 파일 여러 개)에 대응한다. 계획에 없는 범위를 끼워 넣지 않는다.
- 머지는 squash만 쓴다(main은 linear history). 필수 체크 `Frontend (pnpm)`,
  `Rust (Cargo workspace)`, `Rust (Windows)`가 통과해야 한다. `Product foundation acceptance`(Windows 전체
  acceptance, 60–90분)는 머지를 막지 않고 머지 뒤 결과를 확인하며, 실패는 다음 묶음을 머지하기 전에 고친다(로드맵 §4.7).
- v0.8의 B01~B09 통합 정책(#541~#551)은 닫힌 역사 기록이다.
```

  - 386–389행(§11 `작업 기록은 PR 묶음당 workthrough/…` bullet)을 아래로 교체한다.

```markdown
- 작업 기록은 PR 본문과 ledger 이슈 댓글이다. 새 `workthrough/` 파일은 만들지 않는다.
  변경 목적·결정·영향 경로·실제 검증 결과·남은 제한을 적고, 실패·미실행·수동 실기 필요 상태를
  PASS와 구분한다.
```

- [ ] **Step 2: docs/verification.md 13–22행 교체** (표와 첫 bullet)

```markdown
| 시점 | 수행할 확인 |
|---|---|
| 과제 구현 중 | 실패하는 테스트 작성 → 해당 테스트와 직접 영향받는 테스트만 실행 → 통과 후 과제 커밋 |
| PR 전체 구현 완료 | `pnpm verify:affected` 및 미포함 회귀·Windows/WSL 실기 수행 |
| 완료 검증 실패 후 | 확인된 수정들을 모두 마친 뒤 실패·영향 범위만 모아 재실행 |
| 최종 머지·릴리스 | PR 최종 변경의 CI 통과 확인. 릴리스의 exact-main 후보 수용 조건은 release policy 적용 |

- 과제 단위 테스트는 좁게 선택한다. `--workspace`나 전체 패키지 테스트를 과제마다 돌리지 않는다.
  clippy·전체 build·affected·실기 검사는 PR 끝에 한 번 모은다.
```

- [ ] **Step 3: 모순 문장 검사**

Run:
```bash
rg -n "test·Clippy·build·affected·실기 실행은 금지|하나 개발하고 검증하는 반복을 금지|테스트와 fixture는 작성만 하고" AGENTS.md CONVENTIONS.md docs/verification.md
```
Expected: 출력 없음. 남은 문장이 있으면 같은 취지로 고친다.

- [ ] **Step 4: 커밋**

```bash
git add CONVENTIONS.md docs/verification.md
git commit -m "docs(workspace): allow task-level tests and target Windows 11 only"
```

---

### Task 4: Rust·Node 버전 고정

**Files:**
- Create: `rust-toolchain.toml`, `.nvmrc`
- Modify: `package.json`, `.github/workflows/{ci,product-foundation,release,windows-installer-acceptance,windows-package-candidate}.yml`, `.github/scripts/test-github-actions-runtime.py`

- [ ] **Step 1: 실패하는 검사 작성** — `.github/scripts/test-github-actions-runtime.py` 전체를 아래로 교체한다.

```python
#!/usr/bin/env python3
"""Keep repository workflows on audited actions and the pinned Rust/Node toolchains."""

from __future__ import annotations

import pathlib
import re


ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW_ROOT = ROOT / ".github/workflows"

toolchain = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
channel_match = re.search(r'^channel\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$', toolchain, re.MULTILINE)
assert channel_match, "rust-toolchain.toml must pin one exact Rust release"
RUST = channel_match.group(1)

NODE = (ROOT / ".nvmrc").read_text(encoding="utf-8").strip()
assert re.fullmatch(r"[0-9]+", NODE), ".nvmrc must pin one Node major version"

EXPECTED_REFERENCES = {
    "actions/checkout": "v7",
    "actions/setup-node": "v7",
    "actions/cache": "v6",
    "actions/cache/restore": "v6",
    "actions/cache/save": "v6",
    "actions/upload-artifact": "v7",
    "actions/download-artifact": "v8",
    "dtolnay/rust-toolchain": RUST,
    "Swatinem/rust-cache": "v2",
    "EmbarkStudios/cargo-deny-action": "3c6349835b2b7b196a839186cb8b78e02f7b5f25",
}

workflows = sorted(WORKFLOW_ROOT.glob("*.yml"))
assert workflows
combined = "\n".join(path.read_text(encoding="utf-8") for path in workflows)
assert "pnpm/action-setup" not in combined

seen: dict[str, int] = {name: 0 for name in EXPECTED_REFERENCES}
for workflow in workflows:
    text = workflow.read_text(encoding="utf-8")
    for action, reference in re.findall(r"^\s*-?\s*uses:\s*([^@\s]+)@([^\s#]+)", text, re.MULTILINE):
        assert action in EXPECTED_REFERENCES, f"{workflow.name}: unaudited action reference: {action}@{reference}"
        expected = EXPECTED_REFERENCES[action]
        assert reference == expected, f"{workflow.name}: {action}@{reference} must be {action}@{expected}"
        seen[action] += 1
    assert not re.search(r"^\s*node-version:", text, re.MULTILINE), \
        f"{workflow.name}: use node-version-file: .nvmrc instead of node-version"
    setup_node = len(re.findall(r"uses:\s*actions/setup-node@", text))
    node_files = len(re.findall(r"^\s*node-version-file:\s*\.nvmrc\s*$", text, re.MULTILINE))
    assert setup_node == node_files, f"{workflow.name}: every setup-node step must read .nvmrc"
    for version in re.findall(r'^\s*rust-version:\s*"?([0-9.]+)"?\s*$', text, re.MULTILINE):
        assert version == RUST, f"{workflow.name}: rust-version {version} must match rust-toolchain.toml {RUST}"
    if "pnpm install" in text:
        assert "corepack enable pnpm" in text, f"{workflow.name}: pnpm must use the repository packageManager pin"

assert all(count >= 1 for count in seen.values())
package = (ROOT / "package.json").read_text(encoding="utf-8")
assert '"packageManager": "pnpm@9.0.0"' in package
assert f'"node": ">={NODE} <{int(NODE) + 1}"' in package, "package.json engines.node must match .nvmrc"

print(f"GitHub Actions runtime policy: PASS (Rust {RUST}, Node {NODE}, pnpm 9 via Corepack)")
```

- [ ] **Step 2: 실패 확인**

Run: `python3 .github/scripts/test-github-actions-runtime.py`
Expected: `FileNotFoundError: ... rust-toolchain.toml`

- [ ] **Step 3: 고정 파일 추가**

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.98.1"
components = ["clippy", "rustfmt"]
targets = ["x86_64-unknown-linux-musl"]
```

`.nvmrc`:
```text
24
```

`package.json` — `"packageManager"` 줄 아래에 추가:
```json
  "engines": {
    "node": ">=24 <25"
  },
```

- [ ] **Step 4: 워크플로 치환**

```bash
sed -i 's|dtolnay/rust-toolchain@stable|dtolnay/rust-toolchain@1.98.1|' .github/workflows/*.yml
sed -i 's|^\(\s*\)node-version: 22$|\1node-version-file: .nvmrc|' .github/workflows/*.yml
sed -i 's|rust-version: "1.98.0"|rust-version: "1.98.1"|' .github/workflows/ci.yml
rg -n "node-version|rust-toolchain@|rust-version" .github/workflows
```
Expected: `node-version-file: .nvmrc`, `dtolnay/rust-toolchain@1.98.1`, `rust-version: "1.98.1"`만 보인다.
`dtolnay/rust-toolchain`은 버전별 ref(`@1.98.1`)를 제공한다. 만약 첫 CI에서 ref를 찾지 못하면 `@master`와 `with: toolchain: 1.98.1`로 바꾸고 테스트의 기대값도 `master`로 바꾼다(이 경우 PR 본문에 기록).

- [ ] **Step 5: 로컬 툴체인 설치와 확인**

```bash
source ~/.cargo/env
rustup toolchain install 1.98.1 --profile minimal --component clippy rustfmt \
  --target x86_64-unknown-linux-musl x86_64-pc-windows-msvc
rustc --version                       # rustc 1.98.1 이어야 한다 (저장소 안에서 실행)
python3 .github/scripts/test-github-actions-runtime.py
```
Expected: `GitHub Actions runtime policy: PASS (Rust 1.98.1, Node 24, pnpm 9 via Corepack)`

- [ ] **Step 6: 커밋**

```bash
git add rust-toolchain.toml .nvmrc package.json .github/workflows .github/scripts/test-github-actions-runtime.py
git commit -m "build(workspace): pin Rust 1.98.1 and Node 24 across local and CI"
```

---

### Task 5: ledger 이슈

- [ ] **Step 1: 이슈 생성** — 본문은 `00-roadmap.md` §6 표의 PR ID·제목을 체크박스로 옮긴다.

```bash
cat > /tmp/ledger-body.md <<'EOF'
2026-09-23 전체 리뷰 후속 작업의 진행 원장이다.
계획: docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md

각 PR 머지 후 이 이슈에 댓글로 PR·머지 커밋·CI run·Windows 실기 체크리스트 상태(사용자 확인 대기/PASS/FAIL)를 남긴다.
미실행 실기 항목은 PASS로 적지 않는다.

## 묶음 PR (00-roadmap.md §6)
(묶음마다 `- [ ] Bn <PR 제목>`, 마지막 줄은 `- [ ] R P3-01 v0.9.0 릴리스`)
## 계획 (묶음 안의 계획 파일)
(계획마다 `- [ ] P0-01 제목` … `- [ ] P2-12 제목`)
EOF
# 괄호 안 설명을 §6 표의 실제 행으로 바꾼 뒤 실행:
gh issue create --title "Review remediation ledger (2026-09-23)" --label documentation --body-file /tmp/ledger-body.md
```

- [ ] **Step 2: 이슈 번호 기록** — `00-roadmap.md` §6 첫 줄의 `Ledger: #<번호>` 자리표시를 실제 번호로 바꾸고 커밋한다.

```bash
git add docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md
git commit -m "docs(workspace): record remediation ledger issue"
```

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9 절차. 이 PR은 전체 범위 CI가 돈다.
- [ ] PR 본문 Windows 실기 체크리스트: 없음(설정·문서만 변경).
- [ ] 머지 후 main 체크아웃 갱신: `git -C /home/jihoon/projects/devbox pull --ff-only origin main` — 계획서 폴더가 tracked로 들어왔는지 확인.
