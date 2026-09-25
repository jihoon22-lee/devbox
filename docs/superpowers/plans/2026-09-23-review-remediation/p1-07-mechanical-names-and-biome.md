# P1-07 기계적 변경: crate 이름과 Biome — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 동작이 바뀌지 않는 대량 변경 두 가지를 한 PR에 모은다. (1) engine crate 이름을 v0.7 앱 이름(`life_log_lib` 등)에서 도메인 이름으로 바꾼다(D22). (2) 프런트엔드에 Biome을 도입해 전체를 한 번 포맷하고, React hook 의존성 규칙을 CI에서 강제한다(D19, A5). squash된 이 PR의 commit 하나를 `.git-blame-ignore-revs`에 넣어 blame 이력을 지킨다.

**Architecture:** 이 PR에는 기계적 변경만 넣는다(이름 치환 스크립트 결과, `biome format` 결과, 기존 규칙 위반에 대한 suppression 주석). 동작을 바꾸는 수정은 넣지 않는다. 그래서 squash commit 전체를 blame에서 건너뛰어도 사람의 변경이 숨지 않는다. `.git-blame-ignore-revs`는 머지 뒤 SHA가 정해지므로 다음 PR(P1-08) 첫 과제에서 추가한다.

**Tech Stack:** Python, Cargo, `@biomejs/biome` 2.x(개발 의존성, D23 승인), pnpm

**Spec:** `review.md` §6 A3·A5 · `00-roadmap.md` D19·D22·D23

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 이름 변경은 Rust 경로 식별자·Cargo 별칭 키·`[lib] name`만 바꾼다. 패키지 이름(`devbox-*-engine`)·디렉터리·문자열 리터럴·저장 데이터는 바꾸지 않는다.
- Biome은 JS/TS/TSX/MJS/CSS만 포맷한다. JSON은 포맷하지 않는다(카탈로그·fixture·lock 파일의 바이트가 digest 검사 대상이다). `src-tauri`, `node_modules`, `dist`, `target`, `fixtures`는 제외한다.
- 포맷: 들여쓰기 공백 2, 줄 폭 120, 큰따옴표, 세미콜론 항상. 린트는 recommended를 켜지 않고 아래 5개 규칙만 error로 켠다: `useExhaustiveDependencies`, `useHookAtTopLevel`, `noUnusedImports`, `noDoubleEquals`, `noFallthroughSwitchClause`.
- 기존 위반은 고치지 않고 suppression 주석으로 남긴다(사유: `existing dependency list; review in P1-15`). 새 코드는 규칙을 지켜야 한다.

## Review Focus

1. 포맷 후 테스트가 모두 통과한다(포맷은 의미를 바꾸지 않음). JSX 공백 처리 변화로 텍스트 매칭 테스트가 깨지면 포맷 결과가 아니라 테스트 문구 공백을 확인한다. (Task 2)
2. `// eslint-disable-next-line react-hooks/exhaustive-deps` 주석 17곳이 Biome suppression으로 바뀌어 규칙이 다시 켜지지 않는다. (Task 2)
3. 이름 치환 스크립트가 raw string(`r#"…"#`) 안의 옛 이름을 바꿀 수 있다 → 컴파일·테스트로 확인하고, 남은 옛 이름은 guard가 잡는다. (Task 1)
4. CI Frontend job이 scope가 없을 때 Biome을 건너뛰어도, 전체 포맷 PR 이후의 변경은 모두 검사된다(`biome ci .`는 전체 트리를 몇 초 안에 본다). (Task 3)
5. `pnpm verify:affected`가 Biome 검사를 포함해 로컬에서도 같은 실패를 낸다. (Task 3)

## Branch · PR

- 묶음: **B4** — 브랜치 `chore/workspace/mechanical-names-and-format`, PR 제목 `chore(workspace): rename engine crates and format the frontend with Biome`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `chore(workspace): rename engine crates and format the frontend with Biome`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: crate 이름(D22)

- [ ] **Step 1: guard에 옛 이름 추가(실패 확인)** — `.github/scripts/check-no-legacy.py`의 `PATTERNS`에 `r"\b(life_log|everything_plus|code_pad|api_playground|knowledge_base|log_lens|port_manager|workbench|repo_manager|run_manager|wsl_desktop|developer_toolbox|webhook_lab|devbox_manager)_lib\b"`를 추가한다. Run: `python3 .github/scripts/check-no-legacy.py --scope all` → FAIL.

- [ ] **Step 2: 치환 스크립트** — `/tmp/rename-crates.py`로 저장해 저장소 루트에서 실행한다.

```python
import pathlib, re
names = {
    "life_log_lib": "activity_engine",
    "everything_plus_lib": "content_index_engine",
    "code_pad_lib": "editor_engine",
    "api_playground_lib": "http_client_engine",
    "knowledge_base_lib": "knowledge_vault_engine",
    "log_lens_lib": "logs_engine",
    "port_manager_lib": "ports_engine",
    "workbench_lib": "projects_engine",
    "repo_manager_lib": "repositories_engine",
    "run_manager_lib": "runtime_engine",
    "wsl_desktop_lib": "terminal_engine",
    "developer_toolbox_lib": "toolbox_engine",
    "webhook_lab_lib": "webhook_host",
    "devbox_manager_lib": "installation_tools",
}
pattern = re.compile(r"\b(" + "|".join(names) + r")\b")
string = re.compile(r'"(?:\\.|[^"\\])*"')
changed = []
paths = [p for base in ("apps", "crates") for glob in ("*.rs", "Cargo.toml") for p in pathlib.Path(base).rglob(glob)]
for path in paths:
    if "target" in path.parts or "node_modules" in path.parts:
        continue
    text = path.read_text()
    out, last = [], 0
    for literal in string.finditer(text):
        out.append(pattern.sub(lambda m: names[m.group(1)], text[last:literal.start()]))
        out.append(literal.group(0))
        last = literal.end()
    out.append(pattern.sub(lambda m: names[m.group(1)], text[last:]))
    new = "".join(out)
    if path.name == "Cargo.toml":
        # `[lib] name = "life_log_lib"` is a string value; rename it explicitly.
        new = re.sub(r'(\[lib\][^\[]*?name\s*=\s*")(' + "|".join(names) + r')(")',
                     lambda m: m.group(1) + names[m.group(2)] + m.group(3), new, flags=re.S)
    if new != text:
        path.write_text(new)
        changed.append(str(path))
print(len(changed), "files changed")
```

- [ ] **Step 3: 확인** — Run: `source ~/.cargo/env && cargo check --workspace --all-targets && cargo test --workspace --lib --no-run && python3 .github/scripts/check-no-legacy.py --scope all` → PASS. guard가 `.github/scripts`의 Python·mjs에서 옛 이름을 잡으면 그 문자열을 새 이름으로 바꾼다(스크립트가 crate 이름으로 경로·범위를 판정하는 경우). 문자열 리터럴에 남은 옛 이름은 `rg -n "_lib\b" crates apps --glob '*.rs'`로 보고, 테스트 이름·로그 문구면 새 이름으로 바꾼다.

- [ ] **Step 4: 커밋** — `git add -A && git commit -m "refactor(crates): name engine crates after their domains"`

---

### Task 2: Biome 도입과 전체 포맷

**Files:** Create `biome.json`; Modify `package.json`, `pnpm-lock.yaml`, 모든 JS/TS/TSX/MJS/CSS(포맷), `eslint-disable` 주석 17곳

- [ ] **Step 1: 설치** — Run: `pnpm add -D -w --save-exact @biomejs/biome@2`. `package.json`의 `devDependencies`에 정확한 버전이 들어갔는지 본다. Run: `python3 .github/scripts/check-dependencies.py check` → 라이선스(MIT OR Apache-2.0)와 플랫폼 바이너리 패키지(`@biomejs/cli-*`)가 정책을 통과해야 한다. 막히면 `.github/dependency-policy.json`에 D23 승인 근거로 항목을 추가한다.

- [ ] **Step 2: 설정** — 루트 `biome.json`(스키마 경로는 설치된 버전의 파일을 가리킨다):

```json
{
  "$schema": "./node_modules/@biomejs/biome/configuration_schema.json",
  "vcs": { "enabled": true, "clientKind": "git", "useIgnoreFile": true },
  "files": {
    "includes": [
      "apps/**",
      "packages/**",
      ".github/scripts/**/*.mjs",
      "!**/node_modules",
      "!**/dist",
      "!**/target",
      "!**/src-tauri",
      "!**/fixtures",
      "!**/*.json"
    ]
  },
  "formatter": { "enabled": true, "indentStyle": "space", "indentWidth": 2, "lineWidth": 120 },
  "javascript": { "formatter": { "quoteStyle": "double", "semicolons": "always" } },
  "css": { "formatter": { "enabled": true } },
  "assist": { "enabled": false },
  "linter": {
    "enabled": true,
    "rules": {
      "recommended": false,
      "correctness": {
        "useExhaustiveDependencies": "error",
        "useHookAtTopLevel": "error",
        "noUnusedImports": "error"
      },
      "suspicious": {
        "noDoubleEquals": "error",
        "noFallthroughSwitchClause": "error"
      }
    }
  }
}
```

  설치된 Biome 버전에서 `files.includes`의 제외 문법이나 규칙 그룹 이름이 다르면 `pnpm exec biome migrate --write`로 설정을 그 버전에 맞춘다.

- [ ] **Step 3: 스크립트** — 루트 `package.json`의 `scripts`에 추가한다.

```json
    "lint": "biome lint .",
    "format": "biome format --write .",
    "check:frontend": "biome ci ."
```

- [ ] **Step 4: eslint 주석 바꾸기** — `rg -n "eslint-disable" apps packages`의 17곳을 바꾼다. `// eslint-disable-next-line react-hooks/exhaustive-deps`는 `// biome-ignore lint/correctness/useExhaustiveDependencies: <바로 위 주석의 이유, 없으면 "intentional dependency list">`로 바꾼다. 그 밖의 eslint 규칙 이름이면 대응하는 Biome 규칙으로, 대응 규칙이 켜져 있지 않으면 주석을 지운다.

- [ ] **Step 5: 포맷과 suppression** — Run:

```bash
pnpm exec biome format --write .
pnpm exec biome lint --write --suppress="existing dependency list; review in P1-15" .
pnpm exec biome ci .
```

  마지막 명령이 성공해야 한다. `--suppress`가 지원되지 않는 버전이면 `pnpm exec biome lint . 2>&1 | tee /tmp/biome-lint.txt`로 위반 위치를 뽑아 각 줄 위에 같은 사유의 `biome-ignore` 주석을 넣는다.

- [ ] **Step 6: 확인** — Run: `pnpm -r --workspace-concurrency 2 test -- --run && pnpm -r --workspace-concurrency 2 exec tsc --noEmit` → PASS(테스트 스크립트가 이미 `vitest run`이면 `-- --run`은 빼도 된다). 실패하면 포맷이 테스트의 문자열 비교를 바꿨는지 확인한다.

- [ ] **Step 7: 커밋** — `git add -A && git commit -m "style(workspace): format the frontend with Biome"`

---

### Task 3: CI·로컬 검증 연결

- [ ] `.github/workflows/ci.yml` Frontend job의 "Install dependencies" 다음에 step을 추가한다.

```yaml
      - name: Check format and hook rules
        if: ${{ needs.scope.outputs.frontend_scope != 'none' }}
        run: pnpm exec biome ci .
```

- [ ] `.github/scripts/verify-affected.sh`에서 프런트엔드 검사를 실행하는 부분(프런트 scope가 none이 아닐 때 `run-frontend-scope.sh`를 부르는 곳) 앞에 `pnpm exec biome ci .`를 추가한다.
- [ ] `CONVENTIONS.md`의 프런트엔드 절에 두 줄을 추가한다: "포맷·hook 규칙은 Biome(`pnpm format`, `pnpm lint`)이 정한다. 새 코드는 suppression 없이 `useExhaustiveDependencies`를 지킨다." 그리고 AGENTS.md의 검증 절에 "커밋 전 `pnpm exec biome ci .`"를 한 줄 추가한다.
- [ ] Run: `bash .github/scripts/verify-affected.sh` → PASS. 커밋: `git add -A && git commit -m "ci(workspace): enforce Biome in CI and local verification"`

---

### Task 4: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. 전체 범위 변경이므로 `pnpm verify:all`.
- [ ] PR 본문에 "기계적 변경만 포함(이름 치환·포맷·suppression). squash commit을 `.git-blame-ignore-revs`에 등록할 예정(P1-08 첫 과제)"을 적는다.
- [ ] 머지 후 squash commit SHA를 ledger에 적는다: `gh pr view <번호> --json mergeCommit -q .mergeCommit.oid`.
