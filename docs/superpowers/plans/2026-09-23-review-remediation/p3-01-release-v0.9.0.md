# P3-01 v0.9.0 릴리스(버전은 한 번만 올린다) — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. 릴리스 단계에서는 저장소 스킬 `.agents/skills/devbox-release/SKILL.md`와 `docs/release-policy.md`도 함께 읽는다. Codex는 같은 순서를 직접 따른다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Phase 0–2의 모든 PR이 머지된 뒤 버전을 `0.8.1` → `0.9.0`으로 **한 번만** 올려 공개한다(D29). 중간 릴리스는 없다. 태그 전에 사용자가 후보 설치본으로 필수 항목을 확인하고, 공개 뒤 다른 PC에서 v0.8.1 업데이터로 올라가는 경로를 확인한다.

**Architecture:**
- 버전 PR(버전·문서·CHANGELOG·버전 검사 일반화) → main CI → exact-main 후보(`windows-package-candidate.yml`) → **사용자 필수 점검(후보 setup)** → annotated tag → Release workflow(후보를 재빌드 없이 승격·검증·공개·공개본 실행 확인) → 근거 기록 PR → ledger 정리.
- 후보 workflow의 설치 acceptance는 같은 후보 안에서 generation 업데이트·되돌리기·복구·제거를 검사한다. 공개된 v0.8.1의 Control Center 업데이터가 v0.9.0을 받아 적용하는 경로는 공개 뒤에만 확인할 수 있다. 그래서 집 PC는 태그 전에 후보 setup을 기존 v0.8.1 설치 위에 실행해 확인하고, 회사 PC는 공개 뒤 Control Center 업데이트로 확인한다.
- 중간 릴리스가 없었으므로 PR마다 "사용자 확인 대기"로 남은 실기 항목을 ledger에서 모아 점검표 하나로 만든다(필수 8개는 태그 전, 나머지는 공개 뒤).

**Tech Stack:** GitHub Actions(`windows-package-candidate.yml`, `release.yml`), `gh` CLI, Python 검증 스크립트

**Spec:** `00-roadmap.md` §5 최종 게이트, D6·D29 · `docs/release-policy.md`

## Global Constraints

- `00-roadmap.md` §3 전부 적용. D6: 이 세션이 머지·후보 실행·태그·공개까지 한다. 단, Task 3의 사용자 필수 점검 응답을 받기 전에는 태그하지 않는다(§4.10 6번).
- 선행: §6의 묶음 B1–B12 머지, main CI 초록, 모든 묶음 PR의 `Product foundation acceptance`(PF) 마지막 실행이 성공으로 ledger에 기록됨(실패였다면 그 수정이 머지되고 다음 PF가 성공). 이 PR은 PF까지 기다린 뒤 머지한다.
- 버전은 이 PR에서만 바꾼다: 네 제품의 `Cargo.toml`·`tauri.conf.json`·`package.json`, `apps/devbox-agent`의 `Cargo.toml`·`tauri.conf.json`, acceptance config 2개, 문서의 "현재 버전" 표기.
- 공개 계약(자산 7개, release manifest schema 2)을 바꾸지 않는다. v0.8.1 업데이터가 그대로 받아들여야 한다. agent는 Control Center ZIP 안(`resources/suite/devbox-agent.exe`)에 들어 있어 자산 수가 늘지 않는다.
- 태그는 main의 해당 commit CI가 끝난 뒤 민다. 후보 실행부터 태그까지 main에 다른 머지를 넣지 않는다. 후보 artifact 보존은 7일(shard)·14일(조립본)이고 후보 성공 후 7일 안에 태그한다. 7일이 지나면 같은 commit으로 후보를 다시 만든다(실패가 아니라 만료다).
- 실패한 후보·release run을 새 빌드로 덮지 않는다. 원인을 고친 PR을 머지한 뒤 새 main commit으로 후보부터 다시 한다. 공개 RC/prerelease를 만들지 않는다.

## Review Focus

1. v0.8.1 설치본 위에 후보 setup 실행 → 새 generation으로 올라가고, 네 제품과 agent가 뜨며, 기존 데이터(Workspace 프로젝트·LSP·터미널 설정, Knowledge 노트·활동, API Studio 컬렉션·환경·기록)가 그대로다. API Studio 데이터는 첫 실행 때 native 저장소로 옮겨진다. (Task 3 필수 점검)
2. 공개 뒤 v0.8.1 Control Center 업데이터가 v0.9.0의 manifest·자산을 받아들여 적용한다(회사 PC). (Task 5)
3. `check-product-foundation.py`의 버전 정규식이 `0.8.x`에 묶여 있으면 이 PR의 CI가 깨진다 → 먼저 일반화하고, 네 제품과 agent의 버전이 같은지 검사한다(agent 검사는 P2-01에서 추가됨). (Task 1)
4. CHANGELOG 절 제목이 `extract-release-notes.py`의 형식(`## [v0.9.0] - YYYY-MM-DD`)이어야 Release draft가 만들어진다. (Task 1)
5. 후보와 태그 사이 main이 움직이지 않고, 태그는 main CI가 끝난 뒤에만 민다. (Task 2·4)

## Branch · PR

- 브랜치: `chore/release/v0.9.0`
- PR 제목: `chore(release): prepare v0.9.0`

## File Structure

| 파일 | 변경 |
|---|---|
| `apps/devbox-{workspace,api-studio,knowledge,control-center}/src-tauri/Cargo.toml` | `version = "0.9.0"` |
| `apps/devbox-*/src-tauri/tauri.conf.json`, `apps/devbox-*/package.json` | `"version": "0.9.0"` |
| `apps/devbox-agent/Cargo.toml`, `apps/devbox-agent/tauri.conf.json` | `0.9.0` |
| `Cargo.lock` | 자동 갱신 |
| `.github/scripts/check-product-foundation.py` | 버전 정규식 일반화, 제품 버전 동일 검사 |
| `.github/scripts/windows-packaged-smoke-config.json`, `.github/scripts/windows-installer-acceptance-config.json` | `suiteVersion`·제품 `version` |
| `CHANGELOG.md` | `## [v0.9.0] - <날짜>` 절 |
| `README.md`, `docs/windows-guide.md`, `docs/projects.md`, `CONVENTIONS.md`, `AGENTS.md` | 현재 버전·setup 파일 이름 |
| `THIRD_PARTY_NOTICES.md` | 재생성(바뀌면) |

---

### Task 1: 버전과 문서

- [ ] **Step 1: 선행 조건 확인**

```bash
cd /home/jihoon/projects/devbox && git fetch origin main && git pull --ff-only origin main
python3 - <<'EOF'
import json, re, subprocess
roadmap = open("docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md", encoding="utf-8").read()
branches = set(re.findall(r"\| `([a-z]+/[a-z0-9./-]+)` \|", roadmap)) - {"chore/release/v0.9.0"}
merged = {pr["headRefName"] for pr in json.loads(subprocess.check_output(
    ["gh", "pr", "list", "--state", "merged", "--limit", "300", "--json", "headRefName"]))}
missing = sorted(branches - merged)
print(f"plan branches: {len(branches)}, missing: {missing}")
raise SystemExit(1 if missing else 0)
EOF
gh run list --branch main --workflow CI --limit 1 --json conclusion,headSha,status
```

  Expected: `plan branches: 12, missing: []`(묶음 PR B1–B12 모두 머지), main 최신 CI `completed/success`, ledger에 묶음마다 PF 성공 기록. 하나라도 빠지면 멈추고 빠진 것을 먼저 끝낸다.
  이어서 main 최신 commit에서 Windows 전체 acceptance를 한 번 돌려 성공을 확인한다(묶음 PR의 PF는 머지 전 PR 상태에서 돈 것이므로): `gh workflow run product-foundation.yml --ref main` → `gh run watch <run id> --exit-status`.

- [ ] **Step 2: worktree와 브랜치**

```bash
git worktree add /home/jihoon/projects/.worktrees/devbox-release-v0.9.0 -b chore/release/v0.9.0 origin/main
cd /home/jihoon/projects/.worktrees/devbox-release-v0.9.0
pnpm install --frozen-lockfile && source ~/.cargo/env
```

- [ ] **Step 3: 버전 검사 일반화(먼저)** — `.github/scripts/check-product-foundation.py`에서 제품 `Cargo.toml` 버전을 `0.8.x`로 고정한 정규식을 일반 semver로 바꾼다.

```python
        assert re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", cargo["package"]["version"])
```

  같은 파일에 네 제품 버전이 서로 같은지 보는 검사가 없으면 제품 루프 뒤에 추가한다(P2-01의 agent 버전 검사는 그대로 둔다).

```python
    versions = {json.loads((root / f"apps/devbox-{product}/package.json").read_text())["version"] for product in ("workspace", "api-studio", "knowledge", "control-center")}
    assert len(versions) == 1, f"product versions differ: {sorted(versions)}"
```

  Run: `python3 .github/scripts/check-product-foundation.py` → PASS(아직 0.8.1).

- [ ] **Step 4: 버전 올림**

```bash
for app in workspace api-studio knowledge control-center; do
  sed -i '0,/^version = "0\.8\.[0-9]*"/s//version = "0.9.0"/' apps/devbox-$app/src-tauri/Cargo.toml
  sed -i 's/"version": "0\.8\.[0-9]*"/"version": "0.9.0"/' apps/devbox-$app/src-tauri/tauri.conf.json apps/devbox-$app/package.json
done
sed -i '0,/^version = "0\.8\.[0-9]*"/s//version = "0.9.0"/' apps/devbox-agent/Cargo.toml
sed -i 's/"version": "0\.8\.[0-9]*"/"version": "0.9.0"/' apps/devbox-agent/tauri.conf.json
sed -i 's/"0\.8\.1"/"0.9.0"/g' .github/scripts/windows-packaged-smoke-config.json .github/scripts/windows-installer-acceptance-config.json
cargo metadata --format-version 1 >/dev/null   # Cargo.lock 갱신
git diff --stat
python3 .github/scripts/check-product-foundation.py
```

  `git diff`에서 각 `Cargo.toml`의 `[package]` version 한 줄씩만 바뀌었는지 확인한다(의존성 version 줄이 바뀌었으면 되돌린다).

- [ ] **Step 5: 문서의 현재 버전**
  - `README.md`의 `Devbox_0.8.1_x64-setup.exe`, `docs/windows-guide.md`의 setup 파일 이름 → `Devbox_0.9.0_x64-setup.exe`.
  - `docs/projects.md`, `CONVENTIONS.md`, `AGENTS.md`의 "현재 소스"·현재 버전 표기 → 0.9.0.
  - `rg -n "0\.8\.1|0\.8\.0" README.md docs/windows-guide.md docs/projects.md CONVENTIONS.md AGENTS.md`로 남은 현재 버전 표기가 없는지 본다(역사 기록 문장은 그대로 둔다).

- [ ] **Step 6: CHANGELOG** — `CHANGELOG.md` 맨 위 `# Changelog` 다음에 추가한다(날짜는 PR 작성일 `date +%F`). 기존 절처럼 짧은 bullet로 쓴다.

```markdown
## [v0.9.0] - YYYY-MM-DD

v0.8.1 이후 전체 리뷰 후속 작업을 한 번에 담은 릴리스다. v0.8.x 설치본은 Control Center 업데이트로 바로 올라간다.

- **v0.7 데이터 가져오기 제거.** v0.7 앱 데이터는 v0.8.1에서 먼저 옮긴 뒤 업데이트해야 한다. 새 설치는 Control Center "데이터 및 복구"에서 활성화를 확정한다.
- **API Studio 저장소 이전.** 컬렉션·기록·환경·gRPC 기록·변환 워크플로를 WebView 저장소에서 제품 데이터 폴더로 첫 실행 때 자동 이전.
- **백그라운드 서비스(devbox-agent).** 창을 닫아도 작업·서비스·예약 실행, 웹훅 리스너, 활동 기록과 검색 색인이 계속 동작. 알림 영역 아이콘 하나, "로그인할 때 백그라운드 서비스 시작" 설정(기본 꺼짐), 업데이트 전 자동 종료.
- **Workspace 에이전트 화면.** 작업마다 Git worktree를 만들고 Claude Code·Codex 터미널을 바로 시작, 변경 검토 뒤 병합·버리기, 작업별 CPU·메모리·토큰 사용량.
- **Devbox MCP 서버.** Control Center에서 켜면 WSL의 Claude Code·Codex가 프로젝트·작업 실행 기록·로그·검색·노트를 도구로 사용(기본 꺼짐, 노트 기록·작업 실행은 별도 허용).
- **Workspace Source.** branch 만들기·전환·이름 바꾸기·삭제(8초 되돌리기), stash, hunk 단위 stage·unstage·버리기, amend, blame, 병합·rebase 충돌 해결, `gh`로 PR 상태 보기·만들기.
- **API Studio.** curl·Postman·Insomnia·Bruno·HAR 가져오기, 요청마다 파일 하나인 파일 컬렉션, 응답 검증·값 캡처·컬렉션 실행기, OAuth 2.0(Authorization Code + PKCE, Client Credentials, 토큰은 DPAPI 봉인), 요청별 TLS 설정(사용자 CA·클라이언트 인증서·검증 끄기 상시 표시), 코드 생성(curl·fetch·Python·Go·C#).
- Knowledge 노트 자동 저장과 비정상 종료 복구, 오류를 문장 대신 코드로 분류.
- 빠른 기록·템플릿 노트·일일 기록·새 프로젝트 등록은 바로 실행하고 "되돌리기" 제공, "작업 상태 › 최근 오류"에서 진단 복사.
- Activity 개인정보 규칙을 줄 단위로 입력·저장, 잘못된 정규식 저장 거부, 규칙을 읽지 못하면 창 제목 수집 중단.
- 업데이트 다운로드 캐시 자동 정리, 같은 설치의 제품은 업데이트 뒤에도 자동 연결, Control Center 단축키 화면 연결, 조기 거부 오류 문구 구분.
- OneDrive 온라인 전용·압축(WOF) 파일 허용(junction·symlink 차단 유지), Dev Drive(ReFS) 128비트 파일 ID.
- Webhook Lab이 chunked 본문, `Expect: 100-continue`, 바이너리 본문을 받아 기록·fixture 저장·재전송.
- 제품별 운영 로그(코드·소요 시간만, 14일 보관)와 panic 위치 기록, 지원 번들에 최근 기록 포함.
- 명령마다 하던 제품 정보 재조회·설치 파일 재검증 제거, 터미널 출력 push 방식으로 유휴 CPU·IPC 제거.
- 내부 정리: component별 타입 명령과 코드별 오류 문구, 옛 앱 실행기·마이그레이션 코드 제거, 의존성 버전 통일, 자식 프로세스·DPAPI 처리 공용화, 개발 기록 보관소 이동, 지원 OS Windows 11 명시, 개발 도구 Rust 1.98.1·Node 24 고정.

게시·Windows 실기·후보 검증 결과는 해당 Release와 릴리스 원장을 따른다.
```

  목록의 각 항목이 실제로 머지된 PR과 맞는지 ledger로 확인하고, 계획과 달라진 점(PR 본문 "계획과 다르게 한 점")이 사용자에게 보이는 변화라면 문장을 고친다.

- [ ] **Step 7: notices와 전체 검증** — Run: `python3 .github/scripts/check-dependencies.py generate && python3 .github/scripts/check-dependencies.py check && pnpm verify:all`. 실패하면 원인을 고치고 실패 범위만 다시 실행한다.

- [ ] **Step 8: 커밋·PR·머지**

```bash
git add -A && git commit -m "chore(release): prepare v0.9.0"
git push -u origin chore/release/v0.9.0
gh pr create --title "chore(release): prepare v0.9.0" --body-file /tmp/pr-body.md   # 00-roadmap.md §4.6 형식
gh pr checks --watch && gh pr merge --squash --delete-branch   # 릴리스 PR은 PF를 포함한 모든 체크를 기다린다
SHA=$(git ls-remote origin refs/heads/main | cut -f1)
```

  이 시점부터 Task 4의 태그까지 main에 다른 머지를 넣지 않는다.

---

### Task 2: 후보

- [ ] **Step 1: main CI 완료 대기**

```bash
gh run list --branch main --workflow CI --limit 1 --json databaseId,headSha,status,conclusion
gh run watch <databaseId> --exit-status
```

  `headSha`가 `$SHA`이고 `conclusion`이 `success`여야 한다.

- [ ] **Step 2: 후보 실행**

```bash
gh workflow run windows-package-candidate.yml --ref main -f candidate_tag=v0.9.0 -f candidate_commit=$SHA
sleep 5; RUN=$(gh run list --workflow windows-package-candidate.yml --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch $RUN --exit-status
```

  실패하면: `gh run view $RUN --log-failed`의 요약을 ledger에 남기고 원인 수정 PR을 §4 절차로 머지한 뒤 새 `$SHA`로 Step 1부터 다시 한다.

- [ ] **Step 3: 후보 근거** — `gh run view $RUN --json jobs -q '.jobs[] | [.name, .conclusion] | @tsv'`로 모든 job 성공을 확인하고, 조립 artifact(이름은 `gh run view $RUN --json artifacts`)의 evidence에서 자산 7개 이름·크기·SHA-256을 ledger 댓글에 옮긴다. `gh run view $RUN --log | grep -c wsl_source_method_unavailable` → 0(새 Source 메서드가 WSL helper 허용 목록에 모두 있음).

---

### Task 3: 태그 전 사용자 필수 점검(멈추고 기다림)

- [ ] **Step 1: 후보 setup을 Windows로 내려받기**

```bash
WINHOME=$(wslpath -u "$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')")
DEST="$WINHOME/Downloads/devbox-v0.9.0-candidate"
gh run download $RUN -n <조립 artifact 이름> -D "$DEST"
ls "$DEST"    # Devbox_0.9.0_x64-setup.exe 와 네 제품 ZIP
```

- [ ] **Step 2: 대기 항목 모음** — ledger 댓글에서 "사용자 확인 대기"로 남은 실기 항목을 PR별로 모아(`gh issue view <ledger 번호> --comments`) 한 댓글로 정리한다. 아래 필수 8개는 태그 전, 나머지는 Task 5(공개 뒤)에서 확인한다.

- [ ] **Step 3: 사용자에게 필수 점검 요청** — 아래 표를 ledger 댓글과 사용자 보고에 그대로 남기고, **응답을 받을 때까지 멈춘다**(§4.10 6번). 사용자가 "점검 생략"이라고 답하면 기록하고 Task 4로 간다.

  집 PC(기존 v0.8.1 설치 상태)에서 `Downloads\devbox-v0.9.0-candidate\Devbox_0.9.0_x64-setup.exe` 실행:
  1. 설치가 끝나고 Control Center에 v0.9.0이 보이며 네 제품이 열린다.
  2. 알림 영역에 Devbox 아이콘이 하나만 있고, 제품을 모두 닫아도 `devbox-agent.exe`가 남아 있다(작업 관리자).
  3. Workspace: 기존 프로젝트·LSP 설정·터미널 설정이 그대로. 터미널을 열고, 작업 하나를 실행한 채 Workspace를 닫았다 다시 열면 작업이 계속 돌고 있다.
  4. API Studio: 기존 컬렉션·환경·기록이 그대로 보이고(자동 이전), 저장된 요청 하나를 보낼 수 있다.
  5. Knowledge: 기존 노트가 그대로. 노트를 고치고 2초 기다린 뒤 작업 관리자로 강제 종료 → 다시 열면 저장돼 있거나 복구 배너가 뜬다.
  6. API Studio 웹훅 리스너를 켠 채 API Studio를 닫고 WSL에서 `curl -X POST http://127.0.0.1:<포트>/hook -d x` → 다시 연 API Studio 기록에 보인다.
  7. Workspace "에이전트" 화면에서 Claude Code 작업을 하나 만들면 새 폴더·branch가 생기고 터미널에서 `claude`가 시작된다(버리기로 정리).
  8. `%LOCALAPPDATA%\com.devbox.v08.*.i*\logs\`가 제품마다 생긴다.

- [ ] **Step 4: 응답 처리**
  - 모두 PASS(또는 "점검 생략") → ledger에 기록하고 Task 4.
  - FAIL이 있으면: 원인을 조사해 수정 PR을 §4 절차로 머지한다(버전은 0.9.0 그대로). 새 `$SHA`로 Task 2 → Task 3을 다시 한다(태그가 아직 없으므로 같은 `candidate_tag=v0.9.0`으로 다시 만든다).
  - 후보 성공 뒤 7일이 지나면 같은 `$SHA`로 Task 2 Step 2부터 후보를 다시 만든다.

---

### Task 4: 태그와 공개

- [ ] **Step 1: 태그**

```bash
[ "$(git ls-remote origin refs/heads/main | cut -f1)" = "$SHA" ] || { echo "main moved; stop"; exit 1; }
git fetch origin && git tag -a v0.9.0 $SHA -m "Devbox v0.9.0"
git push origin v0.9.0
```

- [ ] **Step 2: Release 확인**

```bash
sleep 10; REL=$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch $REL --exit-status
gh release view v0.9.0 --json tagName,isDraft,isPrerelease,assets -q '{tag:.tagName,draft:.isDraft,pre:.isPrerelease,assets:[.assets[].name]}'
```

  Expected: draft false, prerelease false, 자산 7개(네 제품 ZIP, `Devbox_0.9.0_x64-setup.exe`, `release-manifest.json`, `THIRD_PARTY_NOTICES.md`), Latest가 v0.9.0. 실패하면 새 빌드로 덮지 않고 원인을 ledger에 남긴 뒤 release policy의 실패 절차를 따른다.

- [ ] **Step 3: 정리** — ledger 댓글(PR, `$SHA`, 후보 run, release run, 자산 digest, published-runtime 결과). worktree 정리: `git worktree remove /home/jihoon/projects/.worktrees/devbox-release-v0.9.0 && git worktree prune && git branch -D chore/release/v0.9.0`.

---

### Task 5: 공개 뒤 확인과 계획 마무리

- [ ] **Step 1: 업데이터 경로 점검 요청(사용자 확인 대기)** — 회사 PC(v0.8.1 설치 상태)에서 Control Center › 업데이트 → v0.9.0 확인 → 다운로드 → 적용 → 네 제품이 다시 열리고 "이 설치의 제품이 연결되어 있습니다."가 보인다. 이어서 Task 3 Step 2에서 모은 나머지 대기 항목을 두 PC에서 확인하도록 ledger에 목록으로 남긴다. FAIL이면 v0.9.1 수정 릴리스가 필요한지 사용자와 정한다(§4.10 3번에 준해 묻는다).
- [ ] **Step 2: 근거 기록 PR** — 브랜치 `docs/release/v0.9.0-evidence`, PR 제목 `docs(release): record v0.9.0 evidence`로 §4 절차를 따른다:
  - `docs/release-evidence.md` 맨 위 현재 stable 절을 v0.9.0으로 바꾸고 v0.8.1 절은 "이전 stable"로 내린다. 값은 Task 4 Step 3의 ledger 댓글(머지 PR, source SHA, 후보 run, release run, 자산 7개 SHA-256, published-runtime 결과)과 Task 3의 사용자 점검 결과.
  - `docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md` §2.4 백로그 표에 각 항목의 현재 상태를 적는다.
- [ ] **Step 3: ledger 마무리** — 최종 댓글: 머지한 PR 목록(번호·제목), 릴리스 링크, 아직 대기인 실기 항목. 대기 항목이 없으면 `gh issue close <ledger 번호> --reason completed`, 있으면 열어 둔 채 제목 끝에 "(실기 확인 대기)"를 붙인다.
