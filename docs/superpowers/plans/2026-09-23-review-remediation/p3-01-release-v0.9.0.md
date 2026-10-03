# P3-01 v0.9.0 릴리스(버전은 한 번만 올린다) — Implementation Plan

> **v0.9.0 철회 전 실행 이력.** 아래 상태·명령은 당시 기록이다. 현재 작업은 [2026-10-03 통합 계획](../2026-10-03-product-readiness/00-roadmap.md)을 따르며 #580은 닫혔다.
**공개 완료(2026-10-03):** [v0.9.0 stable](https://github.com/jihoon22-lee/devbox/releases/tag/v0.9.0), source `e499ac7127269bf67863bf0fdc42eaf53236b9f3`. CI·후보 11개 job·공개본 검증이 통과했다. [근거](../../../release-evidence.md)와 #580이 현재 상태의 정본이며, 아래 계획 체크박스는 변경하지 않는다. 사용자 실사용 항목은 출시 후 제보로 추적한다.

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. 릴리스 단계에서는 `docs/release-policy.md`를 직접 읽는다. Codex는 같은 순서를 직접 따른다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Phase 0–2의 모든 PR이 머지된 뒤 버전을 `0.8.1` → `0.9.0`으로 **한 번만** 올려 공개한다(D29). 중간 릴리스는 없다. 2026-10-03 지시에 따라 자체 검토·수정과 자동 후보 검증 후 태그·공개한다. 사용자 실사용 항목은 출시 후 제보로 추적하며 대기하지 않는다.

**Architecture:**
- 버전 PR(버전·문서·CHANGELOG·버전 검사 일반화) → main CI → exact-main 후보(`windows-package-candidate.yml`) → 자동 수용 근거 확인 → annotated tag → Release workflow(후보를 재빌드 없이 승격·검증·공개·공개본 실행 확인) → 근거 기록 PR → ledger 정리.
- 후보 workflow의 설치 acceptance는 같은 후보 안에서 generation 업데이트·되돌리기·복구·제거를 검사한다. 공개 v0.8.1 업데이터와의 호환성을 증명하는 검사는 아니다. 2026-10-03 이전 reader 재현에서 새 Knowledge helper·agent 파일 거부를 확인했다. 중간 호환 릴리스 없이 Release setup 직접 실행을 안내한다. 실제 기존 데이터 보존과 사용자 신규 설치는 출시 후 추적하며 미실행을 PASS로 바꾸지 않는다.
- 중간 릴리스가 없었으므로 PR마다 "사용자 확인 대기"로 남은 실기 항목을 ledger에서 모아 점검표 하나로 만든다. 기존 필수 8개를 포함해 사용자 점검은 공개 뒤 제보/추적으로 전환한다.

**Tech Stack:** GitHub Actions(`windows-package-candidate.yml`, `release.yml`), `gh` CLI, Python 검증 스크립트

**Spec:** `00-roadmap.md` §5 최종 게이트, D6·D29 · `docs/release-policy.md`

## Global Constraints

- `00-roadmap.md` §3 전부 적용. D6: 이 세션이 머지·후보 실행·태그·공개까지 한다. 2026-10-03 사용자가 자체 검토부터 태그·공개까지 진행하도록 다시 지시했으며, Task 3은 응답 대기 없이 미실행 항목을 기록한다.
- 선행: §6의 묶음 B1–B12 머지, main CI 초록, 모든 묶음 PR의 `Product foundation acceptance`(PF) 마지막 실행이 성공으로 ledger에 기록됨(실패였다면 그 수정이 머지되고 다음 PF가 성공). 이 PR은 PF까지 기다린 뒤 머지한다.
- 버전은 이 PR에서만 바꾼다: 네 제품의 `Cargo.toml`·`tauri.conf.json`·`package.json`, `apps/devbox-agent`의 `Cargo.toml`·`tauri.conf.json`, acceptance config 2개, 문서의 "현재 버전" 표기.
- 공개 계약(자산 7개, release manifest schema 2)을 바꾸지 않는다. v0.8.1 업데이터가 그대로 받아들이는 것이 원래 목표였으나, 2026-10-03 코드 검토에서 새 구성요소의 거부를 확인했다. v0.8.1 내장 업데이트는 알려진 제한으로 기록하고 setup 직접 실행을 안내한다. 자동 업데이트 호환성을 완료로 기록하지 않는다. agent는 Control Center ZIP 안(`resources/suite/devbox-agent.exe`)에 들어 있어 자산 수가 늘지 않는다.
- 태그는 main의 해당 commit CI가 끝난 뒤 민다. 후보 실행부터 태그까지 main에 다른 머지를 넣지 않는다. 후보 artifact 보존은 7일(shard)·14일(조립본)이고 후보 성공 후 7일 안에 태그한다. 7일이 지나면 같은 commit으로 후보를 다시 만든다(실패가 아니라 만료다).
- 실패한 후보·release run을 새 빌드로 덮지 않는다. 원인을 고친 PR을 머지한 뒤 새 main commit으로 후보부터 다시 한다. 공개 RC/prerelease를 만들지 않는다.

## Review Focus

1. 신규 설치 → Control Center 초기 준비/활성화 → 네 제품·agent, 새 프로젝트·설정·API 자료·노트의 재시작 유지와 강제 종료 복구를 확인한다. 기존 데이터 보존/자동 이전은 이 결과로 대신하지 않는다. (Task 3 출시 후 점검표)
2. 공개 v0.8.1 reader의 새 manifest 거부를 릴리스 안내에 명시한다. 이번 전환은 setup 직접 실행이며 내장 업데이트를 보장하지 않는다. 신규 설치 PASS로 기존 데이터 보존을 완료 처리하지 않는다. (Task 5)
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

  현재 CI는 main push에 자동 실행되지 않는다. 최종 준비 변경을 모두 머지하고 source를 고정한 뒤
  아래 수동 실행을 한 번 시작한다. 같은 source의 성공 실행이 이미 있으면 그것을 확인한다.

```bash
gh workflow run ci.yml --ref main
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

### Task 3: 후보 근거와 출시 후 실사용 점검표

- [ ] **Step 1: 후보 자산 검증** — assembly artifact를 전용 임시 폴더로 내려받아 이름·크기·SHA-256·내부 구성요소를 확인한다. 아래 Windows 배치는 실제 사용자 점검을 요청받았을 때만 사용한다. 이번 공개는 자동 검증 후 진행하며 오래된 Windows 후보 사본은 정리한다.

```bash
WINHOME=$(wslpath -u "$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')")
DEST="$WINHOME/Downloads/devbox-v0.9.0-candidate"
gh run download $RUN -n <조립 artifact 이름> -D "$DEST"
ls "$DEST"    # Devbox_0.9.0_x64-setup.exe 와 네 제품 ZIP
```

- [ ] **Step 2: 대기 항목 모음** — ledger 댓글에서 "사용자 확인 대기"로 남은 실기 항목을 PR별로 모아(`gh issue view <ledger 번호> --comments`) 한 댓글로 정리한다. 아래 기존 필수 8개와 나머지는 모두 Task 5(공개 뒤)의 제보/추적 항목이다.

- [ ] **Step 3: 실사용 항목 기록** — 2026-10-03 사용자는 사용 중 문제를 직접 제보하고, 자체 검토·수정 뒤 태그·릴리스까지 진행하도록 지시했다. 아래 표는 ledger에 출시 후 추적으로 남기고 응답을 기다리지 않는다. 실행하지 않은 항목은 PASS가 아니다.

  현재 사용자 PC(기존 v0.8.1 없음)에서 `Downloads\devbox-v0.9.0-candidate\Devbox_0.9.0_x64-setup.exe` 신규 설치:
  1. 설치와 Control Center의 초기 저장소 준비·활성화가 끝나고 v0.9.0이 보이며 네 제품이 열린다.
  2. 알림 영역에 Devbox 아이콘이 하나만 있고, 제품을 모두 닫아도 `devbox-agent.exe`가 남아 있다(작업 관리자).
  3. Workspace: 새 테스트 프로젝트 등록·터미널 설정이 재시작 후 유지된다. 사용 가능한 LSP 설정도 확인한다. agent가 관리하는 무해한 작업을 실행한 채 UI를 닫았다 다시 열면 같은 실행과 로그가 이어진다.
  4. API Studio: 새 테스트 컬렉션·비밀이 아닌 환경 변수·요청을 저장하고 안전한 요청을 보낸다. 재실행 후 자료와 기록이 유지되고 저장된 요청을 다시 보낼 수 있다.
  5. Knowledge: 새 테스트 노트의 저장·재실행 유지 확인 후 내용을 고친다. 2초 기다린 뒤 해당 Knowledge만 강제 종료 → 다시 열면 저장돼 있거나 복구 배너에서 복구할 수 있다.
  6. API Studio 웹훅 리스너를 켠 채 API Studio를 닫고 WSL에서 `curl --max-time 10 -X POST http://127.0.0.1:<포트>/hook -d x` → 다시 연 API Studio 기록에 보인다. 검증 후 테스트 리스너만 중지한다.
  7. Workspace “에이전트”에서 전용 테스트 Git 프로젝트의 Claude Code 작업을 만들면 새 폴더·branch와 터미널의 `claude` 실행이 확인된다. 도구 미설치/인증 미완료는 확인 불가로 기록하고, 자신이 만든 작업만 버리기로 정리한다.
  8. 현재 설치의 네 제품마다 `%LOCALAPPDATA%\com.devbox.v08.*.i*\logs\`와 실행 기록이 생긴다.

  기존 설치 위 setup 실행, 기존 Workspace 설정·API 문서 자동 이전·Knowledge 데이터 보존은 별도 **미실행**이다. 실제로 확인하지 못한 항목은 “확인 불가”로 기록한다. 이 사용자 점검의 미실행은 자동 검증을 마친 후보의 태그·공개를 막지 않는다.

- [ ] **Step 4: 후보 결과와 제보 처리**
  - 자동 후보 검증이 모두 PASS이면 사용자 실사용 미실행/출시 후 추적을 ledger에 기록하고 Task 4.
  - 자동 검증 실패나 공개 전에 제보된 재현 가능한 결함은 원인을 조사해 수정 PR을 §4 절차로 머지한다(버전은 0.9.0 그대로). 새 `$SHA`로 Task 2 → Task 3을 다시 한다. 공개 뒤 제보는 별도 수정 릴리스에서 처리한다.
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

- [ ] **Step 1: 기존 설치 전환과 추가 실기** — 공개 v0.8.1 내장 업데이터의 manifest 거부는 코드로 확인된 제한이다. Release setup 직접 실행을 안내한다. 공개 뒤 기존 설치가 있는 PC의 데이터 보존·재실행·제품 연결은 사용자 제보로 확인한다. 현재 사용자 PC의 신규 설치 결과로 대신하지 않는다. Task 3 Step 2의 항목은 출시 후 추적으로 유지하며 작업 대기 조건으로 삼지 않는다. FAIL이면 수정 릴리스 필요성을 사용자와 정한다(§4.10).
- [ ] **Step 2: 근거 기록 PR** — 브랜치 `docs/release/v0.9.0-evidence`, PR 제목 `docs(release): record v0.9.0 evidence`로 §4 절차를 따른다:
  - `docs/release-evidence.md` 맨 위 현재 stable 절을 v0.9.0으로 바꾸고 v0.8.1 절은 "이전 stable"로 내린다. 값은 Task 4 Step 3의 ledger 댓글(머지 PR, source SHA, 후보 run, release run, 자산 7개 SHA-256, published-runtime 결과)과 Task 3의 사용자 점검 결과.
  - `docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md` §2.4 백로그 표에 각 항목의 현재 상태를 적는다.
- [ ] **Step 3: ledger 마무리** — 최종 댓글: 머지한 PR 목록(번호·제목), 릴리스 링크, 아직 대기인 실기 항목. 대기 항목이 없으면 `gh issue close <ledger 번호> --reason completed`, 있으면 최신 이슈 하나를 열린 상태로 유지하고 제목을 "v0.9.0 출시 후 이슈·실사용 추적"으로 정리한다.
