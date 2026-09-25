# P1-01 기록 보관소 이전과 ADR — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 6주간 쌓인 작업 기록(workthrough 178개, 과거 spec·plan·history 약 5만 8천 줄)을 별도 보관 저장소로 옮기고, 앞으로 판단 근거로 쓸 결정 16개를 ADR로 남긴다(A8·D26).

**Architecture:** 비공개 저장소 `jihoon22-lee/devbox-archive`를 만들고 대상 파일을 원래 경로 그대로 복사해 한 commit으로 올린다(원본 이력은 이 저장소 git history에 그대로 남는다). 본 저장소에서는 대상을 지우고, 참조 링크를 보관소 URL이나 ADR로 바꾼다. ADR은 `docs/adr/NNNN-<slug>.md`(MADR 축약형: 상태·맥락·결정·결과)로 쓴다.

**Tech Stack:** Markdown, `gh` CLI, Python(CI 범위 스크립트)

**Spec:** `review.md` §6 A8 · `00-roadmap.md` D26

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 옮기는 대상(정확히 이것만): `workthrough/`, `docs/history/`, `docs/superpowers/specs/`, `docs/superpowers/plans/`의 `2026-09-23-review-remediation/`를 **제외한** 파일 11개, `docs/roadmap.md`.
- 남기는 것: `docs/superpowers/plans/2026-09-23-review-remediation/`, `docs/architecture/`, `docs/v0.8-acceptance.md`(P1-05에서 정리), `docs/release-evidence.md`, 정책 문서 전부.
- 보관소는 비공개. 본 저장소의 링크는 보관소 URL(`https://github.com/jihoon22-lee/devbox-archive/blob/main/<원래 경로>`)로 바꾼다.
- ADR 문장은 한국어, 파일 이름은 영어 slug.

## Review Focus

1. CI 범위 판정이 `workthrough/`를 문서로 취급하던 규칙 → 디렉터리가 사라져도 `resolve-ci-scope.py` 테스트가 통과한다. (Task 2)
2. README·앱 README·CONVENTIONS의 링크가 삭제된 파일을 가리킴 → 링크 검사에서 0건. (Task 2)
3. 보관소 push 전에 본 저장소에서 먼저 지움 → 기록 유실. 반드시 보관소 commit을 확인한 뒤 지운다. (Task 1)
4. `apps/v0.8-feature-parity.json`·`apps/v0.8-data-inventory.json`가 `workthrough/` 경로 문자열을 담음 → 이 PR은 JSON을 바꾸지 않는다(검사 스크립트가 경로 존재를 보는지 확인하고, 보면 P1-05로 미룬 이유를 PR 본문에 적는다). (Task 2)
5. ADR이 현재 코드와 다름 → 각 ADR의 "근거" 절에 확인한 파일 경로를 적는다. (Task 3)

## Branch · PR

- 묶음: **B3** — 브랜치 `refactor/suite/archive-and-remove-v07`, PR 제목 `refactor(suite): archive history, remove v0.7 imports and unify dependencies`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `docs(workspace): move history to an archive and record ADRs`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 경로 | 변경 |
|---|---|
| `workthrough/`, `docs/history/`, `docs/superpowers/specs/`, 과거 plan 11개, `docs/roadmap.md` | 삭제(보관소로 이동) |
| `docs/adr/README.md` | 생성: ADR 목록과 형식 |
| `docs/adr/0001-…0016-*.md` | 생성 |
| `README.md`, `apps/devbox-*/README.md`, `docs/projects.md`, `docs/release-evidence.md`, `docs/architecture.md`, `CONVENTIONS.md` | 링크 수정 |
| `.github/scripts/resolve-ci-scope.py:230`, `.github/scripts/test-ci-scope.py:131` | `workthrough/` 규칙 정리 |

---

### Task 1: 보관소 만들기와 복사

- [ ] **Step 1: 대상 목록 고정**

```bash
cd /home/jihoon/projects/.worktrees/devbox-archive-and-remove-v07   # §4.2로 만든 묶음 B3 worktree
{ git ls-files workthrough docs/history docs/superpowers/specs docs/roadmap.md;
  git ls-files docs/superpowers/plans | grep -v '^docs/superpowers/plans/2026-09-23-review-remediation/'; } | sort > /tmp/archive-files.txt
wc -l /tmp/archive-files.txt   # 기대: 178 + 9개 폴더 파일 + 24 + 1 + 11 (docs/history 파일 수는 git ls-files로 확인)
SOURCE_SHA=$(git rev-parse HEAD)
```

- [ ] **Step 2: 보관소 생성과 복사**

```bash
gh repo view jihoon22-lee/devbox-archive >/dev/null 2>&1 || gh repo create jihoon22-lee/devbox-archive --private --description "Devbox development history archive"
rm -rf /tmp/devbox-archive && gh repo clone jihoon22-lee/devbox-archive /tmp/devbox-archive || (mkdir -p /tmp/devbox-archive && git -C /tmp/devbox-archive init -b main && git -C /tmp/devbox-archive remote add origin https://github.com/jihoon22-lee/devbox-archive.git)
rsync -a --files-from=/tmp/archive-files.txt ./ /tmp/devbox-archive/
cat > /tmp/devbox-archive/README.md <<EOF
# Devbox archive

jihoon22-lee/devbox 개발 기록 보관소. 원래 경로를 그대로 유지한다.
복사 기준 commit: ${SOURCE_SHA}
현재 결정은 본 저장소 docs/adr/를 따른다.
EOF
git -C /tmp/devbox-archive add -A
git -C /tmp/devbox-archive commit -m "docs: archive devbox history up to ${SOURCE_SHA}"
git -C /tmp/devbox-archive push -u origin main
```

- [ ] **Step 3: 복사 확인** — `git -C /tmp/devbox-archive ls-files | grep -v '^README.md$' | sort | diff - /tmp/archive-files.txt` → 차이 없음. 차이가 있으면 멈춘다.

- [ ] **Step 4: 본 저장소에서 삭제·커밋**

```bash
xargs -a /tmp/archive-files.txt git rm -q
git commit -m "docs(workspace): move development history to devbox-archive"
```

---

### Task 2: 참조·CI 규칙 정리

- [ ] **Step 1: 끊긴 링크 찾기(실패 확인)** — 아래 스크립트를 `/tmp/check-links.py`로 저장해 실행한다. 이 단계의 기대 결과는 "끊긴 링크 N건"이다.

```python
import pathlib, re, sys
root = pathlib.Path(".")
broken = []
for path in root.rglob("*.md"):
    if any(part in {"node_modules", "target", ".git"} for part in path.parts):
        continue
    for match in re.finditer(r"\]\(([^)#\s]+)(#[^)]*)?\)", path.read_text(encoding="utf-8", errors="ignore")):
        target = match.group(1)
        if "://" in target or target.startswith("mailto:"):
            continue
        if not (path.parent / target).resolve().exists():
            broken.append(f"{path}: {target}")
print("\n".join(broken)); print(f"broken={len(broken)}"); sys.exit(1 if broken else 0)
```

- [ ] **Step 2: 링크 수정 규칙**
  - `docs/history/...`, `workthrough/...`, `docs/superpowers/specs/...`, 과거 plan, `docs/roadmap.md`를 가리키는 링크 → `https://github.com/jihoon22-lee/devbox-archive/blob/main/<같은 경로>`.
  - `README.md:28`의 `· [로드맵](docs/roadmap.md)` → `· [개선 로드맵](docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md)`.
  - `README.md:30`의 v0.7 안내 줄 → 보관소 URL.
  - `docs/release-evidence.md:53`의 `./roadmap.md#release-status` → 보관소 URL(`docs/roadmap.md#release-status`).
  - 앱 README 세 곳의 "이전 개발 단계의 상세 README" 링크 → 보관소 URL.
  - `docs/superpowers/plans/2026-09-23-review-remediation/` 안의 링크는 바꾸지 않는다(검사에서 제외해도 된다).
  - 다시 실행해 `broken=0`(또는 remediation 폴더만 남음)을 확인한다.

- [ ] **Step 3: CI 범위 규칙** — `.github/scripts/resolve-ci-scope.py:230`의 `path.startswith(("docs/", "workthrough/"))`를 `path.startswith("docs/")`로 바꾸고, `.github/scripts/test-ci-scope.py:131`의 `resolve("docs/development.md", "workthrough/example.md", "README.md")`를 `resolve("docs/development.md", "docs/adr/0001-four-products.md", "README.md")`로 바꾼다. Run: `python3 .github/scripts/test-ci-scope.py` → PASS.

- [ ] **Step 4: 남은 문자열 확인** — `rg -n "workthrough" --glob '!docs/superpowers/plans/2026-09-23-review-remediation/**' --glob '!apps/v0.8-*.json'` → AGENTS·CONVENTIONS에 남은 규칙 문장이 있으면 P0-01에서 바꾼 "PR 본문·ledger" 규칙과 맞게 고친다. `apps/v0.8-*.json`의 경로 문자열은 P1-05에서 파일째 정리한다(검사 스크립트가 파일 존재를 확인하는지 `rg -n "workthrough|exists\(" .github/scripts/check-product-foundation.py`로 보고, 확인한다면 해당 검사를 이 PR에서 "보관소 이동" 예외로 완화하고 PR 본문에 적는다).

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "docs(workspace): point history links to the archive"`

---

### Task 3: ADR 16개

- [ ] **Step 1: 형식** — `docs/adr/README.md`

```markdown
# 아키텍처 결정 기록(ADR)

한 파일에 결정 하나. 번호는 바꾸지 않고, 뒤집을 때는 새 ADR을 쓰고 이전 ADR의 상태를 "대체됨(NNNN)"으로 바꾼다.

형식: 제목 / 상태(제안·채택·대체됨) / 날짜 / 맥락 / 결정 / 결과(좋은 점·치르는 비용) / 근거(파일·PR·이슈)

| 번호 | 제목 | 상태 |
|---|---|---|
(아래 16개를 번호순으로 나열)
```

- [ ] **Step 2: ADR 작성** — 각 파일은 위 형식을 따르고, 아래 "결정"·"근거" 내용을 빠짐없이 담는다. 맥락과 결과는 근거 파일을 읽고 2–5문장으로 쓴다. 날짜는 결정이 실제로 내려진 날(근거 PR·문서 날짜), 모르면 이 PR 날짜.

| 파일 | 상태 | 결정(요지) | 근거 |
|---|---|---|---|
| `0001-four-products.md` | 채택 | v0.7의 15개 앱을 Workspace·API Studio·Knowledge·Control Center 네 제품으로 합친다. 더 쪼개지 않는다(설치·연결·WebView 비용). | `docs/architecture/v0.8-foundation.md`, `apps/products.json`, review §8 |
| `0002-suite-installation.md` | 채택 | 한 설치 루트 아래 `generations/<g>/products/<p>/`, `devbox-installation.json` manifest, `devbox-activation.json` 단계(import·health·committed·recover), writer lease로 업데이트 중 새 writer를 막는다. | `crates/product-shell-tauri/src/installation.rs`, `crates/product-contract/src/activation.rs` |
| `0003-session-guard.md` | 채택 | 렌더러 요청은 handshake·route·deadline(≤30초)·requestId replay 검사를 통과해야 한다. provenance는 native가 만든다. | `crates/product-contract/src/lib.rs`, `crates/product-shell-tauri/src/lib.rs` `authorize_inner` |
| `0004-native-owned-authority.md` | 채택 | 권한은 카탈로그의 component authority와 native allowlist가 정한다. route 선택은 권한을 넓히지 않는다. | `apps/products.json` components, 각 제품 `component.rs` `allowed` |
| `0005-bounded-io.md` | 채택 | 외부 입력(HTTP, 파일, IPC 인자)은 크기·개수·시간 상한을 먼저 두고 넘으면 닫힌 실패로 끝낸다. | `crates/webhook-core/src/core/http.rs`, `MAX_ARGUMENT_BYTES` |
| `0006-filesystem-safety.md` | 채택 | symlink·junction(이름 대리 reparse)은 거부하고 OneDrive placeholder·WOF 같은 비대리 reparse는 허용한다. 열린 handle의 128비트 file ID로 대상 고정. | P0-05, `crates/filesystem/src/links.rs` |
| `0007-data-namespaces.md` | 채택 | 제품 데이터는 `%LOCALAPPDATA%\<identifier>.i<설치 접미사>`. 같은 설치의 제품은 접미사를 공유한다. | `isolate_installation`, `installation::namespace` |
| `0008-secrets.md` | 채택 | 비밀값은 DPAPI(CurrentUser)로만 저장하고 평문 내보내기를 하지 않는다. 개인용 도구 기준을 넘는 보안 장치는 두지 않는다(0016). | `crates/secrets`, S5 |
| `0009-suite-connection.md` | 채택 | 같은 설치의 제품은 named pipe bus로 기본 자동 연결, 설정에서 끌 수 있다. 연결 선호는 `suite-connection-v2.json`. | P0-04, `crates/suite-runtime/src/preference.rs` |
| `0010-release-contract.md` | 채택 | exact-main 후보를 재빌드 없이 승격, 공개 자산 7개, annotated tag. 서명은 두지 않는다(0016). | `docs/release-policy.md`, `.github/workflows/release.yml` |
| `0011-verification-policy.md` | 채택 | 과제마다 바뀐 범위의 테스트를 먼저 실패시키고 통과시킨다. `verify:affected`·clippy·Windows 검증은 PR 끝에 한 번. | P0-01(D4a), `docs/verification.md` |
| `0012-operation-log.md` | 채택 | 운영 로그는 고정 스키마(코드·시간·결과)만, 14일, 외부 전송 없음. | P0-07, `crates/product-contract/src/operation_log.rs` |
| `0013-drop-v07-migration.md` | 채택 | v0.7 데이터 가져오기를 다음 릴리스(v0.9.0)에서 제거한다(두 PC 모두 새로 구축). | D9, Q1, P1-02~P1-05 |
| `0014-typed-ipc.md` | 제안 | component별 Tauri command + 공통 admission extractor + ts-rs 타입 생성. Knowledge에서 시범 후 확대. | D18, P1-11 |
| `0015-devbox-agent.md` | 제안 | 터미널·런타임·웹훅·수집기를 사용자별 headless agent로 옮기고 UI는 클라이언트가 된다. 상세 설계는 P1-19에서 이 ADR을 갱신한다. | D24, review §8 C안 |
| `0016-personal-security-scope.md` | 채택 | 개인용 도구로서 "실제 문제가 없을 정도"의 보안만 둔다. 업데이트 서명·Authenticode·pipe DACL·CSP 추가·multipart grant·복구 저널 암호화·네이티브 확인 대화상자는 하지 않는다. 필요해지면 새 ADR로 다시 연다. | `00-roadmap.md` §2 보안 조정 |

- [ ] **Step 3: 연결** — `README.md`의 문서 목록에 `- [아키텍처 결정 기록](docs/adr/README.md)`을 추가하고, `CONVENTIONS.md`의 문서 안내 절(§11 또는 "먼저 읽을 문서" 목록)에 "결정의 이유는 `docs/adr/`를 본다" 한 줄을 추가한다.

- [ ] **Step 4: 커밋** — `git add docs/adr README.md CONVENTIONS.md && git commit -m "docs(workspace): record architecture decisions as ADRs"`

---

### Task 4: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. 문서 PR이라 `pnpm verify:affected`는 docs 범위만 돈다. `/tmp/check-links.py` 결과를 PR 본문 검증 절에 적는다.
