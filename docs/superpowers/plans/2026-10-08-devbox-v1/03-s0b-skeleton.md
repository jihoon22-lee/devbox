# S0b 걷는 뼈대 (1/2: 저장소·프로토콜·데몬) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: 계획 · 미착수 · 시작 조건: S0a 합격(또는 대체 경로로 01-design 갱신 완료)

**Goal:** 옛 코드를 걷어내고, WSL 데몬이 unix socket으로 타입 있는 RPC·구독을 제공하며, 브리지와 개발용 WebSocket gateway로 바깥에 연결되는 최소 동작 뼈대를 만든다. 첫 도메인은 `projects`다.

**Architecture:**
- `crates/protocol`: 프레임·메시지·메서드 선언·오류·라우터
- `crates/mux`: 창 ↔ 채널 라우팅(Windows 껍데기용, 플랫폼 독립)
- `crates/core`: 인스턴스·경로·설정·DB·이벤트 허브·로그
- `crates/projects`: 첫 도메인
- `crates/cli`: `devbox` 바이너리(daemon·bridge·dev-gateway·setup·doctor)
- `xtask`: TS 타입 생성기

화면과 Windows 껍데기는 [03b-s0b-app](03b-s0b-app.md)에서 만든다.

**Tech Stack:** Rust 1.98.1(edition 2024), tokio 1.53, tokio-util 0.7(codec), serde/serde_json(raw_value), thiserror 2, ts-rs 12.0.1, rusqlite 0.32(bundled), tracing·tracing-appender, clap 4, axum 0.8(ws), mimalloc, uuid.

**Spec:** [01-design.md](01-design.md) §3·§4·§5·§7·§9(protocol·mux·core·projects·cli)·§11·§13

## Global Constraints

- 작업은 `/home/jihoon/projects/devbox-wt/<브랜치>` 전용 worktree에서 한다. 원본 체크아웃은 `main`으로 둔다.
- 버전은 `1.0.0-dev`. Rust edition 2024, `rust-version = "1.98"`.
- 도메인 crate는 Tauri에 의존하지 않는다. 경계 타입으로 `serde_json::Value`와 `Result<T, String>`을 쓰지 않는다(RPC 파라미터의 `RawValue` 중계는 예외).
- 프레임: `u32 LE 길이(뒤 바이트 수, 0 금지)` + `u8 종류` + `u32 LE 채널` + 본문. JSON 하드 상한 16 MiB, 스트림 조각 64 KiB, 제어 4 KiB.
- `DEVBOX_INSTANCE`는 `^[a-z][a-z0-9-]{0,31}$`, 기본 `prod`. 소켓은 `$XDG_RUNTIME_DIR/devbox-<i>.sock`(없으면 `/run/user/<uid>`), 데이터는 `~/.local/share/devbox/<i>/`.
- 오류 코드는 `<도메인>.<이름>` 소문자 snake_case. 공통 코드만 접두사가 없다(`internal`, `invalid_params`, `method_not_found`, `hello_required`, `cancelled`, `daemon_restarting`, `not_found`, `secrets_locked`).
- 정수 ID·rev는 문자열로 보낸다. 시간은 `*_ms: i64`.
- 커밋은 영어 Conventional Commits. 과제마다 커밋한다.
- 이 계획의 코드에 컴파일 오류가 있으면, 같은 의도를 지키는 범위에서 고친다. 고친 내용은 PR 본문에 적는다.

## Review Focus

| 상황 | 기대 동작 | 시험 위치 |
|---|---|---|
| 프레임이 여러 TCP 조각으로 나뉘어 오거나 한 번에 여러 개가 옴 | 정확히 같은 프레임 순서로 복원 | Task 2 |
| 잘못된 프레임(길이 0, 상한 초과, 모르는 종류) 뒤에 정상 프레임이 와도 | 연결을 끊고 이후 프레임을 처리하지 않음(poisoning) | Task 2 |
| 구독자가 느려 큐가 찬 상태에서 이벤트가 계속 발행됨 | 발행자는 기다리지 않고, 그 구독에는 `resync` 하나만 감 | Task 9 |
| 같은 `clientRequestId`로 mutation을 두 번 보냄(재연결 재시도) | 두 번째는 실행하지 않고 첫 결과를 돌려줌 | Task 13 |
| 채널을 닫은 뒤 그 채널의 구독 이벤트 | 더 이상 보내지 않음 | Task 13 |
| 사용자 systemd가 다른 프로그램의 실패한 unit 때문에 `degraded`임(이 PC 실측: snap scope 실패) | setup·doctor가 정상으로 진행. systemd 판정은 PID 1과 `systemctl --user` 응답으로만 하고 `is-system-running == running`을 요구하지 않음(IR-9) | Task 15 |
| 새 CI job을 더한 PR | `ci-ok.needs`에 그 job이 있어 실패하면 머지가 막힘. 코드가 바뀐 PR에서는 `ci-ok`가 skipped로 통과하지 않음(IR-1) | Task 1·21 |
| `docs/`·Markdown만 바꾼 PR | `changes`가 `code=false`, 무거운 job은 건너뛰고 `ci-ok` 통과(1분 안). 코드가 한 줄이라도 섞이면 전부 실행 | Task 1 |

---

## PR 묶음

| PR | 브랜치 | 과제 |
|---|---|---|
| A | `chore/rebuild/clean-slate` | Task 1 |
| B | `feat/daemon/protocol` | Task 2–6 |
| C | `feat/daemon/core` | Task 7–15 |

---

### Task 1: 옛 코드 걷어내기와 새 저장소 골격

**Files:**
- Delete: 추적 중인 모든 파일. 예외: `docs/adr/`, `.gitignore`, `.nvmrc`, `rust-toolchain.toml`
- Create: `Cargo.toml`, `rust-toolchain.toml`(교체), `package.json`, `pnpm-workspace.yaml`, `biome.json`, `.gitattributes`, `README.md`, `AGENTS.md`, `CLAUDE.md`
- Create: `crates/protocol/Cargo.toml`, `crates/protocol/src/lib.rs`(골격)
- Create: `docs/adr/0017-rebuild-single-app-wsl-daemon.md`, `docs/ui-terms.md`
- Modify: `docs/adr/0001`–`0016`(상태 줄), `docs/adr/README.md`
- Keep: `docs/superpowers/plans/2026-10-08-devbox-v1/*`(P0에서 `docs/v1-plan`에 커밋됨. 이 PR이 그 브랜치에서 갈라지므로 함께 main에 들어간다. 지우지 않음)
- Modify: `.gitignore`(맨 위 `logs` 줄을 `/logs/`로 바꾼다. 그대로면 어느 깊이든 `logs`라는 폴더를 무시해, 나중에 `app/src/features/logs/` 같은 코드가 조용히 커밋에서 빠진다. `sed -i 's#^logs$#/logs/#' .gitignore`)
- Create: `scripts/banned-words.sh`, `scripts/sweep.sh`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces: Cargo workspace(`members = ["crates/*", "xtask"]`, 이후 과제가 crate를 추가), pnpm workspace(`app`), CI 워크플로 골격, 새 AGENTS.md 규칙

- [ ] **Step 1: worktree 만들기**

PR A는 계획 원본 브랜치 `docs/v1-plan`에서 갈라진다(00-roadmap §2 P0). 그래야 계획 폴더가 이 PR과 함께 main에 들어간다.

```bash
cd /home/jihoon/projects/devbox && git fetch origin main docs/v1-plan && git status --short
# main이 그 사이 움직였으면 계획 브랜치를 먼저 main 위로 올린다(계획 문서만 있어 충돌하지 않는다)
git merge-base --is-ancestor origin/main origin/docs/v1-plan || {
  git -C ../devbox-wt/docs-v1-plan pull --ff-only && git -C ../devbox-wt/docs-v1-plan rebase origin/main &&
  git -C ../devbox-wt/docs-v1-plan push --force-with-lease origin docs/v1-plan && git fetch origin docs/v1-plan; }
git worktree add ../devbox-wt/chore-rebuild-clean-slate -b chore/rebuild/clean-slate origin/docs/v1-plan
cd ../devbox-wt/chore-rebuild-clean-slate
```

- [ ] **Step 2: 옛 파일 삭제**

```bash
git ls-files | grep -vE '^(docs/adr/|docs/superpowers/plans/2026-10-08-devbox-v1/|\.gitignore$|\.nvmrc$|rust-toolchain\.toml$)' | xargs -d '\n' git rm -q --
git status --short | wc -l   # 수천 줄의 D
ls -A                        # .git .gitignore .nvmrc docs rust-toolchain.toml 만 남음(docs 안에는 adr와 이 계획 폴더)
```

- [ ] **Step 3: 계획 폴더 확인**

이 계획 폴더는 P0의 `docs/v1-plan` 커밋으로 이 브랜치에 들어 있고, Step 2의 삭제에서도 남긴다. 다른 계획 폴더(`docs/superpowers/plans/`의 옛 것)는 지워진다.

```bash
ls docs/superpowers/plans/                         # 2026-10-08-devbox-v1 하나
ls docs/superpowers/plans/2026-10-08-devbox-v1     # PROGRESS.md 00-roadmap.md … 09-s0a-results.md
```

- [ ] **Step 4: 루트 파일 작성**

```toml
# Cargo.toml
[workspace]
resolver = "3"
members = ["crates/*", "xtask"]

[workspace.package]
version = "1.0.0-dev"
edition = "2024"
rust-version = "1.98"
publish = false

[workspace.dependencies]
devbox-protocol = { path = "crates/protocol" }
devbox-mux = { path = "crates/mux" }
devbox-core = { path = "crates/core" }
devbox-projects = { path = "crates/projects" }
devbox-cli = { path = "crates/cli" }
anyhow = "1"
axum = { version = "0.8", features = ["ws"] }
bytes = "1"
clap = { version = "4", features = ["derive"] }
futures = "0.3"
libc = "0.2"
mimalloc = "0.1"
rusqlite = { version = "0.32", features = ["bundled", "backup"] }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["raw_value"] }
sha2 = "0.10"
tempfile = "3"
thiserror = "2"
tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "net", "io-util", "io-std", "sync", "time", "signal", "process", "fs"] }
tokio-tungstenite = "0.27"
tokio-util = { version = "0.7", features = ["codec", "rt"] }
toml = "0.9"
tracing = "0.1"
tracing-appender = "0.2"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
ts-rs = { version = "=12.0.1", features = ["serde-json-impl", "no-serde-warnings"] }
uuid = { version = "1", features = ["v4"] }

[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
dbg_macro = "deny"
unwrap_used = "warn"

[profile.dev]
debug = "line-tables-only"

[profile.release]
debug = "line-tables-only"
lto = "thin"
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.98.1"
components = ["clippy", "rustfmt"]
targets = ["x86_64-unknown-linux-musl"]
```

```json
// package.json
{
  "name": "devbox",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "engines": { "node": ">=24 <25" },
  "scripts": {
    "check": "bash scripts/check.sh",
    "dev": "bash scripts/dev.sh",
    "gen": "cargo run -q -p xtask -- gen-ts"
  },
  "devDependencies": { "@biomejs/biome": "2.5.14" }
}
```

```yaml
# pnpm-workspace.yaml
packages:
  - "app"
```

```json
// biome.json
{
  "$schema": "./node_modules/@biomejs/biome/configuration_schema.json",
  "vcs": { "enabled": true, "clientKind": "git", "useIgnoreFile": true },
  "files": {
    "includes": ["app/**/*.ts", "app/**/*.tsx", "app/**/*.css", "app/**/*.mjs", "!!**/node_modules", "!!**/dist", "!!app/src-tauri", "!!app/src/rpc/gen"]
  },
  "formatter": { "enabled": true, "indentStyle": "space", "indentWidth": 2, "lineWidth": 120 },
  "javascript": { "formatter": { "quoteStyle": "double", "semicolons": "always" } },
  "linter": {
    "enabled": true,
    "rules": {
      "preset": "none",
      "correctness": { "useExhaustiveDependencies": "error", "useHookAtTopLevel": "error", "noUnusedImports": "error" },
      "suspicious": { "noDoubleEquals": "error", "noFallthroughSwitchClause": "error" }
    }
  }
}
```

```gitattributes
# .gitattributes
* text=auto eol=lf
*.png binary
*.ico binary
*.woff2 binary
```

- [ ] **Step 5: AGENTS.md·CLAUDE.md·README.md 작성**

```markdown
<!-- AGENTS.md -->
# AGENTS.md

devbox는 Windows 11 + WSL에서 혼자 개발하는 사람을 위한 개인 개발 허브다. WSL 안의 Rust 데몬(`devbox`)이 모든 상태와 작업을 소유하고, Windows의 Tauri 앱(`Devbox.exe`)이 화면·트레이·단축키·알림을 맡는다.

- 설계 원장: `docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md`. 실행 순서·규칙: 같은 폴더의 `00-roadmap.md`. 진행 원장: 같은 폴더의 `PROGRESS.md`.
- 작업 범위는 사용자 지시와 그 로드맵의 미완료 계획을 따른다.
- 세션을 시작·끝낼 때 `00-roadmap.md` §3.1 인계 절차를 따른다. §3.2에 해당하면 멈추고 사용자에게 묻는다.

## 작업 방식
- 원본 체크아웃은 `main`으로 두고, 과제마다 `../devbox-wt/<브랜치>` worktree를 만든다. 브랜치: `feat|fix/<영역>/<범위>`, `chore/<범위>`, `docs/<범위>`.
- 로직은 실패 테스트부터 쓴다. 바꾼 crate·화면의 테스트만 먼저 돌린다: `cargo test -p <crate>`, `pnpm --filter app exec vitest run <파일>`.
- PR 전에 `pnpm check`를 한 번 돌린다. 필수 CI가 통과하면 squash 머지한다. 커밋은 영어 Conventional Commits.
- 진행 상태의 원장은 PR과 계획 문서 머리의 "상태" 줄이다. 체크박스는 보조다.

## 코드 규칙
- 도메인 crate는 Tauri에 의존하지 않는다. 오류는 `domain_error!` enum으로 생기는 곳에서 만든다. `Result<T, String>`과 `serde_json::Value` 경계 타입은 쓰지 않는다.
- RPC 메서드·주제는 `method!`·`topic!`으로 선언하고 `pnpm gen`으로 TS를 생성한다. 생성물은 손으로 고치지 않는다.
- 파일 600줄, React 컴포넌트 300줄을 넘으면 나눈다.
- 새 방어 장치나 검증 단계를 더하기 전에 구조로 없앨 수 있는지 먼저 본다. 렌더러 대상 session·route·replay 검사는 만들지 않는다(ADR 0016·0017).
- 화면 문구는 한국어, 작업·대상·결과 중심. 용어는 `docs/ui-terms.md`. 단축키는 `Ctrl`로 적는다.

## 검증
- flaky 테스트는 발견한 PR에서 고치거나 지운다. 재시도 래퍼를 만들지 않는다.
- `.github`와 `scripts`는 합쳐 1,500줄 이하, 워크플로 3개 이하를 유지한다.
- WSL에서 Rust 쓰기 전 `source ~/.cargo/env`. `CARGO_BUILD_JOBS=4`. 무거운 검증은 한 번에 하나.
- 사용자의 기존 tmux·systemd unit·Docker·방화벽을 테스트로 바꾸지 않는다. 테스트는 `DEVBOX_INSTANCE=test-<난수>`를 쓴다.

## 하위 에이전트
- 계획은 subagent-driven-development로 실행한다(과제별 구현 + 검토 하위 에이전트). 하위 에이전트도 전용 worktree에서 일한다.
```

```markdown
<!-- CLAUDE.md -->
# CLAUDE.md

이 저장소의 지침 원장은 AGENTS.md다.

@AGENTS.md
```

```markdown
<!-- README.md -->
# devbox

Windows 11 + WSL에서 여러 프로젝트를 AI 에이전트와 함께 열고·돌리고·지켜보고·기록하는 개인 개발 허브.

- 상태: v1.0.0 재구축 중. 이전 버전은 [v0.9.0 태그](https://github.com/jihoon22-lee/devbox/tree/v0.9.0)와 Release에 있다.
- 설계: [docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md](docs/superpowers/plans/2026-10-08-devbox-v1/01-design.md)

## 개발
- WSL: `pnpm install && pnpm dev` (데몬 `--instance dev` + 화면 http://localhost:1450)
- 전체 검사: `pnpm check`
- Windows 껍데기: `scripts/win-dev.ps1`
```

- [ ] **Step 6: ADR 정리**

`09-s0a-results.md` 끝의 "ADR 0017 초안"을 `docs/adr/0017-rebuild-single-app-wsl-daemon.md`로 옮긴다. 옛 ADR의 상태 줄을 바꾼다.

```bash
for n in 0001 0002 0003 0004 0007 0009 0010 0011 0012 0014 0015; do
  f=$(ls docs/adr/${n}-*.md); sed -i -E '0,/^상태: .*/s//상태: 대체됨(0017)/' "$f"
done
for n in 0005 0006 0008 0013 0016; do
  f=$(ls docs/adr/${n}-*.md); sed -i -E '0,/^상태: (.*)/s//상태: \1 · 0017에서 형태 조정(01-design §13)/' "$f"
done
grep -h '^상태:' docs/adr/0*.md
```

`docs/adr/README.md` 표에 `| 0017 | [재구축: 단일 앱 + WSL 데몬](0017-rebuild-single-app-wsl-daemon.md) | 채택 |`을 추가하고, 다른 행의 상태 열을 위와 같이 고친다.

- [ ] **Step 7: 용어 표·스크립트·protocol 골격·CI**

```markdown
<!-- docs/ui-terms.md -->
# 화면 용어

| 쓰는 말 | 뜻 | 쓰지 않는 말 |
|---|---|---|
| 에이전트 작업 | Claude Code·Codex 등이 worktree에서 하는 일 하나 | 태스크, 세션(단독) |
| 작업 | `devbox.toml`의 `[[task]]` 한 번 실행하는 명령 | 잡, 런 |
| 서비스 | 계속 떠 있는 실행(`[[service]]`) | 데몬(사용자 화면에서) |
| 예약 | 정해진 시각에 작업을 실행하는 규칙 | 스케줄 잡 |
| 실행 | 작업·서비스를 한 번 돌린 것 | run, receipt |
| 진행 작업 | 오래 걸리는 데몬 작업(설치·색인 등) | operation |
| 작업 폴더 | git worktree | 워크트리(본문 설명 외) |
| 데몬 | WSL 안의 devbox 백그라운드 프로그램(진단·설정 화면에서만) | agent, suite |
| 연결 | 앱과 데몬의 연결 | 세션, 핸드셰이크 |
```

```bash
# scripts/banned-words.sh
#!/usr/bin/env bash
# 화면 문자열에 옛 앱 이름·내부 용어·Mac 기호가 들어가지 않게 한다.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
[ -d "$root/app/src" ] || { echo "banned-words: app/src 없음, 건너뜀"; exit 0; }
pattern='Workbench|Code Pad|Repo Manager|Run Manager|Log Lens|Port Manager|⌘|provenance|receipt|generation|route'
if grep -rnE "$pattern" "$root/app/src" --include='*.tsx' --include='*.ts' \
     --exclude-dir=gen --exclude='*.test.ts' --exclude='*.test.tsx' \
   | grep -vE '^\S+:\s*//|import |from "|TanStack|createRoute|Route\b|useRoute|router'; then
  echo "banned-words: 위 줄의 화면 문자열을 고쳐 주세요(docs/ui-terms.md)." >&2
  exit 1
fi
echo "banned-words: ok"
```

```bash
# scripts/sweep.sh
#!/usr/bin/env bash
# 전역 cargo target-dir 정리(00-roadmap §3 자원). 다른 세션의 cargo·rustc가 돌고 있으면 하지 않는다.
set -euo pipefail
cd "$(dirname "$0")/.."
if pgrep -x cargo >/dev/null || pgrep -x rustc >/dev/null; then
  echo "cargo/rustc 실행 중이라 정리하지 않습니다. 나중에 다시 실행하세요." >&2
  exit 1
fi
target="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
du -sh "$target" 2>/dev/null || true
# worktree 경로마다 따로 쌓이는 incremental 결과물이 가장 크다(2026-09 590 GiB의 절반). 3일 넘은 것을 지운다.
find "$target" -maxdepth 3 -type d -name incremental -print0 |
  while IFS= read -r -d '' d; do find "$d" -mindepth 1 -maxdepth 1 -mtime +3 -exec rm -rf {} +; done
# 나머지 산출물은 cargo-sweep으로 14일 넘게 안 쓴 것을 지운다(target-dir은 cargo metadata로 찾음).
if command -v cargo-sweep >/dev/null; then cargo sweep --time 14; else echo "cargo-sweep 미설치: cargo install cargo-sweep"; fi
du -sh "$target" 2>/dev/null || true
```

```toml
# crates/protocol/Cargo.toml (골격 — Task 2부터 채운다)
[package]
name = "devbox-protocol"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[lints]
workspace = true
```

```rust
// crates/protocol/src/lib.rs (골격)
//! devbox 데몬과 클라이언트 사이의 프레임·메시지·메서드 계약.

/// 클라이언트와 데몬이 같은 빌드에서 나왔는지 확인하는 프로토콜 번호.
pub const PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    #[test]
    fn protocol_version_is_one() {
        assert_eq!(super::PROTOCOL_VERSION, 1);
    }
}
```

xtask가 아직 없으므로 이 과제에서만 `Cargo.toml`의 members를 `["crates/*"]`로 두고, Task 6에서 `"xtask"`를 추가한다.

```yaml
# .github/workflows/ci.yml
name: CI
on:
  pull_request:
  push:
    branches: [main]
  workflow_dispatch:
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
jobs:
  # 바뀐 파일이 docs/ 와 루트 Markdown뿐이면 무거운 job을 건너뛴다(계획 갱신에 전체 CI를 돌리지 않음, 사용자 요청 2026-10-08).
  changes:
    runs-on: ubuntu-24.04
    timeout-minutes: 2
    outputs:
      code: ${{ steps.diff.outputs.code }}
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - id: diff
        env:
          EVENT: ${{ github.event_name }}
          BASE: ${{ github.event.pull_request.base.sha || github.event.before }}
        run: |
          if [ "$EVENT" = "workflow_dispatch" ] || [ -z "$BASE" ] || ! git cat-file -e "$BASE^{commit}" 2>/dev/null; then
            echo "code=true" >> "$GITHUB_OUTPUT"
          elif git diff --name-only "$BASE" HEAD | grep -qvE '^(docs/|[^/]+\.md$)'; then
            echo "code=true" >> "$GITHUB_OUTPUT"
          else
            echo "code=false" >> "$GITHUB_OUTPUT"
          fi
  rust:
    needs: changes
    if: needs.changes.outputs.code == 'true'
    runs-on: ubuntu-24.04
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.98.1
        with: { components: "clippy, rustfmt" }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: bash scripts/banned-words.sh
  # 보호 규칙의 유일한 필수 검사(IR-1). job을 더하면 needs에도 더하고, 그 job에 `needs: changes`와 같은 `if`를 단다.
  # needs가 실패하면 이 job이 skipped가 되어 필수 검사가 통과로 보이는 것을 막으려고 always()로 돌리고 결과를 직접 본다.
  # 문서만 바뀐 경우(code=false)에만 changes 밖 job의 skipped를 통과로 인정한다.
  ci-ok:
    if: always()
    needs: [changes, rust]
    runs-on: ubuntu-24.04
    timeout-minutes: 2
    steps:
      - env:
          NEEDS: ${{ toJSON(needs) }}
          CODE: ${{ needs.changes.outputs.code }}
        run: |
          echo "$NEEDS" | jq -e --arg code "$CODE" \
            'to_entries | all(.value.result == "success" or ($code == "false" and .key != "changes" and .value.result == "skipped"))'
```

- [ ] **Step 8: 확인과 커밋**

```bash
chmod +x scripts/*.sh
source ~/.cargo/env && cargo test --workspace && bash scripts/banned-words.sh
git add -A && git commit -m "chore: start v1 rebuild from a clean tree

Remove the v0.9.0 sources, CI and process documents (preserved by the
v0.9.0 tag) and add the rebuild workspace skeleton, AGENTS.md and ADR 0017.
The v1 plan folder added in P0 is kept."
```

Expected: 테스트 1개 통과, `banned-words: app/src 없음, 건너뜀`.

- [ ] **Step 9: PR**

```bash
git push -u origin chore/rebuild/clean-slate
gh pr create --title "chore: start v1 rebuild from a clean tree" --body-file - <<'EOF'
## 요약
- v0.9.0 소스·CI·과정 문서를 걷어내고(태그 v0.9.0으로 보존) 재구축 골격을 만든다.
- 새 AGENTS.md(한 장)와 ADR 0017을 넣는다. P0에서 넣은 v1 계획 폴더는 남긴다.
- main 보호 규칙의 필수 검사를 `ci-ok` 하나로 바꾼다(Step 10).

## 검증
- `cargo test --workspace`, `scripts/banned-words.sh`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
```

- [ ] **Step 10: 보호 규칙의 필수 검사를 `ci-ok` 하나로 바꾸기 (IR-1)**

2026-10-08 현재 main의 필수 검사는 옛 job 이름 셋(`Frontend (pnpm)`·`Rust (Cargo workspace)`·`Rust (Windows)`)이다. 이 PR에는 그 job이 없으므로 바꾸지 않으면 머지가 영원히 막힌다. PR의 CI가 모두 초록인 것을 본 뒤에 바꾼다.

```bash
gh api repos/jihoon22-lee/devbox/branches/main/protection/required_status_checks --jq '.contexts'
gh api -X PATCH repos/jihoon22-lee/devbox/branches/main/protection/required_status_checks \
  -F strict=false -f 'contexts[]=ci-ok'
gh api repos/jihoon22-lee/devbox/branches/main/protection/required_status_checks --jq '.contexts'
```

Expected: 첫 줄은 옛 이름 셋, 마지막 줄은 `["ci-ok"]`. 권한 오류(403)가 나면 사용자에게 저장소 설정 → Branches → main 규칙의 필수 검사를 `ci-ok` 하나로 바꿔 달라고 요청하고 기다린다. 우회 머지(`--admin`)는 하지 않는다.

CI 통과와 보호 규칙 전환 뒤 squash 머지한다. worktree와 브랜치를 정리한다(00-roadmap §3). 계획 원본 브랜치 `docs/v1-plan`과 worktree `../devbox-wt/docs-v1-plan`도 정리한다. 이제 계획 원본은 main이다(PROGRESS "현재 위치"를 같은 PR에서 고친다).

---

### Task 2: 프레임 코덱

**Files:**
- Modify: `crates/protocol/Cargo.toml`
- Create: `crates/protocol/src/frame.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Produces:
  - `FrameKind { Json = 0, Stream = 1, Control = 2 }`
  - `Frame { kind, channel: u32, body: Bytes }`
  - `FrameCodec`(tokio-util `Decoder`/`Encoder`)
  - `CodecError`, `FrameError`
  - 상수 `HEADER_LEN = 9`, `MAX_JSON_BYTES = 16 MiB`, `SOFT_JSON_BYTES = 1 MiB`, `MAX_STREAM_CHUNK = 64 KiB`, `MAX_CONTROL_BYTES = 4 KiB`
  - `fn frame_channel(raw: &[u8]) -> Option<u32>`, `fn set_frame_channel(raw: &mut [u8], channel: u32) -> bool`
  - Task 3·5·11·12가 사용한다.

- [ ] **Step 0: worktree**

```bash
cd /home/jihoon/projects/devbox && git fetch origin main
git worktree add ../devbox-wt/feat-daemon-protocol -b feat/daemon/protocol origin/main && cd ../devbox-wt/feat-daemon-protocol
```

- [ ] **Step 1: 의존성**

```toml
# crates/protocol/Cargo.toml 의 [package] 아래에 추가
[dependencies]
bytes.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio-util.workspace = true
tracing.workspace = true
ts-rs.workspace = true
uuid.workspace = true

[dev-dependencies]
tokio.workspace = true
```

- [ ] **Step 2: 실패하는 테스트 작성**

```rust
// crates/protocol/src/frame.rs (테스트 먼저, 구현은 Step 4)
#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    fn json(channel: u32, text: &str) -> Frame {
        Frame { kind: FrameKind::Json, channel, body: Bytes::copy_from_slice(text.as_bytes()) }
    }
    fn encode(frames: &[Frame]) -> BytesMut {
        let mut codec = FrameCodec::default();
        let mut out = BytesMut::new();
        for f in frames { codec.encode(f.clone(), &mut out).expect("encode"); }
        out
    }

    #[test]
    fn round_trips_several_frames_fed_one_byte_at_a_time() {
        let frames = vec![json(0, "{\"a\":1}"), json(7, "{}"), Frame { kind: FrameKind::Stream, channel: 3, body: Bytes::from_static(&[0, 1, 2, 255]) }];
        let bytes = encode(&frames);
        let mut codec = FrameCodec::default();
        let mut buf = BytesMut::new();
        let mut got = Vec::new();
        for b in bytes.iter() {
            buf.extend_from_slice(&[*b]);
            while let Some(f) = codec.decode(&mut buf).expect("decode") { got.push(f); }
        }
        assert_eq!(got, frames);
    }

    #[test]
    fn decodes_two_frames_from_one_buffer() {
        let mut buf = encode(&[json(1, "1"), json(2, "2")]);
        let mut codec = FrameCodec::default();
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().channel, 1);
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().channel, 2);
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn zero_length_poisons_the_decoder() {
        let mut buf = BytesMut::from(&[0u8, 0, 0, 0][..]);
        buf.extend_from_slice(&encode(&[json(0, "{}")]));
        let mut codec = FrameCodec::default();
        assert!(matches!(codec.decode(&mut buf), Err(CodecError::Frame(FrameError::Empty))));
        assert!(matches!(codec.decode(&mut buf), Err(CodecError::Frame(FrameError::Poisoned))));
    }

    #[test]
    fn rejects_unknown_kind_and_oversized_chunks() {
        let mut bad = BytesMut::new();
        bad.extend_from_slice(&5u32.to_le_bytes());
        bad.extend_from_slice(&[9, 0, 0, 0, 0]);
        assert!(matches!(FrameCodec::default().decode(&mut bad), Err(CodecError::Frame(FrameError::UnknownKind(9)))));

        let mut big = BytesMut::new();
        let len = (5 + MAX_STREAM_CHUNK + 1) as u32;
        big.extend_from_slice(&len.to_le_bytes());
        big.extend_from_slice(&[FrameKind::Stream as u8, 0, 0, 0, 0]);
        assert!(matches!(FrameCodec::default().decode(&mut big), Err(CodecError::Frame(FrameError::TooLarge { .. }))));
    }

    #[test]
    fn encoder_refuses_oversized_bodies() {
        let mut out = BytesMut::new();
        let f = Frame { kind: FrameKind::Control, channel: 0, body: Bytes::from(vec![b' '; MAX_CONTROL_BYTES + 1]) };
        assert!(FrameCodec::default().encode(f, &mut out).is_err());
    }

    #[test]
    fn reads_and_rewrites_the_channel_in_raw_bytes() {
        let mut raw = encode(&[json(4, "{}")]).to_vec();
        assert_eq!(frame_channel(&raw), Some(4));
        assert!(set_frame_channel(&mut raw, 9));
        assert_eq!(frame_channel(&raw), Some(9));
        assert_eq!(frame_channel(&raw[..5]), None);
    }
}
```

- [ ] **Step 3: 실패 확인**

Run: `cargo test -p devbox-protocol frame`
Expected: 컴파일 실패(`Frame` 등 미정의)

- [ ] **Step 4: 구현**

```rust
// crates/protocol/src/frame.rs (테스트 모듈 위에)
//! 길이 접두 프레임: u32 LE 길이(뒤따르는 바이트 수) + u8 종류 + u32 LE 채널 + 본문.
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

pub const HEADER_LEN: usize = 9;
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const SOFT_JSON_BYTES: usize = 1024 * 1024;
pub const MAX_STREAM_CHUNK: usize = 64 * 1024;
pub const MAX_CONTROL_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Json = 0,
    Stream = 1,
    Control = 2,
}

impl FrameKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Json),
            1 => Some(Self::Stream),
            2 => Some(Self::Control),
            _ => None,
        }
    }
    /// 본문 상한. 스트림 본문은 스트림 ID(4) + 플래그(1) + 데이터.
    pub fn max_body(self) -> usize {
        match self {
            Self::Json => MAX_JSON_BYTES,
            Self::Stream => 5 + MAX_STREAM_CHUNK,
            Self::Control => MAX_CONTROL_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub channel: u32,
    pub body: Bytes,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame length is zero")]
    Empty,
    #[error("frame length {0} is shorter than its header")]
    Short(usize),
    #[error("unknown frame kind {0}")]
    UnknownKind(u8),
    #[error("{kind:?} frame body is {len} bytes, limit {limit}")]
    TooLarge { kind: FrameKind, len: usize, limit: usize },
    #[error("decoder stopped after an earlier malformed frame")]
    Poisoned,
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Default)]
pub struct FrameCodec {
    poisoned: bool,
}

impl FrameCodec {
    fn fail(&mut self, e: FrameError) -> CodecError {
        self.poisoned = true;
        e.into()
    }
}

impl Decoder for FrameCodec {
    type Item = Frame;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, CodecError> {
        if self.poisoned {
            return Err(FrameError::Poisoned.into());
        }
        if src.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if len == 0 {
            return Err(self.fail(FrameError::Empty));
        }
        if len < 5 {
            return Err(self.fail(FrameError::Short(len)));
        }
        if src.len() < 5 {
            return Ok(None);
        }
        let Some(kind) = FrameKind::from_u8(src[4]) else {
            return Err(self.fail(FrameError::UnknownKind(src[4])));
        };
        let body_len = len - 5;
        if body_len > kind.max_body() {
            return Err(self.fail(FrameError::TooLarge { kind, len: body_len, limit: kind.max_body() }));
        }
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        src.advance(5);
        let channel = src.get_u32_le();
        let body = src.split_to(body_len).freeze();
        Ok(Some(Frame { kind, channel, body }))
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = CodecError;

    fn encode(&mut self, f: Frame, dst: &mut BytesMut) -> Result<(), CodecError> {
        if f.body.len() > f.kind.max_body() {
            return Err(FrameError::TooLarge { kind: f.kind, len: f.body.len(), limit: f.kind.max_body() }.into());
        }
        dst.reserve(HEADER_LEN + f.body.len());
        dst.put_u32_le((5 + f.body.len()) as u32);
        dst.put_u8(f.kind as u8);
        dst.put_u32_le(f.channel);
        dst.put_slice(&f.body);
        Ok(())
    }
}

/// 완성된 프레임 바이트에서 채널 번호를 읽는다(본문은 해석하지 않음).
pub fn frame_channel(raw: &[u8]) -> Option<u32> {
    (raw.len() >= HEADER_LEN).then(|| u32::from_le_bytes([raw[5], raw[6], raw[7], raw[8]]))
}

/// 완성된 프레임 바이트의 채널 번호를 바꾼다.
pub fn set_frame_channel(raw: &mut [u8], channel: u32) -> bool {
    if raw.len() < HEADER_LEN {
        return false;
    }
    raw[5..9].copy_from_slice(&channel.to_le_bytes());
    true
}
```

```rust
// crates/protocol/src/lib.rs 에 추가
pub mod frame;
```

- [ ] **Step 5: 통과 확인과 커밋**

Run: `cargo test -p devbox-protocol frame` → 6 passed

```bash
git add crates/protocol && git commit -m "feat(protocol): add length-prefixed frame codec with channel header"
```

---

### Task 3: 메시지 봉투와 공용 예시 파일

**Files:**
- Create: `crates/protocol/src/message.rs`
- Create: `crates/protocol/fixtures/envelopes.json`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 2의 `Frame`, `FrameKind`
- Produces:
  - `ClientKind { App, Cli, Mcp, Dev, Test }`
  - `ClientMessage { Hello, Request, Cancel, Subscribe, Unsubscribe }`
  - `ServerMessage { Welcome, Response, Subscribed, Event, Resync }`
  - `Control { ChannelOpen, ChannelClose, StreamCredit }`
  - `WireError { code, detail }`
  - `fn json_frame<T: Serialize>(channel, &T) -> Frame`, `fn control_frame(channel, &Control) -> Frame`, `fn parse_json<T>(&Frame) -> Result<T, serde_json::Error>`
  - `envelopes.json`: 03b의 TS 클라이언트 테스트도 같은 파일을 읽는다.

- [ ] **Step 1: 예시 파일 작성**

```json
// crates/protocol/fixtures/envelopes.json
{
  "client": {
    "hello": { "type": "hello", "version": "1.0.0-dev", "protocol": 1, "client": "app", "instance": "prod" },
    "request": { "type": "request", "id": 3, "method": "projects.add", "params": { "path": "/home/u/p" }, "clientRequestId": "c-1" },
    "requestNoClientId": { "type": "request", "id": 4, "method": "projects.list", "params": {} },
    "cancel": { "type": "cancel", "id": 3 },
    "subscribe": { "type": "subscribe", "id": 5, "topic": "projects.changed" },
    "unsubscribe": { "type": "unsubscribe", "subId": 2 }
  },
  "server": {
    "welcome": { "type": "welcome", "version": "1.0.0-dev", "protocol": 1, "daemonId": "d-1" },
    "ok": { "type": "response", "id": 3, "result": { "id": "p1" } },
    "err": { "type": "response", "id": 3, "error": { "code": "projects.already_exists", "detail": { "path": "/home/u/p" } } },
    "internal": { "type": "response", "id": 4, "error": { "code": "internal" }, "diagnosticId": "a1b2c3d4" },
    "subscribed": { "type": "subscribed", "id": 5, "subId": 2, "rev": "41" },
    "event": { "type": "event", "subId": 2, "rev": "42", "payload": { "rev": "42" } },
    "resync": { "type": "resync", "subId": 2 }
  },
  "control": {
    "open": { "type": "channelOpen" },
    "close": { "type": "channelClose" },
    "credit": { "type": "streamCredit", "stream": 7, "bytes": 262144 }
  }
}
```

- [ ] **Step 2: 실패하는 테스트**

```rust
// crates/protocol/src/message.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixtures() -> Value {
        serde_json::from_str(include_str!("../fixtures/envelopes.json")).expect("fixture json")
    }
    fn round_trip<T: Serialize + for<'de> Deserialize<'de>>(v: &Value) {
        let parsed: T = serde_json::from_value(v.clone()).expect("parse fixture");
        let back = serde_json::to_value(&parsed).expect("serialize");
        assert_eq!(&back, v);
    }

    #[test]
    fn every_fixture_round_trips_exactly() {
        let f = fixtures();
        for (_, v) in f["client"].as_object().unwrap() { round_trip::<ClientMessage>(v); }
        for (_, v) in f["server"].as_object().unwrap() { round_trip::<ServerMessage>(v); }
        for (_, v) in f["control"].as_object().unwrap() { round_trip::<Control>(v); }
    }

    #[test]
    fn json_frame_carries_channel_and_body() {
        let f = json_frame(6, &Control::ChannelOpen);
        assert_eq!(f.kind, FrameKind::Json);
        assert_eq!(f.channel, 6);
        assert_eq!(parse_json::<Control>(&f).unwrap(), Control::ChannelOpen);
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-protocol message` → 컴파일 실패

- [ ] **Step 4: 구현**

```rust
// crates/protocol/src/message.rs (테스트 위)
//! JSON 메시지 봉투. 메서드별 파라미터·결과는 RawValue로 그대로 싣는다.
use crate::frame::{Frame, FrameKind};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientKind {
    App,
    Cli,
    Mcp,
    Dev,
    Test,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ClientMessage {
    Hello { version: String, protocol: u32, client: ClientKind, instance: String },
    Request {
        id: u32,
        method: String,
        params: Box<RawValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_request_id: Option<String>,
    },
    Cancel { id: u32 },
    Subscribe {
        id: u32,
        topic: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<Box<RawValue>>,
    },
    Unsubscribe { sub_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerMessage {
    Welcome { version: String, protocol: u32, daemon_id: String },
    Response {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Box<RawValue>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<WireError>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagnostic_id: Option<String>,
    },
    Subscribed { id: u32, sub_id: u32, rev: String },
    Event { sub_id: u32, rev: String, payload: Box<RawValue> },
    Resync { sub_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Control {
    ChannelOpen,
    ChannelClose,
    StreamCredit { stream: u32, bytes: u32 },
}

pub fn json_frame<T: Serialize>(channel: u32, msg: &T) -> Frame {
    let body = serde_json::to_vec(msg).expect("protocol messages always serialize");
    Frame { kind: FrameKind::Json, channel, body: Bytes::from(body) }
}

pub fn control_frame(channel: u32, msg: &Control) -> Frame {
    let mut f = json_frame(channel, msg);
    f.kind = FrameKind::Control;
    f
}

pub fn parse_json<T: for<'de> Deserialize<'de>>(f: &Frame) -> Result<T, serde_json::Error> {
    serde_json::from_slice(&f.body)
}
```

`WireError`의 `detail`은 도메인 오류 enum을 직렬화한 결과를 그대로 싣는 자리라 `Value`를 쓴다. 생성된 TS 타입이 도메인별 `detail` 형태를 정하므로 경계 타입 규칙의 예외로 둔다.

```rust
// lib.rs
pub mod message;
```

- [ ] **Step 5: 통과·커밋**

Run: `cargo test -p devbox-protocol message` → 2 passed

```bash
git add crates/protocol && git commit -m "feat(protocol): add message envelopes with shared fixtures"
```

---

### Task 4: 메서드·주제 선언, 도메인 오류, 라우터

**Files:**
- Create: `crates/protocol/src/method.rs`, `crates/protocol/src/error.rs`, `crates/protocol/src/router.rs`, `crates/protocol/src/system.rs`
- Modify: `crates/protocol/src/lib.rs`, `crates/protocol/Cargo.toml`(tokio-util `rt`는 이미 포함)

**Interfaces:**
- Consumes: Task 3의 `WireError`
- Produces:
  - `MethodKind { Query, Mutation, Subscription, Stream }`, `Confirm { None, Always, Reversible }`
  - `trait Method { NAME, KIND, CONFIRM, Params, Output, Error }`
  - `trait Topic { NAME, Payload }`
  - `trait DomainError { CODES }`
  - 매크로 `method!`, `topic!`, `domain_error!`
  - `CommonError`, `Fail<E> { Domain(E), Internal(String) }`, `trait Internal`(`.internal("what")`)
  - `Ctx { channel, cancel }`, `DispatchError { error, diagnostic_id }`
  - `Router<S>`: `new`, `add::<M, _, _>`, `dispatch`, `kind_of`, `names`
  - `trait Exporter { method::<M>(), topic::<T>() }`, `NameCollector`
  - `Done { rev }`
  - `system` 모듈: `Ping`, `Info`, `SystemError`, `export`

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/protocol/src/router.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain_error, method};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Deserialize, ts_rs::TS)]
    pub struct EchoParams { pub text: String }
    #[derive(Serialize, ts_rs::TS)]
    pub struct EchoResult { pub text: String }
    domain_error! {
        pub enum EchoError {
            Empty = "echo.empty",
            TooLong { limit: u32 } = "echo.too_long",
        }
    }
    method!(pub Echo = "echo.say", query, EchoParams => EchoResult, EchoError);
    method!(pub EchoSet = "echo.set", mutation(always), EchoParams => EchoResult, EchoError);

    struct State;
    fn router() -> Router<State> {
        Router::new(Arc::new(State))
            .add::<Echo, _, _>(|_, _, p| async move {
                if p.text.is_empty() { return Err(EchoError::Empty.into()); }
                if p.text == "boom" { return Err(Fail::Internal("disk on fire".into())); }
                if p.text.len() > 5 { return Err(EchoError::TooLong { limit: 5 }.into()); }
                Ok(EchoResult { text: p.text })
            })
    }
    fn raw(s: &str) -> Box<serde_json::value::RawValue> { serde_json::value::RawValue::from_string(s.into()).unwrap() }
    fn ctx() -> Ctx { Ctx { channel: 0, cancel: tokio_util::sync::CancellationToken::new() } }

    #[tokio::test]
    async fn dispatches_ok_domain_internal_and_bad_params() {
        let r = router();
        let ok = r.dispatch("echo.say", ctx(), raw(r#"{"text":"hi"}"#)).await.unwrap();
        assert_eq!(ok.get(), r#"{"text":"hi"}"#);

        let e = r.dispatch("echo.say", ctx(), raw(r#"{"text":"toolong"}"#)).await.unwrap_err();
        assert_eq!(e.error.code, "echo.too_long");
        assert_eq!(e.error.detail, Some(serde_json::json!({"limit": 5})));
        assert!(e.diagnostic_id.is_none());

        let e = r.dispatch("echo.say", ctx(), raw(r#"{"text":"boom"}"#)).await.unwrap_err();
        assert_eq!(e.error.code, "internal");
        assert!(e.diagnostic_id.is_some());

        let e = r.dispatch("echo.say", ctx(), raw(r#"{"nope":1}"#)).await.unwrap_err();
        assert_eq!(e.error.code, "invalid_params");

        let e = r.dispatch("echo.unknown", ctx(), raw("{}")).await.unwrap_err();
        assert_eq!(e.error.code, "method_not_found");
    }

    #[tokio::test]
    async fn cancellation_wins_over_a_slow_handler() {
        let r = Router::new(Arc::new(State)).add::<Echo, _, _>(|_, _, _| async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok(EchoResult { text: String::new() })
        });
        let c = ctx();
        let cancel = c.cancel.clone();
        let fut = r.dispatch("echo.say", c, raw(r#"{"text":"x"}"#));
        cancel.cancel();
        assert_eq!(fut.await.unwrap_err().error.code, "cancelled");
    }

    #[test]
    fn metadata_and_codes() {
        assert_eq!(<EchoSet as Method>::KIND, MethodKind::Mutation);
        assert_eq!(<EchoSet as Method>::CONFIRM, Confirm::Always);
        assert_eq!(<Echo as Method>::CONFIRM, Confirm::None);
        assert_eq!(<EchoError as DomainError>::CODES, &["echo.empty", "echo.too_long"]);
        assert_eq!(router().kind_of("echo.say"), Some(MethodKind::Query));
    }

    #[test]
    #[should_panic(expected = "duplicate method echo.say")]
    fn duplicate_registration_panics() {
        let _ = router().add::<Echo, _, _>(|_, _, p| async move { Ok(EchoResult { text: p.text }) });
    }
}
```

```toml
# crates/protocol/Cargo.toml [dev-dependencies]에 추가
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "time"] }
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-protocol router` → 컴파일 실패

- [ ] **Step 3: 구현 — method.rs**

```rust
// crates/protocol/src/method.rs
use serde::{de::DeserializeOwned, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MethodKind {
    Query,
    Mutation,
    Subscription,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Confirm {
    None,
    Always,
    Reversible,
}

/// 도메인 오류 enum. `domain_error!`가 구현한다.
pub trait DomainError: std::error::Error + Serialize + ts_rs::TS + Send + Sync + 'static {
    const CODES: &'static [&'static str];
}

pub trait Method: 'static {
    const NAME: &'static str;
    const KIND: MethodKind;
    const CONFIRM: Confirm;
    type Params: DeserializeOwned + ts_rs::TS + Send + 'static;
    type Output: Serialize + ts_rs::TS + Send + 'static;
    type Error: DomainError;
}

pub trait Topic: 'static {
    const NAME: &'static str;
    type Payload: Serialize + ts_rs::TS + Send + Sync + 'static;
}

/// 모든 mutation이 결과가 따로 없을 때 돌려주는 값.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, ts_rs::TS)]
pub struct Done {
    pub rev: String,
}

/// 선언된 메서드·주제를 순회하는 쪽(TS 생성기, 라우터 완전성 시험)이 구현한다.
pub trait Exporter {
    fn method<M: Method>(&mut self);
    fn topic<T: Topic>(&mut self);
}

#[derive(Default, Debug)]
pub struct NameCollector {
    pub methods: Vec<&'static str>,
    pub topics: Vec<&'static str>,
}

impl Exporter for NameCollector {
    fn method<M: Method>(&mut self) {
        self.methods.push(M::NAME);
    }
    fn topic<T: Topic>(&mut self) {
        self.topics.push(T::NAME);
    }
}

#[macro_export]
macro_rules! method {
    ($vis:vis $ty:ident = $name:literal, $kind:ident $( ( $confirm:ident ) )?, $params:ty => $output:ty, $error:ty) => {
        $vis struct $ty;
        impl $crate::method::Method for $ty {
            const NAME: &'static str = $name;
            const KIND: $crate::method::MethodKind = $crate::__method_kind!($kind);
            const CONFIRM: $crate::method::Confirm = $crate::__method_confirm!($($confirm)?);
            type Params = $params;
            type Output = $output;
            type Error = $error;
        }
    };
}

#[macro_export]
macro_rules! topic {
    ($vis:vis $ty:ident = $name:literal, $payload:ty) => {
        $vis struct $ty;
        impl $crate::method::Topic for $ty {
            const NAME: &'static str = $name;
            type Payload = $payload;
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __method_kind {
    (query) => { $crate::method::MethodKind::Query };
    (mutation) => { $crate::method::MethodKind::Mutation };
    (subscription) => { $crate::method::MethodKind::Subscription };
    (stream) => { $crate::method::MethodKind::Stream };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __method_confirm {
    () => { $crate::method::Confirm::None };
    (none) => { $crate::method::Confirm::None };
    (always) => { $crate::method::Confirm::Always };
    (reversible) => { $crate::method::Confirm::Reversible };
}
```

- [ ] **Step 4: 구현 — error.rs**

```rust
// crates/protocol/src/error.rs
use crate::message::WireError;
use serde::Serialize;

/// `#[serde(tag = "code", content = "detail")]` enum을 만든다.
/// 세부 필드 이름은 한 단어 소문자로 쓴다(TS와 그대로 맞추기 위해).
#[macro_export]
macro_rules! domain_error {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $( $variant:ident $( { $($field:ident : $fty:ty),* $(,)? } )? = $code:literal ),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::ts_rs::TS, ::thiserror::Error)]
        #[serde(tag = "code", content = "detail")]
        $vis enum $name {
            $( #[serde(rename = $code)] #[error($code)] $variant $( { $($field: $fty),* } )? ),+
        }
        impl $crate::method::DomainError for $name {
            const CODES: &'static [&'static str] = &[$($code),+];
        }
    };
}

domain_error! {
    pub enum CommonError {
        Internal = "internal",
        InvalidParams { reason: String } = "invalid_params",
        MethodNotFound { method: String } = "method_not_found",
        HelloRequired = "hello_required",
        Cancelled = "cancelled",
        DaemonRestarting = "daemon_restarting",
        NotFound = "not_found",
        SecretsLocked = "secrets_locked",
    }
}

/// 핸들러 실패: 도메인 오류이거나, 로그로만 남길 내부 오류.
#[derive(Debug)]
pub enum Fail<E> {
    Domain(E),
    Internal(String),
}

impl<E: crate::method::DomainError> From<E> for Fail<E> {
    fn from(e: E) -> Self {
        Fail::Domain(e)
    }
}

/// `io·sqlite` 같은 하부 오류를 내부 오류로 바꾼다: `x.internal("read projects")?`
pub trait Internal<T> {
    fn internal<E>(self, what: &'static str) -> Result<T, Fail<E>>;
}

impl<T, X: std::fmt::Display> Internal<T> for Result<T, X> {
    fn internal<E>(self, what: &'static str) -> Result<T, Fail<E>> {
        self.map_err(|e| Fail::Internal(format!("{what}: {e}")))
    }
}

pub fn to_wire<E: Serialize>(e: &E) -> WireError {
    let v = serde_json::to_value(e).expect("domain errors always serialize");
    WireError {
        code: v.get("code").and_then(|c| c.as_str()).unwrap_or("internal").to_owned(),
        detail: v.get("detail").cloned(),
    }
}
```

- [ ] **Step 5: 구현 — router.rs**

```rust
// crates/protocol/src/router.rs (테스트 위)
use crate::error::{to_wire, CommonError, Fail};
use crate::message::WireError;
use crate::method::{Method, MethodKind};
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Debug, Clone)]
pub struct Ctx {
    pub channel: u32,
    pub cancel: CancellationToken,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchError {
    pub error: WireError,
    pub diagnostic_id: Option<String>,
}

impl DispatchError {
    pub fn common(e: CommonError) -> Self {
        Self { error: to_wire(&e), diagnostic_id: None }
    }
}

type Handler<S> = Arc<dyn Fn(Arc<S>, Ctx, Box<RawValue>) -> BoxFuture<Result<Box<RawValue>, DispatchError>> + Send + Sync>;

pub struct Router<S> {
    state: Arc<S>,
    handlers: HashMap<&'static str, (MethodKind, Handler<S>)>,
}

impl<S: Send + Sync + 'static> Router<S> {
    pub fn new(state: Arc<S>) -> Self {
        Self { state, handlers: HashMap::new() }
    }

    pub fn add<M, F, Fut>(mut self, f: F) -> Self
    where
        M: Method,
        F: Fn(Arc<S>, Ctx, M::Params) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<M::Output, Fail<M::Error>>> + Send + 'static,
    {
        assert!(!self.handlers.contains_key(M::NAME), "duplicate method {}", M::NAME);
        let f = Arc::new(f);
        let handler: Handler<S> = Arc::new(move |state, ctx, raw| {
            let f = f.clone();
            Box::pin(async move {
                let params: M::Params = serde_json::from_str(raw.get())
                    .map_err(|e| DispatchError::common(CommonError::InvalidParams { reason: e.to_string() }))?;
                match f(state, ctx, params).await {
                    Ok(out) => serde_json::value::to_raw_value(&out).map_err(|e| internal(M::NAME, &e.to_string())),
                    Err(Fail::Domain(e)) => Err(DispatchError { error: to_wire(&e), diagnostic_id: None }),
                    Err(Fail::Internal(msg)) => Err(internal(M::NAME, &msg)),
                }
            })
        });
        self.handlers.insert(M::NAME, (M::KIND, handler));
        self
    }

    pub fn kind_of(&self, method: &str) -> Option<MethodKind> {
        self.handlers.get(method).map(|(k, _)| *k)
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.handlers.keys().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn dispatch(&self, method: &str, ctx: Ctx, params: Box<RawValue>) -> BoxFuture<Result<Box<RawValue>, DispatchError>> {
        let Some((_, h)) = self.handlers.get(method) else {
            let e = DispatchError::common(CommonError::MethodNotFound { method: method.to_owned() });
            return Box::pin(async move { Err(e) });
        };
        let cancel = ctx.cancel.clone();
        let fut = h(self.state.clone(), ctx, params);
        Box::pin(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DispatchError::common(CommonError::Cancelled)),
                r = fut => r,
            }
        })
    }
}

fn internal(method: &'static str, msg: &str) -> DispatchError {
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    tracing::error!(diagnostic_id = %id, method, error = %msg, "internal error");
    DispatchError { error: to_wire(&CommonError::Internal), diagnostic_id: Some(id) }
}
```

`tokio::select!`를 쓰므로 `[dependencies]`에 `tokio = { workspace = true }`를 추가한다.

- [ ] **Step 6: 구현 — system.rs와 lib.rs**

```rust
// crates/protocol/src/system.rs
//! 데몬 자체에 대한 메서드. 도메인 crate가 아니라 프로토콜에 둔다.
use crate::method::Exporter;
use crate::{domain_error, method};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PingParams {}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Pong {
    pub daemon_id: String,
    pub version: String,
    pub uptime_ms: i64,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonInfo {
    pub version: String,
    pub instance: String,
    pub pid: u32,
    pub started_ms: i64,
    pub data_dir: String,
}

domain_error! {
    pub enum SystemError {
        Unavailable = "system.unavailable",
    }
}

method!(pub Ping = "system.ping", query, PingParams => Pong, SystemError);
method!(pub Info = "system.info", query, PingParams => DaemonInfo, SystemError);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<Ping>();
    x.method::<Info>();
}
```

```rust
// crates/protocol/src/lib.rs (최종)
//! devbox 데몬과 클라이언트 사이의 프레임·메시지·메서드 계약.
pub mod error;
pub mod frame;
pub mod message;
pub mod method;
pub mod router;
pub mod system;

pub use error::{to_wire, CommonError, Fail, Internal};
pub use method::{Confirm, DomainError, Done, Exporter, Method, MethodKind, NameCollector, Topic};
pub use router::{Ctx, DispatchError, Router};
pub use ts_rs;

pub const PROTOCOL_VERSION: u32 = 1;
```

- [ ] **Step 7: 통과·커밋**

Run: `cargo test -p devbox-protocol` → 모든 테스트 통과

```bash
git add crates/protocol && git commit -m "feat(protocol): declare methods, topics and domain errors with a typed router"
```

---

### Task 5: 창 ↔ 채널 라우팅 (`crates/mux`)

**Files:**
- Create: `crates/mux/Cargo.toml`, `crates/mux/src/lib.rs`

**Interfaces:**
- Consumes:
  - Task 2의 `frame_channel`, `set_frame_channel`, `HEADER_LEN`, `FrameKind::max_body`
  - Task 3의 `control_frame`, `Control`
- Produces:
  - `Mux<K>`: `new()`, `open(key) -> (u32, Vec<Vec<u8>>)`, `close(&key) -> Option<Vec<u8>>`, `outbound(&key, &mut [u8]) -> Result<(), MuxError>`, `inbound(&[u8]) -> Option<&K>`, `reopen_all() -> Vec<Vec<u8>>`
  - `FrameSplitter`: `push(&[u8]) -> Result<Vec<Vec<u8>>, MuxError>`
  - `Backoff`: `next() -> Duration`, `reset()`
  - 03b의 Windows 껍데기 과제와 Task 14(dev-gateway)가 사용한다.

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "devbox-mux"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
devbox-protocol.workspace = true
bytes.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: 실패하는 테스트**

```rust
// crates/mux/src/lib.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use devbox_protocol::frame::{Frame, FrameCodec, FrameKind};
    use devbox_protocol::message::{parse_json, Control};
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    fn raw(kind: FrameKind, channel: u32, body: &[u8]) -> Vec<u8> {
        let mut out = BytesMut::new();
        FrameCodec::default().encode(Frame { kind, channel, body: bytes::Bytes::copy_from_slice(body) }, &mut out).unwrap();
        out.to_vec()
    }
    fn decode(raw: &[u8]) -> Frame {
        FrameCodec::default().decode(&mut BytesMut::from(raw)).unwrap().unwrap()
    }

    #[test]
    fn windows_get_distinct_channels_and_open_frames() {
        let mut m = Mux::new();
        let (a, fa) = m.open("main");
        let (b, _) = m.open("popout");
        assert_ne!(a, b);
        assert_eq!(fa.len(), 1);
        let f = decode(&fa[0]);
        assert_eq!((f.kind, f.channel), (FrameKind::Control, a));
        assert_eq!(parse_json::<Control>(&f).unwrap(), Control::ChannelOpen);
    }

    #[test]
    fn reopening_a_window_closes_its_old_channel_first() {
        let mut m = Mux::new();
        let (a, _) = m.open("main");
        let (a2, frames) = m.open("main");
        assert_ne!(a, a2);
        assert_eq!(frames.len(), 2);
        assert_eq!(parse_json::<Control>(&decode(&frames[0])).unwrap(), Control::ChannelClose);
        assert_eq!(decode(&frames[0]).channel, a);
    }

    #[test]
    fn rewrites_outbound_and_routes_inbound_by_header_only() {
        let mut m = Mux::new();
        let (a, _) = m.open("main");
        let mut out = raw(FrameKind::Json, 0, b"{\"not\":\"parsed\"");
        m.outbound(&"main", &mut out).unwrap();
        assert_eq!(frame_channel(&out), Some(a));
        assert_eq!(m.inbound(&out), Some(&"main"));
        assert!(m.outbound(&"ghost", &mut raw(FrameKind::Json, 0, b"{}")).is_err());
    }

    #[test]
    fn close_returns_a_close_frame_and_forgets_routing() {
        let mut m = Mux::new();
        let (a, _) = m.open("main");
        let f = m.close(&"main").unwrap();
        assert_eq!(decode(&f).channel, a);
        assert_eq!(m.inbound(&raw(FrameKind::Json, a, b"{}")), None);
        assert!(m.close(&"main").is_none());
    }

    #[test]
    fn reopen_all_lists_every_open_channel() {
        let mut m = Mux::new();
        m.open("a");
        m.open("b");
        assert_eq!(m.reopen_all().len(), 2);
    }

    #[test]
    fn splitter_reassembles_frames_across_reads() {
        let mut s = FrameSplitter::default();
        let mut stream = raw(FrameKind::Json, 1, b"{}");
        stream.extend(raw(FrameKind::Stream, 2, &[0, 0, 0, 1, 0, 9, 9]));
        let (left, right) = stream.split_at(7);
        assert!(s.push(left).unwrap().is_empty());
        let frames = s.push(right).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frame_channel(&frames[1]), Some(2));
        assert!(s.push(&[0, 0, 0, 0]).is_err());
    }

    #[test]
    fn backoff_follows_the_design_schedule() {
        let mut b = Backoff::default();
        let ms: Vec<u128> = (0..7).map(|_| b.next().as_millis()).collect();
        assert_eq!(ms, vec![200, 500, 1000, 2000, 5000, 5000, 5000]);
        b.reset();
        assert_eq!(b.next().as_millis(), 200);
    }
}
```

```toml
# crates/mux/Cargo.toml
[dev-dependencies]
tokio-util.workspace = true
```

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-mux` → 컴파일 실패

- [ ] **Step 4: 구현**

```rust
// crates/mux/src/lib.rs (테스트 위)
//! Windows 앱의 창과 데몬 채널을 잇는 라우팅. JSON 본문은 해석하지 않는다.
use devbox_protocol::frame::{FrameCodec, FrameKind, HEADER_LEN};
pub use devbox_protocol::frame::{frame_channel, set_frame_channel};
use devbox_protocol::message::{control_frame, Control};
use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;
use tokio_util::codec::Encoder;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MuxError {
    #[error("window has no open channel")]
    UnknownWindow,
    #[error("frame is shorter than its header")]
    ShortFrame,
    #[error("malformed frame stream: {0}")]
    Malformed(String),
}

pub struct Mux<K> {
    by_key: HashMap<K, u32>,
    by_channel: HashMap<u32, K>,
    next: u32,
}

impl<K: Eq + Hash + Clone> Default for Mux<K> {
    fn default() -> Self {
        Self::new()
    }
}

fn control_bytes(channel: u32, c: &Control) -> Vec<u8> {
    let mut out = bytes::BytesMut::new();
    FrameCodec::default().encode(control_frame(channel, c), &mut out).expect("control frames are small");
    out.to_vec()
}

impl<K: Eq + Hash + Clone> Mux<K> {
    pub fn new() -> Self {
        Self { by_key: HashMap::new(), by_channel: HashMap::new(), next: 1 }
    }

    /// 창에 새 채널을 준다. 이미 열린 창이면(새로 고침) 옛 채널을 먼저 닫는다.
    pub fn open(&mut self, key: K) -> (u32, Vec<Vec<u8>>) {
        let mut frames = Vec::new();
        if let Some(old) = self.close(&key) {
            frames.push(old);
        }
        let ch = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.by_key.insert(key.clone(), ch);
        self.by_channel.insert(ch, key);
        frames.push(control_bytes(ch, &Control::ChannelOpen));
        (ch, frames)
    }

    pub fn close(&mut self, key: &K) -> Option<Vec<u8>> {
        let ch = self.by_key.remove(key)?;
        self.by_channel.remove(&ch);
        Some(control_bytes(ch, &Control::ChannelClose))
    }

    pub fn outbound(&self, key: &K, frame: &mut [u8]) -> Result<(), MuxError> {
        let ch = *self.by_key.get(key).ok_or(MuxError::UnknownWindow)?;
        if set_frame_channel(frame, ch) { Ok(()) } else { Err(MuxError::ShortFrame) }
    }

    pub fn inbound(&self, frame: &[u8]) -> Option<&K> {
        self.by_channel.get(&frame_channel(frame)?)
    }

    /// 데몬 재연결 뒤 열린 채널을 모두 다시 연다.
    pub fn reopen_all(&self) -> Vec<Vec<u8>> {
        let mut chans: Vec<u32> = self.by_channel.keys().copied().collect();
        chans.sort_unstable();
        chans.into_iter().map(|c| control_bytes(c, &Control::ChannelOpen)).collect()
    }
}

/// 바이트 흐름을 완성된 프레임 단위로 자른다(본문은 해석하지 않음).
#[derive(Default)]
pub struct FrameSplitter {
    buf: Vec<u8>,
}

impl FrameSplitter {
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, MuxError> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 5 {
                return Ok(out);
            }
            let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            let kind = FrameKind::from_u8(self.buf[4]).ok_or_else(|| MuxError::Malformed(format!("kind {}", self.buf[4])))?;
            if len < 5 || len - 5 > kind.max_body() {
                return Err(MuxError::Malformed(format!("length {len}")));
            }
            if self.buf.len() < 4 + len {
                return Ok(out);
            }
            let rest = self.buf.split_off(4 + len);
            let frame = std::mem::replace(&mut self.buf, rest);
            debug_assert!(frame.len() >= HEADER_LEN);
            out.push(frame);
        }
    }
}

#[derive(Default)]
pub struct Backoff {
    idx: usize,
}

impl Backoff {
    pub const DELAYS_MS: [u64; 5] = [200, 500, 1000, 2000, 5000];
    pub fn next(&mut self) -> Duration {
        let d = Self::DELAYS_MS[self.idx.min(Self::DELAYS_MS.len() - 1)];
        self.idx += 1;
        Duration::from_millis(d)
    }
    pub fn reset(&mut self) {
        self.idx = 0;
    }
}
```

```toml
# crates/mux/Cargo.toml [dependencies]에 추가
tokio-util.workspace = true
```

- [ ] **Step 5: 통과·커밋**

Run: `cargo test -p devbox-mux` → 7 passed

```bash
git add crates/mux && git commit -m "feat(mux): route frames between windows and daemon channels"
```

---

### Task 6: TS 타입 생성기 (`xtask gen-ts`)

**Files:**
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/ts.rs`
- Modify: `Cargo.toml`(members에 `"xtask"`)

**Interfaces:**
- Consumes:
  - Task 4의 `Exporter`, `Method`, `Topic`, `DomainError`, `CommonError`
  - 도메인 crate의 `export` 함수(Task 10 이후 projects 추가)
- Produces:
  - `cargo run -p xtask -- gen-ts [--check]` → `app/src/rpc/gen/rpc.ts` 한 파일. 내용은 모든 타입 선언, `interface Methods`, `interface Topics`, `METHOD_INFO`, `RPC_ERROR_CODES`, `type RpcErrorCode`.
  - `--check`는 차이가 있으면 종료 코드 1이다.

- [ ] **Step 1: Cargo.toml**

```toml
# xtask/Cargo.toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
devbox-protocol.workspace = true
ts-rs.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: 실패하는 테스트**

```rust
// xtask/src/ts.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_system_methods_types_and_error_codes() {
        let mut x = TsExporter::new();
        devbox_protocol::system::export(&mut x);
        let out = x.render().unwrap();
        assert!(out.starts_with("// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.\n"));
        assert!(out.contains("export type Pong = { daemonId: string, version: string, uptimeMs: number, };"));
        assert!(out.contains("\"system.ping\": { kind: \"query\"; confirm: \"none\"; params: PingParams; result: Pong; error: SystemError };"));
        assert!(out.contains("\"internal\","));
        assert!(out.contains("\"system.unavailable\","));
        assert!(out.contains("export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];"));
    }

    #[test]
    fn output_is_deterministic() {
        let render = || { let mut x = TsExporter::new(); devbox_protocol::system::export(&mut x); x.render().unwrap() };
        assert_eq!(render(), render());
    }
}
```

ts-rs가 실제로 내는 필드 구분자·공백 형식이 이 기대값과 다르면, 기대 문자열을 실제 출력에 맞춘다. 단, `daemonId: string`처럼 camelCase 필드와 `number` 타입이 나오는지는 반드시 확인한다.

- [ ] **Step 3: 실패 확인** — `cargo test -p xtask` → 컴파일 실패

- [ ] **Step 4: 구현**

```rust
// xtask/src/ts.rs (테스트 위)
use devbox_protocol::{CommonError, DomainError, Exporter, Method, MethodKind, Topic, Confirm};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use ts_rs::{Config, TypeVisitor, TS};

pub struct TsExporter {
    cfg: Config,
    seen: HashSet<TypeId>,
    decls: BTreeMap<String, String>,
    methods: BTreeMap<&'static str, String>,
    info: BTreeMap<&'static str, (MethodKind, Confirm)>,
    topics: BTreeMap<&'static str, String>,
    codes: BTreeSet<&'static str>,
    error: Option<String>,
}

fn kind_str(k: MethodKind) -> &'static str {
    match k { MethodKind::Query => "query", MethodKind::Mutation => "mutation", MethodKind::Subscription => "subscription", MethodKind::Stream => "stream" }
}
fn confirm_str(c: Confirm) -> &'static str {
    match c { Confirm::None => "none", Confirm::Always => "always", Confirm::Reversible => "reversible" }
}

impl TsExporter {
    pub fn new() -> Self {
        let mut x = Self {
            cfg: Config::new().with_large_int("number"),
            seen: HashSet::new(),
            decls: BTreeMap::new(),
            methods: BTreeMap::new(),
            info: BTreeMap::new(),
            topics: BTreeMap::new(),
            codes: BTreeSet::new(),
            error: None,
        };
        x.error_type::<CommonError>();
        x
    }

    fn error_type<E: DomainError>(&mut self) {
        self.codes.extend(E::CODES.iter().copied());
        self.visit::<E>();
    }

    fn name<T: TS + 'static + ?Sized>(&self) -> String {
        T::name(&self.cfg)
    }

    pub fn render(&self) -> anyhow::Result<String> {
        if let Some(e) = &self.error {
            anyhow::bail!(e.clone());
        }
        let mut out = String::from("// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.\n/* eslint-disable */\n\n");
        for decl in self.decls.values() {
            out.push_str("export ");
            out.push_str(decl.trim_end());
            out.push('\n');
        }
        out.push_str("\nexport interface Methods {\n");
        for (name, line) in &self.methods {
            out.push_str(&format!("  \"{name}\": {line};\n"));
        }
        out.push_str("}\n\nexport interface Topics {\n");
        for (name, payload) in &self.topics {
            out.push_str(&format!("  \"{name}\": {payload};\n"));
        }
        out.push_str("}\n\nexport const METHOD_INFO = {\n");
        for (name, (k, c)) in &self.info {
            out.push_str(&format!("  \"{name}\": {{ kind: \"{}\", confirm: \"{}\" }},\n", kind_str(*k), confirm_str(*c)));
        }
        out.push_str("} as const;\n\nexport const RPC_ERROR_CODES = [\n");
        for code in &self.codes {
            out.push_str(&format!("  \"{code}\",\n"));
        }
        out.push_str("] as const;\nexport type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];\n");
        Ok(out)
    }
}

impl Exporter for TsExporter {
    fn method<M: Method>(&mut self) {
        self.visit::<M::Params>();
        self.visit::<M::Output>();
        self.error_type::<M::Error>();
        let line = format!(
            "{{ kind: \"{}\"; confirm: \"{}\"; params: {}; result: {}; error: {} }}",
            kind_str(M::KIND), confirm_str(M::CONFIRM), self.name::<M::Params>(), self.name::<M::Output>(), self.name::<M::Error>()
        );
        if self.methods.insert(M::NAME, line).is_some() {
            self.error = Some(format!("method {} declared twice", M::NAME));
        }
        self.info.insert(M::NAME, (M::KIND, M::CONFIRM));
    }

    fn topic<T: Topic>(&mut self) {
        self.visit::<T::Payload>();
        self.topics.insert(T::NAME, self.name::<T::Payload>());
    }
}

impl TypeVisitor for TsExporter {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if self.error.is_some() || !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        if T::output_path().is_some() {
            let ident = T::ident(&self.cfg);
            let decl = T::decl(&self.cfg);
            if let Some(prev) = self.decls.get(&ident) {
                if *prev != decl {
                    self.error = Some(format!("two different types are both named {ident}"));
                    return;
                }
            }
            self.decls.insert(ident, decl);
        }
        T::visit_dependencies(self);
        T::visit_generics(self);
    }
}
```

```rust
// xtask/src/main.rs
mod ts;
use anyhow::{bail, Context, Result};
use devbox_protocol::Exporter;
use std::path::PathBuf;

fn exporter() -> ts::TsExporter {
    let mut x = ts::TsExporter::new();
    devbox_protocol::system::export(&mut x);
    // 도메인 crate가 생기면 여기에 추가한다: devbox_projects::export(&mut x);
    x
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-ts") => gen_ts(args.iter().any(|a| a == "--check")),
        _ => bail!("usage: cargo run -p xtask -- gen-ts [--check]"),
    }
}

fn gen_ts(check: bool) -> Result<()> {
    let out = exporter().render()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let path = root.join("app/src/rpc/gen/rpc.ts");
    if check {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != out {
            bail!("{} 가 선언과 다릅니다. `pnpm gen`을 실행해 커밋하세요.", path.display());
        }
        println!("gen-ts: up to date");
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().context("gen dir")?)?;
    std::fs::write(&path, out)?;
    println!("gen-ts: wrote {}", path.display());
    Ok(())
}
```

`Cargo.toml` members를 `["crates/*", "xtask"]`로 바꾼다.

- [ ] **Step 5: 통과·생성·커밋**

```bash
cargo test -p xtask && cargo run -q -p xtask -- gen-ts && head -20 app/src/rpc/gen/rpc.ts
cargo run -q -p xtask -- gen-ts --check
git add Cargo.toml xtask app/src/rpc/gen/rpc.ts && git commit -m "feat(xtask): generate TypeScript RPC types from method declarations"
```

- [ ] **Step 6: CI에 생성물 검사 추가와 PR B**

`.github/workflows/ci.yml`의 rust job 끝에 `- run: cargo run -q -p xtask -- gen-ts --check`를 추가한다.

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add .github && git commit -m "ci: check generated RPC types"
git push -u origin feat/daemon/protocol && gh pr create --fill --body "프로토콜(프레임·메시지·메서드·오류·라우터), mux, TS 생성기. 01-design §5·§9.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

CI 통과 후 squash 머지, worktree 정리.

---

### Task 7: 인스턴스와 경로 (`crates/core`)

**Files:**
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/paths.rs`

**Interfaces:**
- Produces:
  - `Instance`: `parse(&str)`, `from_env(&dyn Env)`, `as_str()`, `is_test()`
  - `trait Env { var, uid }`, `OsEnv`
  - `Paths { instance, home, data_dir, state_dir, config_dir, instance_config_dir, socket, bin_dir, log_dir, db_path, index_db_path, backups_dir }`, `Paths::resolve(Instance, &dyn Env)`
  - `CoreError`

- [ ] **Step 0: worktree** — `git worktree add ../devbox-wt/feat-daemon-core -b feat/daemon/core origin/main`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "devbox-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
devbox-protocol.workspace = true
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
toml.workspace = true
tracing.workspace = true
tracing-appender.workspace = true
tracing-subscriber.workspace = true

[dev-dependencies]
tempfile.workspace = true

[lints]
workspace = true
```

uid는 `libc::getuid` 대신 `/proc/self`의 소유자를 읽어 얻는다(`unsafe` 없이, 아래 `OsEnv::uid`).

- [ ] **Step 2: 실패하는 테스트**

```rust
// crates/core/src/paths.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeEnv(HashMap<&'static str, &'static str>);
    impl Env for FakeEnv {
        fn var(&self, k: &str) -> Option<String> { self.0.get(k).map(|v| v.to_string()) }
        fn uid(&self) -> u32 { 1000 }
    }
    fn env(pairs: &[(&'static str, &'static str)]) -> FakeEnv { FakeEnv(pairs.iter().copied().collect()) }

    #[test]
    fn instance_names_are_validated() {
        assert!(Instance::parse("prod").is_ok());
        assert!(Instance::parse("test-ab12").unwrap().is_test());
        for bad in ["", "Prod", "1dev", "a/b", "x".repeat(33).as_str()] {
            assert!(Instance::parse(bad).is_err(), "{bad} should be rejected");
        }
        assert_eq!(Instance::from_env(&env(&[])).unwrap().as_str(), "prod");
        assert_eq!(Instance::from_env(&env(&[("DEVBOX_INSTANCE", "dev")])).unwrap().as_str(), "dev");
    }

    #[test]
    fn defaults_follow_xdg_layout() {
        let p = Paths::resolve(Instance::parse("dev").unwrap(), &env(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/home/u/.local/share/devbox/dev"));
        assert_eq!(p.state_dir, PathBuf::from("/home/u/.local/state/devbox/dev"));
        assert_eq!(p.config_dir, PathBuf::from("/home/u/.config/devbox"));
        assert_eq!(p.instance_config_dir, PathBuf::from("/home/u/.config/devbox/dev"));
        assert_eq!(p.socket, PathBuf::from("/run/user/1000/devbox-dev.sock"));
        assert_eq!(p.bin_dir, PathBuf::from("/home/u/.local/share/devbox/bin"));
        assert_eq!(p.db_path, PathBuf::from("/home/u/.local/share/devbox/dev/devbox.db"));
        assert_eq!(p.index_db_path, PathBuf::from("/home/u/.local/share/devbox/dev/index.db"));
    }

    #[test]
    fn xdg_variables_override_defaults() {
        let p = Paths::resolve(
            Instance::parse("prod").unwrap(),
            &env(&[("HOME", "/h"), ("XDG_DATA_HOME", "/d"), ("XDG_STATE_HOME", "/s"), ("XDG_CONFIG_HOME", "/c"), ("XDG_RUNTIME_DIR", "/r")]),
        ).unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/d/devbox/prod"));
        assert_eq!(p.state_dir, PathBuf::from("/s/devbox/prod"));
        assert_eq!(p.config_dir, PathBuf::from("/c/devbox"));
        assert_eq!(p.socket, PathBuf::from("/r/devbox-prod.sock"));
    }

    #[test]
    fn missing_home_is_an_error() {
        assert!(Paths::resolve(Instance::parse("prod").unwrap(), &env(&[])).is_err());
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-core paths`

- [ ] **Step 4: 구현**

```rust
// crates/core/src/lib.rs
//! 데몬 공용 기반: 인스턴스·경로·설정·DB·이벤트 허브·로그.
pub mod paths;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid instance name {0:?}")]
    InvalidInstance(String),
    #[error("HOME is not set")]
    NoHome,
    #[error("{what}: {source}")]
    Io { what: String, #[source] source: std::io::Error },
    #[error("config {path}: {message}")]
    Config { path: String, message: String },
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("database worker stopped")]
    DbClosed,
}

pub fn io_err(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> CoreError {
    let what = what.into();
    move |source| CoreError::Io { what, source }
}
```

```rust
// crates/core/src/paths.rs (테스트 위)
use crate::CoreError;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance(String);

impl Instance {
    pub fn parse(s: &str) -> Result<Self, CoreError> {
        let mut chars = s.chars();
        let ok = s.len() <= 32
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok { Ok(Self(s.to_owned())) } else { Err(CoreError::InvalidInstance(s.to_owned())) }
    }
    pub fn from_env(env: &dyn Env) -> Result<Self, CoreError> {
        Self::parse(env.var("DEVBOX_INSTANCE").as_deref().unwrap_or("prod"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_test(&self) -> bool {
        self.0.starts_with("test-")
    }
}

pub trait Env {
    fn var(&self, key: &str) -> Option<String>;
    fn uid(&self) -> u32;
}

pub struct OsEnv;

impl Env for OsEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }
    fn uid(&self) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub instance: Instance,
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub config_dir: PathBuf,
    pub instance_config_dir: PathBuf,
    pub socket: PathBuf,
    pub bin_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
    pub index_db_path: PathBuf,
    pub backups_dir: PathBuf,
}

impl Paths {
    pub fn resolve(instance: Instance, env: &dyn Env) -> Result<Self, CoreError> {
        let home = PathBuf::from(env.var("HOME").ok_or(CoreError::NoHome)?);
        let xdg = |key: &str, rel: &str| env.var(key).map(PathBuf::from).unwrap_or_else(|| home.join(rel));
        let data_root = xdg("XDG_DATA_HOME", ".local/share").join("devbox");
        let data_dir = data_root.join(instance.as_str());
        let state_dir = xdg("XDG_STATE_HOME", ".local/state").join("devbox").join(instance.as_str());
        let config_dir = xdg("XDG_CONFIG_HOME", ".config").join("devbox");
        let runtime = env.var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", env.uid())));
        Ok(Self {
            socket: runtime.join(format!("devbox-{}.sock", instance.as_str())),
            instance_config_dir: config_dir.join(instance.as_str()),
            bin_dir: data_root.join("bin"),
            log_dir: state_dir.join("logs"),
            db_path: data_dir.join("devbox.db"),
            index_db_path: data_dir.join("index.db"),
            backups_dir: data_dir.join("backups"),
            instance,
            home,
            data_dir,
            state_dir,
            config_dir,
        })
    }

    pub fn ensure_dirs(&self) -> Result<(), CoreError> {
        for d in [&self.data_dir, &self.state_dir, &self.log_dir, &self.backups_dir, &self.instance_config_dir] {
            std::fs::create_dir_all(d).map_err(crate::io_err(format!("create {}", d.display())))?;
        }
        Ok(())
    }
}
```

`lib.rs`에 `pub use paths::{Env, Instance, OsEnv, Paths};`를 추가한다.

- [ ] **Step 5: 통과·커밋** — `cargo test -p devbox-core paths` → 4 passed

```bash
git add Cargo.toml crates/core && git commit -m "feat(core): resolve instance-scoped XDG paths"
```

---

### Task 8: 설정 (공유 `config.toml` + PC별 `config.local.toml`)

**Files:**
- Create: `crates/core/src/config.rs`

**Interfaces:**
- Consumes: Task 7의 `Paths`
- Produces:
  - `Config { projects: ProjectsConfig { roots: Vec<String> }, pc: PcConfig { name: Option<String>, windows_root: Option<String> } }`
  - `Config::load(&Paths) -> Result<Config, CoreError>`
  - `expand_home(&str, &Path) -> PathBuf`
  - 이후 하위 프로젝트가 절을 추가한다(`deny_unknown_fields` 유지).

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/core/src/config.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    fn paths_in(dir: &std::path::Path) -> Paths {
        struct E(String);
        impl crate::Env for E {
            fn var(&self, k: &str) -> Option<String> { (k == "HOME").then(|| self.0.clone()) }
            fn uid(&self) -> u32 { 1000 }
        }
        Paths::resolve(crate::Instance::parse("test-c").unwrap(), &E(dir.display().to_string())).unwrap()
    }

    #[test]
    fn defaults_when_files_are_missing() {
        let t = tempfile::tempdir().unwrap();
        let c = Config::load(&paths_in(t.path())).unwrap();
        assert_eq!(c.projects.roots, vec!["~/projects".to_string()]);
        assert!(c.pc.name.is_none());
    }

    #[test]
    fn local_file_overrides_shared_file_key_by_key() {
        let t = tempfile::tempdir().unwrap();
        let p = paths_in(t.path());
        std::fs::create_dir_all(&p.config_dir).unwrap();
        std::fs::write(p.config_dir.join("config.toml"), "[projects]\nroots = [\"~/a\", \"~/b\"]\n[pc]\nname = \"shared\"\n").unwrap();
        std::fs::write(p.config_dir.join("config.local.toml"), "[pc]\nname = \"desk\"\nwindows_root = 'C:\\dev'\n").unwrap();
        let c = Config::load(&p).unwrap();
        assert_eq!(c.projects.roots, vec!["~/a", "~/b"]);
        assert_eq!(c.pc.name.as_deref(), Some("desk"));
        assert_eq!(c.pc.windows_root.as_deref(), Some("C:\\dev"));
    }

    #[test]
    fn unknown_keys_are_reported_with_the_file() {
        let t = tempfile::tempdir().unwrap();
        let p = paths_in(t.path());
        std::fs::create_dir_all(&p.config_dir).unwrap();
        std::fs::write(p.config_dir.join("config.toml"), "[projects]\nroot = []\n").unwrap();
        let e = Config::load(&p).unwrap_err().to_string();
        assert!(e.contains("config.toml") || e.contains("config"), "{e}");
    }

    #[test]
    fn tilde_expands_to_home() {
        let home = std::path::Path::new("/home/u");
        assert_eq!(expand_home("~/projects", home), std::path::PathBuf::from("/home/u/projects"));
        assert_eq!(expand_home("/abs", home), std::path::PathBuf::from("/abs"));
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-core config`

- [ ] **Step 3: 구현**

```rust
// crates/core/src/config.rs (테스트 위)
use crate::{CoreError, Paths};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub projects: ProjectsConfig,
    pub pc: PcConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectsConfig {
    pub roots: Vec<String>,
}

impl Default for ProjectsConfig {
    fn default() -> Self {
        Self { roots: vec!["~/projects".into()] }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PcConfig {
    pub name: Option<String>,
    pub windows_root: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { projects: ProjectsConfig::default(), pc: PcConfig::default() }
    }
}

fn read_table(path: &Path) -> Result<toml::Table, CoreError> {
    match std::fs::read_to_string(path) {
        Ok(text) => text.parse::<toml::Table>().map_err(|e| CoreError::Config { path: path.display().to_string(), message: e.to_string() }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(CoreError::Io { what: format!("read {}", path.display()), source: e }),
    }
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self, CoreError> {
        let shared = paths.config_dir.join("config.toml");
        let local = paths.config_dir.join("config.local.toml");
        let mut table = read_table(&shared)?;
        merge(&mut table, read_table(&local)?);
        toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| CoreError::Config {
            path: format!("{} + {}", shared.display(), local.display()),
            message: e.to_string(),
        })
    }
}

pub fn expand_home(s: &str, home: &Path) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if s == "~" => home.to_path_buf(),
        None => PathBuf::from(s),
    }
}
```

`lib.rs`에 `pub mod config; pub use config::{expand_home, Config};`를 추가한다.

- [ ] **Step 4: 통과·커밋** — `cargo test -p devbox-core config` → 4 passed

```bash
git add crates/core && git commit -m "feat(core): load shared and per-PC configuration"
```

---

### Task 9: 이벤트 허브

**Files:**
- Create: `crates/core/src/hub.rs`

**Interfaces:**
- Consumes: `devbox_protocol::Topic`
- Produces:
  - `Hub::new()`
  - `Hub::subscribe(&self, topic: &str) -> HubReceiver`
  - `Hub::publish<T: Topic>(&self, rev: u64, payload: &T::Payload)`
  - `HubReceiver::recv() -> Option<HubMessage>`
  - `HubMessage { Event { rev: u64, payload: Arc<str> }, Resync }`
  - 구독 큐 상한 `QUEUE = 256`. 넘치면 발행은 기다리지 않고 그 구독에 Resync 하나를 예약한다.

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/core/src/hub.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use devbox_protocol::topic;
    use serde::Serialize;

    #[derive(Serialize, devbox_protocol::ts_rs::TS)]
    struct Ping { n: u32 }
    topic!(PingTopic = "test.ping", Ping);

    #[tokio::test]
    async fn delivers_events_in_order_to_matching_topics_only() {
        let hub = Hub::new();
        let mut a = hub.subscribe("test.ping");
        let mut other = hub.subscribe("test.other");
        hub.publish::<PingTopic>(1, &Ping { n: 1 });
        hub.publish::<PingTopic>(2, &Ping { n: 2 });
        assert_eq!(a.recv().await, Some(HubMessage::Event { rev: 1, payload: "{\"n\":1}".into() }));
        assert_eq!(a.recv().await, Some(HubMessage::Event { rev: 2, payload: "{\"n\":2}".into() }));
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), other.recv()).await.is_err());
    }

    #[tokio::test]
    async fn slow_subscriber_gets_one_resync_and_publisher_never_waits() {
        let hub = Hub::new();
        let mut slow = hub.subscribe("test.ping");
        let started = std::time::Instant::now();
        for n in 0..(QUEUE as u32 + 50) { hub.publish::<PingTopic>(n as u64, &Ping { n }); }
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
        assert_eq!(slow.recv().await, Some(HubMessage::Resync));
        // Resync 뒤에는 큐에 남아 있던 옛 이벤트를 버리고 새 이벤트만 받는다.
        hub.publish::<PingTopic>(999, &Ping { n: 999 });
        assert_eq!(slow.recv().await, Some(HubMessage::Event { rev: 999, payload: "{\"n\":999}".into() }));
    }

    #[tokio::test]
    async fn dropped_receivers_are_removed() {
        let hub = Hub::new();
        drop(hub.subscribe("test.ping"));
        hub.publish::<PingTopic>(1, &Ping { n: 1 });
        assert_eq!(hub.subscriber_count("test.ping"), 0);
    }
}
```

`Cargo.toml` `[dev-dependencies]`에 `tokio = { workspace = true, features = ["macros", "rt-multi-thread", "time"] }`를 추가한다.

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-core hub`

- [ ] **Step 3: 구현**

```rust
// crates/core/src/hub.rs (테스트 위)
use devbox_protocol::Topic;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Notify};

pub const QUEUE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubMessage {
    Event { rev: u64, payload: Arc<str> },
    Resync,
}

struct Sub {
    tx: mpsc::Sender<(u64, Arc<str>)>,
    lagged: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

#[derive(Default)]
pub struct Hub {
    subs: Mutex<HashMap<String, Vec<Sub>>>,
}

pub struct HubReceiver {
    rx: mpsc::Receiver<(u64, Arc<str>)>,
    lagged: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, topic: &str) -> HubReceiver {
        let (tx, rx) = mpsc::channel(QUEUE);
        let lagged = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        self.subs.lock().expect("hub lock").entry(topic.to_owned()).or_default().push(Sub {
            tx,
            lagged: lagged.clone(),
            notify: notify.clone(),
        });
        HubReceiver { rx, lagged, notify }
    }

    pub fn publish<T: Topic>(&self, rev: u64, payload: &T::Payload) {
        let text: Arc<str> = serde_json::to_string(payload).expect("topic payloads serialize").into();
        let mut subs = self.subs.lock().expect("hub lock");
        if let Some(list) = subs.get_mut(T::NAME) {
            list.retain(|s| match s.tx.try_send((rev, text.clone())) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    s.lagged.store(true, Ordering::Release);
                    s.notify.notify_one();
                    true
                }
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            });
        }
    }

    pub fn subscriber_count(&self, topic: &str) -> usize {
        let mut subs = self.subs.lock().expect("hub lock");
        subs.get_mut(topic).map(|l| {
            l.retain(|s| !s.tx.is_closed());
            l.len()
        }).unwrap_or(0)
    }
}

impl HubReceiver {
    pub async fn recv(&mut self) -> Option<HubMessage> {
        loop {
            if self.lagged.swap(false, Ordering::AcqRel) {
                while self.rx.try_recv().is_ok() {}
                return Some(HubMessage::Resync);
            }
            tokio::select! {
                m = self.rx.recv() => return m.map(|(rev, payload)| HubMessage::Event { rev, payload }),
                _ = self.notify.notified() => continue,
            }
        }
    }
}
```

`lib.rs`에 `pub mod hub; pub use hub::{Hub, HubMessage, HubReceiver};`를 추가한다.

- [ ] **Step 4: 통과·커밋** — `cargo test -p devbox-core hub` → 3 passed

```bash
git add crates/core && git commit -m "feat(core): add a non-blocking topic hub with resync on lag"
```

---

### Task 10: SQLite (쓰기 전용 스레드·도메인별 마이그레이션·백업·rev)

**Files:**
- Create: `crates/core/src/db.rs`

**Interfaces:**
- Consumes: Task 7의 `CoreError`
- Produces:
  - `Migration { domain: &'static str, version: u32, sql: &'static str }`
  - `Db::open(path, backups_dir, app_version, &[Migration]) -> Result<Db, CoreError>`
  - `Db::write(f: FnOnce(&Transaction) -> rusqlite::Result<T>) -> Result<(T, u64), CoreError>`(async, 커밋 rev 반환)
  - `Db::read(f: FnOnce(&Transaction) -> rusqlite::Result<T>) -> Result<T, CoreError>`(async)
  - `Db::rev() -> u64`, `fn current_rev(&Transaction) -> rusqlite::Result<u64>`

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/core/src/db.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    const V1: Migration = Migration { domain: "t", version: 1, sql: "CREATE TABLE t_items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);" };
    const V2: Migration = Migration { domain: "t", version: 2, sql: "ALTER TABLE t_items ADD COLUMN extra TEXT;" };

    #[tokio::test]
    async fn applies_migrations_and_counts_commits() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("devbox.db"), &d.path().join("backups"), "1.0.0-dev", &[V1]).unwrap();
        let (_, r1) = db.write(|tx| tx.execute("INSERT INTO t_items(name) VALUES ('a')", [])).await.unwrap();
        let (_, r2) = db.write(|tx| tx.execute("INSERT INTO t_items(name) VALUES ('b')", [])).await.unwrap();
        assert_eq!(r2, r1 + 1);
        assert_eq!(db.rev(), r2);
        let (n, rev) = db.read(|tx| Ok((tx.query_row("SELECT count(*) FROM t_items", [], |r| r.get::<_, i64>(0))?, current_rev(tx)?))).await.unwrap();
        assert_eq!((n, rev), (2, r2));
    }

    #[tokio::test]
    async fn reopening_keeps_rev_and_backs_up_once_before_new_migrations() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("devbox.db");
        let backups = d.path().join("backups");
        {
            let db = Db::open(&path, &backups, "1.0.0-dev", &[V1]).unwrap();
            db.write(|tx| tx.execute("INSERT INTO t_items(name) VALUES ('a')", [])).await.unwrap();
        }
        assert!(!backups.join("devbox-pre-1.0.1.db").exists());
        let db = Db::open(&path, &backups, "1.0.1", &[V1, V2]).unwrap();
        assert!(backups.join("devbox-pre-1.0.1.db").exists());
        assert_eq!(db.rev(), 1);
        db.write(|tx| tx.execute("UPDATE t_items SET extra = 'x'", [])).await.unwrap();
        let again = Db::open(&path, &backups, "1.0.2", &[V1, V2]);
        assert!(again.is_ok());
        assert!(!backups.join("devbox-pre-1.0.2.db").exists(), "no pending migrations, no backup");
    }

    #[tokio::test]
    async fn failed_write_rolls_back_and_does_not_bump_rev() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("devbox.db"), &d.path().join("b"), "1.0.0-dev", &[V1]).unwrap();
        let before = db.rev();
        let r = db.write(|tx| { tx.execute("INSERT INTO t_items(name) VALUES ('a')", [])?; tx.execute("INSERT INTO nope VALUES (1)", []) }).await;
        assert!(r.is_err());
        assert_eq!(db.rev(), before);
        let n: i64 = db.read(|tx| tx.query_row("SELECT count(*) FROM t_items", [], |r| r.get(0))).await.unwrap();
        assert_eq!(n, 0);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-core db`

- [ ] **Step 3: 구현**

```rust
// crates/core/src/db.rs (테스트 위)
use crate::CoreError;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub domain: &'static str,
    pub version: u32,
    pub sql: &'static str,
}

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

#[derive(Clone)]
pub struct Db {
    writer: std_mpsc::Sender<Job>,
    readers: Arc<Mutex<Vec<Connection>>>,
    path: PathBuf,
    rev: Arc<AtomicU64>,
}

const BASE: &str = "
CREATE TABLE IF NOT EXISTS schema_versions (domain TEXT PRIMARY KEY, version INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS core_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
INSERT OR IGNORE INTO core_meta(key, value) VALUES ('rev', 0);
";

fn connect(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
    Ok(c)
}

pub fn current_rev(tx: &Transaction) -> rusqlite::Result<u64> {
    tx.query_row("SELECT value FROM core_meta WHERE key = 'rev'", [], |r| r.get::<_, i64>(0)).map(|v| v as u64)
}

fn pending<'a>(conn: &Connection, migrations: &'a [Migration]) -> rusqlite::Result<Vec<&'a Migration>> {
    let mut out = Vec::new();
    for m in migrations {
        let have: Option<u32> = conn
            .query_row("SELECT version FROM schema_versions WHERE domain = ?1", [m.domain], |r| r.get(0))
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
        if have.is_none_or(|v| v < m.version) {
            out.push(m);
        }
    }
    Ok(out)
}

impl Db {
    pub fn open(path: &Path, backups_dir: &Path, app_version: &str, migrations: &[Migration]) -> Result<Self, CoreError> {
        let existed = path.exists();
        let mut conn = connect(path)?;
        conn.execute_batch(BASE)?;
        let todo = pending(&conn, migrations)?;
        if existed && !todo.is_empty() {
            std::fs::create_dir_all(backups_dir).map_err(crate::io_err("create backups dir"))?;
            for entry in std::fs::read_dir(backups_dir).map_err(crate::io_err("list backups"))? {
                let p = entry.map_err(crate::io_err("list backups"))?.path();
                if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("devbox-pre-")) {
                    std::fs::remove_file(&p).map_err(crate::io_err("remove old backup"))?;
                }
            }
            let target = backups_dir.join(format!("devbox-pre-{app_version}.db"));
            conn.execute("VACUUM INTO ?1", [target.display().to_string()])?;
        }
        for m in todo {
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.execute(
                "INSERT INTO schema_versions(domain, version) VALUES (?1, ?2) ON CONFLICT(domain) DO UPDATE SET version = excluded.version",
                rusqlite::params![m.domain, m.version],
            )?;
            tx.commit()?;
        }
        let rev: i64 = conn.query_row("SELECT value FROM core_meta WHERE key = 'rev'", [], |r| r.get(0))?;
        let (tx, rx) = std_mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("devbox-db-writer".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })
            .map_err(crate::io_err("spawn db writer"))?;
        Ok(Self { writer: tx, readers: Arc::new(Mutex::new(Vec::new())), path: path.to_owned(), rev: Arc::new(AtomicU64::new(rev as u64)) })
    }

    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Acquire)
    }

    pub async fn write<T, F>(&self, f: F) -> Result<(T, u64), CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction) -> rusqlite::Result<T> + Send + 'static,
    {
        let (reply, wait) = oneshot::channel();
        let rev = self.rev.clone();
        let job: Job = Box::new(move |conn| {
            let result = (|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let out = f(&tx)?;
                tx.execute("UPDATE core_meta SET value = value + 1 WHERE key = 'rev'", [])?;
                let new_rev = current_rev(&tx)?;
                tx.commit()?;
                rev.store(new_rev, Ordering::Release);
                Ok((out, new_rev))
            })();
            let _ = reply.send(result.map_err(CoreError::Db));
        });
        self.writer.send(job).map_err(|_| CoreError::DbClosed)?;
        wait.await.map_err(|_| CoreError::DbClosed)?
    }

    pub async fn read<T, F>(&self, f: F) -> Result<T, CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction) -> rusqlite::Result<T> + Send + 'static,
    {
        let readers = self.readers.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = readers.lock().expect("reader pool").pop();
            let conn = match conn { Some(c) => c, None => connect(&path)? };
            let out = {
                let tx = conn.unchecked_transaction()?;
                let out = f(&tx)?;
                tx.finish()?;
                out
            };
            readers.lock().expect("reader pool").push(conn);
            Ok(out)
        })
        .await
        .map_err(|_| CoreError::DbClosed)?
    }
}
```

`lib.rs`에 `pub mod db; pub use db::{current_rev, Db, Migration};`를 추가한다.

- [ ] **Step 4: 통과·커밋** — `cargo test -p devbox-core db` → 3 passed

```bash
git add crates/core && git commit -m "feat(core): add SQLite with a single writer, per-domain migrations and commit revs"
```

---

### Task 11: 로그와 panic 기록

**Files:**
- Create: `crates/core/src/log.rs`, `crates/core/tests/panic_hook.rs`

**Interfaces:**
- Produces:
  - `init_logging(&Paths, stderr: bool) -> Result<LogGuard, CoreError>`(일 단위 회전 7일, `RUST_LOG` 필터, 최근 이벤트 50개 링)
  - `install_panic_hook(&Paths, version: &'static str)`(`crash-<unix_ms>.log`에 버전·스레드·위치·메시지·최근 이벤트를 씀)

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/core/tests/panic_hook.rs
use devbox_core::{log, Instance, Paths};

#[test]
fn panic_hook_writes_a_crash_file_with_location_and_recent_events() {
    let t = tempfile::tempdir().unwrap();
    struct E(String);
    impl devbox_core::Env for E {
        fn var(&self, k: &str) -> Option<String> { (k == "HOME").then(|| self.0.clone()) }
        fn uid(&self) -> u32 { 1000 }
    }
    let paths = Paths::resolve(Instance::parse("test-p").unwrap(), &E(t.path().display().to_string())).unwrap();
    paths.ensure_dirs().unwrap();
    let _guard = log::init_logging(&paths, false).unwrap();
    log::install_panic_hook(&paths, "1.0.0-test");
    tracing::info!(method = "projects.add", "handled request");
    let r = std::thread::Builder::new().name("worker".into()).spawn(|| panic!("boom here")).unwrap().join();
    assert!(r.is_err());
    let crash = std::fs::read_dir(&paths.log_dir).unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_str().unwrap().starts_with("crash-"))
        .expect("crash file");
    let text = std::fs::read_to_string(crash).unwrap();
    assert!(text.contains("version: 1.0.0-test"));
    assert!(text.contains("thread: worker"));
    assert!(text.contains("panic_hook.rs"));
    assert!(text.contains("boom here"));
    assert!(text.contains("handled request"));
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-core --test panic_hook`

- [ ] **Step 3: 구현**

```rust
// crates/core/src/log.rs
use crate::{CoreError, Paths};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const RECENT: usize = 50;
static RING: OnceLock<Arc<Mutex<VecDeque<String>>>> = OnceLock::new();

fn ring() -> Arc<Mutex<VecDeque<String>>> {
    RING.get_or_init(|| Arc::new(Mutex::new(VecDeque::with_capacity(RECENT)))).clone()
}

struct RecentLayer;

struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={:?}", field.name(), value);
    }
}

impl<S: tracing::Subscriber> Layer<S> for RecentLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut f = Fields(format!("{} {}", event.metadata().level(), event.metadata().target()));
        event.record(&mut f);
        let r = ring();
        let mut q = r.lock().expect("ring");
        if q.len() == RECENT {
            q.pop_front();
        }
        q.push_back(f.0);
    }
}

pub struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

pub fn init_logging(paths: &Paths, stderr: bool) -> Result<LogGuard, CoreError> {
    std::fs::create_dir_all(&paths.log_dir).map_err(crate::io_err("create log dir"))?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("daemon")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&paths.log_dir)
        .map_err(|e| CoreError::Io { what: "log appender".into(), source: std::io::Error::other(e) })?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer);
    let console = stderr.then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    let _ = tracing_subscriber::registry().with(filter).with(RecentLayer).with(file).with(console).try_init();
    Ok(LogGuard(guard))
}

pub fn install_panic_hook(paths: &Paths, version: &'static str) {
    let dir = paths.log_dir.clone();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let mut text = format!(
            "version: {version}\nthread: {}\nlocation: {}\nmessage: {}\nrecent:\n",
            thread.name().unwrap_or("unnamed"),
            info.location().map(|l| l.to_string()).unwrap_or_default(),
            info.payload().downcast_ref::<&str>().copied().or_else(|| info.payload().downcast_ref::<String>().map(String::as_str)).unwrap_or("?"),
        );
        if let Ok(q) = ring().lock() {
            for line in q.iter() {
                text.push_str("  ");
                text.push_str(line);
                text.push('\n');
            }
        }
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let _ = std::fs::write(dir.join(format!("crash-{ms}.log")), text);
        eprintln!("devbox panicked; see {}", dir.display());
    }));
}
```

`lib.rs`에 `pub mod log;`를 추가한다.

- [ ] **Step 4: 통과·커밋** — `cargo test -p devbox-core --test panic_hook` → 1 passed

```bash
git add crates/core && git commit -m "feat(core): add rolling logs and a panic hook with recent events"
```

---

### Task 12: 첫 도메인 `projects`

**Files:**
- Create: `crates/projects/Cargo.toml`, `crates/projects/src/lib.rs`, `crates/projects/src/model.rs`, `crates/projects/src/store.rs`
- Modify: `xtask/src/main.rs`(`devbox_projects::export` 추가), `xtask/Cargo.toml`

**Interfaces:**
- Consumes: `devbox_core::{Db, Migration, Hub, Config, expand_home}`, `devbox_protocol::{method!, topic!, domain_error!, Fail, Internal, Done, Exporter}`
- Produces:
  - 타입: `Project { id, name, path, favorite, addedMs }`, `ProjectList { rev, items }`, `ListProjects {}`, `AddProject { path }`, `RemoveProject { id }`, `ScanProjects {}`, `ScanResult { candidates: Vec<Candidate> }`, `Candidate { path, name, isGit, registered }`, `ProjectsChanged { rev }`
  - 메서드: `projects.list`(query), `projects.add`(mutation), `projects.remove`(mutation, always), `projects.scan`(query)
  - 주제: `projects.changed`
  - `ProjectsError { PathInvalid{path}, NotDirectory{path}, AlreadyExists{path}, NotFound{id} }`
  - `ProjectsService::new(Db, Arc<Hub>, home, roots)`, `MIGRATIONS`, `export`
  - S1이 프로젝트 전환기·시작 구성을 붙인다.

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "devbox-projects"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
devbox-core.workspace = true
devbox-protocol.workspace = true
serde.workspace = true
thiserror.workspace = true
ts-rs.workspace = true
uuid.workspace = true
rusqlite.workspace = true

[dev-dependencies]
tempfile.workspace = true
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }

[lints]
workspace = true
```

- [ ] **Step 2: 실패하는 테스트**

```rust
// crates/projects/src/lib.rs 끝
#[cfg(test)]
mod tests {
    use super::*;
    use devbox_core::{Db, Hub};
    use std::sync::Arc;

    async fn service(dir: &std::path::Path) -> (ProjectsService, Arc<Hub>) {
        let db = Db::open(&dir.join("devbox.db"), &dir.join("b"), "1.0.0-dev", MIGRATIONS).unwrap();
        let hub = Arc::new(Hub::new());
        (ProjectsService::new(db, hub.clone(), dir.to_path_buf(), vec!["~/projects".into()]), hub)
    }

    #[tokio::test]
    async fn add_list_remove_and_publish() {
        let t = tempfile::tempdir().unwrap();
        let proj = t.path().join("projects/alpha");
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        let (s, hub) = service(t.path()).await;
        let mut events = hub.subscribe("projects.changed");

        let p = s.add(AddProject { path: proj.display().to_string() }).await.unwrap();
        assert_eq!(p.name, "alpha");
        assert!(matches!(events.recv().await, Some(devbox_core::HubMessage::Event { .. })));

        let list = s.list(ListProjects {}).await.unwrap();
        assert_eq!(list.items.len(), 1);
        assert!(list.rev.parse::<u64>().unwrap() >= 1);

        let dup = s.add(AddProject { path: proj.display().to_string() }).await.unwrap_err();
        assert!(matches!(dup, Fail::Domain(ProjectsError::AlreadyExists { .. })));

        s.remove(RemoveProject { id: p.id.clone() }).await.unwrap();
        let gone = s.remove(RemoveProject { id: p.id }).await.unwrap_err();
        assert!(matches!(gone, Fail::Domain(ProjectsError::NotFound { .. })));
    }

    #[tokio::test]
    async fn rejects_relative_missing_and_file_paths() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        let (s, _) = service(t.path()).await;
        for (path, want) in [("rel/path", "projects.path_invalid"), ("/no/such/dir", "projects.path_invalid"), (file.to_str().unwrap(), "projects.not_directory")] {
            let e = s.add(AddProject { path: path.into() }).await.unwrap_err();
            let Fail::Domain(e) = e else { panic!("domain error expected") };
            assert_eq!(devbox_protocol::to_wire(&e).code, want);
        }
    }

    #[tokio::test]
    async fn scan_lists_root_children_and_marks_registered_and_git() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("projects");
        std::fs::create_dir_all(root.join("a/.git")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::create_dir_all(root.join("a-agents/x")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        let (s, _) = service(t.path()).await;
        s.add(AddProject { path: root.join("a").display().to_string() }).await.unwrap();
        let r = s.scan(ScanProjects {}).await.unwrap();
        let names: Vec<_> = r.candidates.iter().map(|c| (c.name.as_str(), c.is_git, c.registered)).collect();
        assert_eq!(names, vec![("a", true, true), ("b", false, false)]);
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-projects`

- [ ] **Step 4: 구현**

```rust
// crates/projects/src/model.rs
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub favorite: bool,
    pub added_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ProjectList {
    pub rev: String,
    pub items: Vec<Project>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ListProjects {}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct AddProject {
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct RemoveProject {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ScanProjects {}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    pub is_git: bool,
    pub registered: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ScanResult {
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ProjectsChanged {
    pub rev: String,
}

domain_error! {
    pub enum ProjectsError {
        PathInvalid { path: String } = "projects.path_invalid",
        NotDirectory { path: String } = "projects.not_directory",
        AlreadyExists { path: String } = "projects.already_exists",
        NotFound { id: String } = "projects.not_found",
    }
}
```

```rust
// crates/projects/src/store.rs
use crate::model::Project;
use rusqlite::{params, Transaction};

pub const MIGRATION_1: &str = "
CREATE TABLE projects_projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  favorite INTEGER NOT NULL DEFAULT 0,
  added_ms INTEGER NOT NULL
);";

pub fn all(tx: &Transaction) -> rusqlite::Result<Vec<Project>> {
    let mut stmt = tx.prepare("SELECT id, name, path, favorite, added_ms FROM projects_projects ORDER BY favorite DESC, name")?;
    let rows = stmt.query_map([], |r| {
        Ok(Project { id: r.get(0)?, name: r.get(1)?, path: r.get(2)?, favorite: r.get::<_, i64>(3)? != 0, added_ms: r.get(4)? })
    })?;
    rows.collect()
}

pub fn insert(tx: &Transaction, p: &Project) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO projects_projects(id, name, path, favorite, added_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![p.id, p.name, p.path, p.favorite as i64, p.added_ms],
    )?;
    Ok(())
}

pub fn exists_path(tx: &Transaction, path: &str) -> rusqlite::Result<bool> {
    tx.query_row("SELECT EXISTS(SELECT 1 FROM projects_projects WHERE path = ?1)", [path], |r| r.get(0))
}

pub fn delete(tx: &Transaction, id: &str) -> rusqlite::Result<usize> {
    tx.execute("DELETE FROM projects_projects WHERE id = ?1", [id])
}
```

```rust
// crates/projects/src/lib.rs (테스트 위)
//! 프로젝트 목록 도메인.
pub mod model;
mod store;

pub use model::*;
use devbox_core::{current_rev, expand_home, Db, Hub, Migration};
use devbox_protocol::{method, topic, Done, Exporter, Fail, Internal};
use std::path::PathBuf;
use std::sync::Arc;

pub const MIGRATIONS: &[Migration] = &[Migration { domain: "projects", version: 1, sql: store::MIGRATION_1 }];

method!(pub ProjectsList = "projects.list", query, ListProjects => ProjectList, ProjectsError);
method!(pub ProjectsAdd = "projects.add", mutation, AddProject => Project, ProjectsError);
method!(pub ProjectsRemove = "projects.remove", mutation(always), RemoveProject => Done, ProjectsError);
method!(pub ProjectsScan = "projects.scan", query, ScanProjects => ScanResult, ProjectsError);
topic!(pub ProjectsChangedTopic = "projects.changed", ProjectsChanged);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ProjectsList>();
    x.method::<ProjectsAdd>();
    x.method::<ProjectsRemove>();
    x.method::<ProjectsScan>();
    x.topic::<ProjectsChangedTopic>();
}

type R<T> = Result<T, Fail<ProjectsError>>;

pub struct ProjectsService {
    db: Db,
    hub: Arc<Hub>,
    home: PathBuf,
    roots: Vec<String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl ProjectsService {
    pub fn new(db: Db, hub: Arc<Hub>, home: PathBuf, roots: Vec<String>) -> Self {
        Self { db, hub, home, roots }
    }

    fn changed(&self, rev: u64) {
        self.hub.publish::<ProjectsChangedTopic>(rev, &ProjectsChanged { rev: rev.to_string() });
    }

    pub async fn list(&self, _: ListProjects) -> R<ProjectList> {
        let (items, rev) = self.db.read(|tx| Ok((store::all(tx)?, current_rev(tx)?))).await.internal("list projects")?;
        Ok(ProjectList { rev: rev.to_string(), items })
    }

    pub async fn add(&self, p: AddProject) -> R<Project> {
        let raw = PathBuf::from(&p.path);
        if !raw.is_absolute() {
            return Err(ProjectsError::PathInvalid { path: p.path }.into());
        }
        let real = std::fs::canonicalize(&raw).map_err(|_| ProjectsError::PathInvalid { path: p.path.clone() })?;
        if !real.is_dir() {
            return Err(ProjectsError::NotDirectory { path: p.path }.into());
        }
        let path = real.display().to_string();
        let project = Project {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone()),
            path: path.clone(),
            favorite: false,
            added_ms: now_ms(),
        };
        let row = project.clone();
        let (inserted, rev) = self
            .db
            .write(move |tx| {
                if store::exists_path(tx, &row.path)? {
                    return Ok(false);
                }
                store::insert(tx, &row)?;
                Ok(true)
            })
            .await
            .internal("add project")?;
        if !inserted {
            return Err(ProjectsError::AlreadyExists { path }.into());
        }
        self.changed(rev);
        Ok(project)
    }

    pub async fn remove(&self, p: RemoveProject) -> R<Done> {
        let id = p.id.clone();
        let (n, rev) = self.db.write(move |tx| store::delete(tx, &id)).await.internal("remove project")?;
        if n == 0 {
            return Err(ProjectsError::NotFound { id: p.id }.into());
        }
        self.changed(rev);
        Ok(Done { rev: rev.to_string() })
    }

    pub async fn scan(&self, _: ScanProjects) -> R<ScanResult> {
        let registered: std::collections::HashSet<String> =
            self.db.read(|tx| store::all(tx)).await.internal("scan projects")?.into_iter().map(|p| p.path).collect();
        let mut candidates = Vec::new();
        for root in &self.roots {
            let dir = expand_home(root, &self.home);
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || name.ends_with("-agents") || !e.path().is_dir() {
                    continue;
                }
                let path = std::fs::canonicalize(e.path()).unwrap_or_else(|_| e.path()).display().to_string();
                candidates.push(Candidate { is_git: e.path().join(".git").exists(), registered: registered.contains(&path), name, path });
            }
        }
        candidates.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(ScanResult { candidates })
    }
}
```

xtask에 `devbox-projects.workspace = true`를 추가하고 `exporter()`에 `devbox_projects::export(&mut x);`를 추가한다.

- [ ] **Step 5: 통과·생성·커밋**

```bash
cargo test -p devbox-projects && cargo run -q -p xtask -- gen-ts
git add crates/projects xtask app/src/rpc/gen && git commit -m "feat(projects): add the project list domain"
```

---

### Task 13: 데몬 서버 (`devbox daemon`)

**Files:**
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `crates/cli/src/lib.rs`
- Create: `crates/cli/src/daemon/mod.rs`, `crates/cli/src/daemon/state.rs`, `crates/cli/src/daemon/server.rs`, `crates/cli/src/daemon/routes.rs`
- Create: `crates/cli/src/client.rs`(테스트·CLI용 클라이언트)
- Create: `crates/cli/src/{bridge,gateway,setup,doctor}.rs`(한 줄짜리 자리 함수. `lib.rs`·`main.rs`가 컴파일되게 하고 Task 14·15에서 채운다)
- Create: `crates/cli/tests/support/mod.rs`, `crates/cli/tests/daemon.rs`

**Interfaces:**
- Consumes: Task 2–12 전부
- Produces:
  - `devbox daemon [--foreground]`: `$DEVBOX_INSTANCE` 소켓에서 받고, `LISTEN_FDS=1`이면 fd 3을 쓰며, 준비되면 `NOTIFY_SOCKET`에 `READY=1`을 보낸다.
  - `Client::connect(socket, ClientKind) -> Client`, `Client::call::<M>(params) -> Result<M::Output, ClientError>`, `Client::subscribe(topic) -> Subscription`, `Client::raw_call(...)`
  - 테스트 지원 `TestDaemon::start() -> TestDaemon { paths, socket }`(임시 HOME, `test-<난수>` 인스턴스, 끝나면 종료)
  - 규칙:
    - Hello 전에는 `hello_required`
    - 채널은 처음 쓰일 때 열린다
    - `ChannelClose`는 그 채널의 구독·진행 중 요청을 정리한다
    - mutation + `clientRequestId`는 10분간 결과를 기억한다

- [ ] **Step 1: Cargo.toml과 main**

```toml
[package]
name = "devbox-cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "devbox"
path = "src/main.rs"

[dependencies]
devbox-core.workspace = true
devbox-protocol.workspace = true
devbox-projects.workspace = true
devbox-mux.workspace = true
anyhow.workspace = true
axum.workspace = true
bytes.workspace = true
clap.workspace = true
futures.workspace = true
mimalloc.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
uuid.workspace = true
sha2.workspace = true

[dev-dependencies]
tempfile.workspace = true
tokio-tungstenite.workspace = true

[lints]
workspace = true
```

```rust
// crates/cli/src/main.rs
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "devbox", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 백그라운드 데몬을 실행한다(보통 systemd가 실행).
    Daemon {
        #[arg(long)]
        foreground: bool,
    },
    /// stdin/stdout을 데몬 소켓에 잇는다(Windows 앱이 wsl.exe로 실행).
    Bridge,
    /// 개발·E2E용 WebSocket gateway.
    DevGateway {
        #[arg(long, default_value_t = 1451)]
        port: u16,
        #[arg(long, default_value = "http://localhost:1450")]
        origin: String,
        #[arg(long)]
        token: Option<String>,
    },
    /// systemd unit과 사용자 환경 파일을 설치한다.
    Setup {
        #[arg(long)]
        json: bool,
    },
    /// 환경을 점검한다.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// 버전만 출력한다.
    Version,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        match cli.cmd {
            Cmd::Daemon { foreground } => devbox_cli::daemon::run(foreground).await,
            Cmd::Bridge => devbox_cli::bridge::run().await,
            Cmd::DevGateway { port, origin, token } => devbox_cli::gateway::run(port, origin, token).await,
            Cmd::Setup { json } => devbox_cli::setup::run(json).await,
            Cmd::Doctor { json } => devbox_cli::doctor::run(json).await,
            Cmd::Version => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
        }
    })
}
```

```rust
// crates/cli/src/lib.rs
pub mod bridge;
pub mod client;
pub mod daemon;
pub mod doctor;
pub mod gateway;
pub mod setup;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn paths() -> anyhow::Result<devbox_core::Paths> {
    let env = devbox_core::OsEnv;
    Ok(devbox_core::Paths::resolve(devbox_core::Instance::from_env(&env)?, &env)?)
}
```

이 과제에서는 `bridge`·`gateway`·`setup`·`doctor`를 `pub async fn run(..) -> anyhow::Result<()> { anyhow::bail!("not yet") }` 한 줄짜리로 만들고, Task 14·15에서 채운다.

- [ ] **Step 2: 테스트 지원과 실패하는 통합 테스트**

```rust
// crates/cli/tests/support/mod.rs
#![allow(dead_code)]
use devbox_cli::client::Client;
use devbox_protocol::message::ClientKind;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub struct TestDaemon {
    pub home: tempfile::TempDir,
    pub instance: String,
    pub socket: PathBuf,
    child: Child,
}

impl TestDaemon {
    pub fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let instance = format!("test-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let runtime = home.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        let socket = runtime.join(format!("devbox-{instance}.sock"));
        let child = Command::new(env!("CARGO_BIN_EXE_devbox"))
            .args(["daemon", "--foreground"])
            .env("HOME", home.path())
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("DEVBOX_INSTANCE", &instance)
            .env_remove("XDG_DATA_HOME").env_remove("XDG_STATE_HOME").env_remove("XDG_CONFIG_HOME")
            .env_remove("LISTEN_FDS").env_remove("NOTIFY_SOCKET")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !socket.exists() {
            assert!(Instant::now() < deadline, "daemon did not create {}", socket.display());
            std::thread::sleep(Duration::from_millis(20));
        }
        Self { home, instance, socket, child }
    }

    pub async fn client(&self) -> Client {
        Client::connect(&self.socket, ClientKind::Test, &self.instance).await.unwrap()
    }

    /// 같은 HOME·인스턴스·소켓으로 데몬 프로세스만 다시 띄운다.
    pub fn restart(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        self.child = Command::new(env!("CARGO_BIN_EXE_devbox"))
            .args(["daemon", "--foreground"])
            .env("HOME", self.home.path())
            .env("XDG_RUNTIME_DIR", self.socket.parent().unwrap())
            .env("DEVBOX_INSTANCE", &self.instance)
            .env_remove("XDG_DATA_HOME").env_remove("XDG_STATE_HOME").env_remove("XDG_CONFIG_HOME")
            .env_remove("LISTEN_FDS").env_remove("NOTIFY_SOCKET")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.socket.exists() {
            assert!(Instant::now() < deadline, "daemon did not come back");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```

```rust
// crates/cli/tests/daemon.rs
mod support;
use devbox_cli::client::{Client, ClientError};
use devbox_projects::{AddProject, ListProjects, ProjectsAdd, ProjectsList};
use devbox_protocol::message::ClientKind;
use devbox_protocol::system::{Ping, PingParams};
use support::TestDaemon;

#[tokio::test]
async fn ping_and_project_round_trip_with_events() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let pong = c.call::<Ping>(&PingParams {}).await.unwrap();
    assert_eq!(pong.version, devbox_cli::VERSION);

    let mut sub = c.subscribe("projects.changed").await.unwrap();
    let dir = d.home.path().join("projects/alpha");
    std::fs::create_dir_all(&dir).unwrap();
    let p = c.call::<ProjectsAdd>(&AddProject { path: dir.display().to_string() }).await.unwrap();
    let ev = sub.next_event().await.unwrap();
    assert!(ev.rev.parse::<u64>().unwrap() >= 1);
    let list = c.call::<ProjectsList>(&ListProjects {}).await.unwrap();
    assert_eq!(list.items[0].id, p.id);
}

#[tokio::test]
async fn domain_errors_and_unknown_methods_are_typed() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let e = c.call::<ProjectsAdd>(&AddProject { path: "relative".into() }).await.unwrap_err();
    assert!(matches!(e, ClientError::Rpc { ref code, .. } if code == "projects.path_invalid"), "{e:?}");
    let e = c.raw_call("nope.method", serde_json::json!({}), None).await.unwrap_err();
    assert!(matches!(e, ClientError::Rpc { ref code, .. } if code == "method_not_found"));
}

#[tokio::test]
async fn requests_before_hello_are_refused() {
    let d = TestDaemon::start();
    let c = Client::connect_without_hello(&d.socket).await.unwrap();
    let e = c.raw_call("system.ping", serde_json::json!({}), None).await.unwrap_err();
    assert!(matches!(e, ClientError::Rpc { ref code, .. } if code == "hello_required"));
}

#[tokio::test]
async fn same_client_request_id_runs_a_mutation_once() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let dir = d.home.path().join("projects/beta");
    std::fs::create_dir_all(&dir).unwrap();
    let params = serde_json::json!({ "path": dir.display().to_string() });
    let a = c.raw_call("projects.add", params.clone(), Some("req-1".into())).await.unwrap();
    let b = c.raw_call("projects.add", params, Some("req-1".into())).await.unwrap();
    assert_eq!(a, b, "second call must return the remembered result, not already_exists");
}

#[tokio::test]
async fn closing_a_channel_stops_its_subscriptions() {
    let d = TestDaemon::start();
    let c = Client::connect(&d.socket, ClientKind::Test, &d.instance).await.unwrap();
    let mut sub = c.subscribe("projects.changed").await.unwrap();
    c.close_channel().await.unwrap();
    let other = d.client().await;
    let dir = d.home.path().join("projects/gamma");
    std::fs::create_dir_all(&dir).unwrap();
    other.call::<ProjectsAdd>(&AddProject { path: dir.display().to_string() }).await.unwrap();
    assert!(tokio::time::timeout(std::time::Duration::from_millis(300), sub.next_event()).await.is_err());
}
```

`[dev-dependencies]`에 `uuid.workspace = true`와 `serde_json.workspace = true`를 추가한다.

- [ ] **Step 3: 실패 확인** — `cargo test -p devbox-cli --test daemon` → 컴파일 실패

- [ ] **Step 4: 구현 — 클라이언트**

```rust
// crates/cli/src/client.rs
//! 데몬 클라이언트(CLI·MCP·테스트). 채널 0 하나를 쓴다.
use devbox_protocol::frame::{Frame, FrameCodec, FrameKind};
use devbox_protocol::message::{control_frame, json_frame, parse_json, ClientKind, ClientMessage, Control, ServerMessage};
use devbox_protocol::Method;
use futures::{SinkExt, StreamExt};
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::Framed;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("rpc error {code}")]
    Rpc { code: String, detail: Option<serde_json::Value>, diagnostic_id: Option<String> },
    #[error("connection lost")]
    ConnectionLost,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}

pub struct SubEvent {
    pub rev: String,
    pub payload: String,
}

pub struct Subscription {
    rx: mpsc::Receiver<Option<SubEvent>>,
}

impl Subscription {
    /// 다음 이벤트. `resync`는 rev가 빈 이벤트로 온다.
    pub async fn next_event(&mut self) -> Option<SubEvent> {
        self.rx.recv().await.flatten()
    }
}

type Pending = Arc<Mutex<HashMap<u32, oneshot::Sender<ServerMessage>>>>;
type Subs = Arc<Mutex<HashMap<u32, mpsc::Sender<Option<SubEvent>>>>>;

pub struct Client {
    out: mpsc::Sender<Frame>,
    pending: Pending,
    subs: Subs,
    next: AtomicU32,
}

impl Client {
    pub async fn connect(socket: &Path, kind: ClientKind, instance: &str) -> Result<Self, ClientError> {
        let c = Self::connect_without_hello(socket).await?;
        let hello = ClientMessage::Hello { version: crate::VERSION.into(), protocol: devbox_protocol::PROTOCOL_VERSION, client: kind, instance: instance.into() };
        c.send_and_wait(0, hello).await?;
        Ok(c)
    }

    pub async fn connect_without_hello(socket: &Path) -> Result<Self, ClientError> {
        let stream = UnixStream::connect(socket).await?;
        let (mut sink, mut source) = Framed::new(stream, FrameCodec::default()).split();
        let (out, mut out_rx) = mpsc::channel::<Frame>(256);
        let pending: Pending = Arc::default();
        let subs: Subs = Arc::default();
        tokio::spawn(async move {
            while let Some(f) = out_rx.recv().await {
                if sink.send(f).await.is_err() {
                    break;
                }
            }
        });
        let (p2, s2) = (pending.clone(), subs.clone());
        tokio::spawn(async move {
            while let Some(Ok(frame)) = source.next().await {
                if frame.kind != FrameKind::Json {
                    continue;
                }
                let Ok(msg) = parse_json::<ServerMessage>(&frame) else { continue };
                match &msg {
                    ServerMessage::Welcome { .. } => {
                        if let Some(tx) = p2.lock().expect("pending").remove(&0) { let _ = tx.send(msg); }
                    }
                    ServerMessage::Response { id, .. } | ServerMessage::Subscribed { id, .. } => {
                        if let Some(tx) = p2.lock().expect("pending").remove(id) { let _ = tx.send(msg); }
                    }
                    ServerMessage::Event { sub_id, rev, payload } => {
                        if let Some(tx) = s2.lock().expect("subs").get(sub_id) {
                            let _ = tx.try_send(Some(SubEvent { rev: rev.clone(), payload: payload.get().to_owned() }));
                        }
                    }
                    ServerMessage::Resync { sub_id } => {
                        if let Some(tx) = s2.lock().expect("subs").get(sub_id) {
                            let _ = tx.try_send(Some(SubEvent { rev: String::new(), payload: String::new() }));
                        }
                    }
                }
            }
            p2.lock().expect("pending").clear();
            s2.lock().expect("subs").clear();
        });
        Ok(Self { out, pending, subs, next: AtomicU32::new(1) })
    }

    async fn send_and_wait(&self, id: u32, msg: ClientMessage) -> Result<ServerMessage, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("pending").insert(id, tx);
        self.out.send(json_frame(0, &msg)).await.map_err(|_| ClientError::ConnectionLost)?;
        rx.await.map_err(|_| ClientError::ConnectionLost)
    }

    pub async fn raw_call(&self, method: &str, params: serde_json::Value, client_request_id: Option<String>) -> Result<String, ClientError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let params = RawValue::from_string(params.to_string())?;
        match self.send_and_wait(id, ClientMessage::Request { id, method: method.into(), params, client_request_id }).await? {
            ServerMessage::Response { result: Some(r), .. } => Ok(r.get().to_owned()),
            ServerMessage::Response { error: Some(e), diagnostic_id, .. } => Err(ClientError::Rpc { code: e.code, detail: e.detail, diagnostic_id }),
            _ => Err(ClientError::ConnectionLost),
        }
    }

    pub async fn call<M: Method>(&self, params: &M::Params) -> Result<M::Output, ClientError>
    where
        M::Params: serde::Serialize,
        M::Output: serde::de::DeserializeOwned,
    {
        let text = self.raw_call(M::NAME, serde_json::to_value(params)?, None).await?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn subscribe(&self, topic: &str) -> Result<Subscription, ClientError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(256);
        match self.send_and_wait(id, ClientMessage::Subscribe { id, topic: topic.into(), params: None }).await? {
            ServerMessage::Subscribed { sub_id, .. } => {
                self.subs.lock().expect("subs").insert(sub_id, tx);
                Ok(Subscription { rx })
            }
            ServerMessage::Response { error: Some(e), diagnostic_id, .. } => Err(ClientError::Rpc { code: e.code, detail: e.detail, diagnostic_id }),
            _ => Err(ClientError::ConnectionLost),
        }
    }

    pub async fn close_channel(&self) -> Result<(), ClientError> {
        self.out.send(control_frame(0, &Control::ChannelClose)).await.map_err(|_| ClientError::ConnectionLost)
    }
}
```

`Client::connect`의 `send_and_wait(0, hello)`는 Welcome을 대기 표의 0번으로 받는다. 요청 ID는 1부터 쓰므로 겹치지 않는다.

- [ ] **Step 5: 구현 — 데몬 상태·라우트·서버**

```rust
// crates/cli/src/daemon/state.rs
use devbox_core::{Db, Hub, Paths};
use devbox_projects::ProjectsService;
use std::sync::Arc;
use std::time::Instant;

pub struct Services {
    pub version: &'static str,
    pub daemon_id: String,
    pub paths: Paths,
    pub started: Instant,
    pub started_ms: i64,
    pub db: Db,
    pub hub: Arc<Hub>,
    pub projects: ProjectsService,
}
```

```rust
// crates/cli/src/daemon/routes.rs
use super::state::Services;
use devbox_projects as projects;
use devbox_protocol::system::{DaemonInfo, Info, Ping, Pong};
use devbox_protocol::{Exporter, NameCollector, Router};
use std::sync::Arc;

pub fn router(s: Arc<Services>) -> Router<Services> {
    Router::new(s)
        .add::<Ping, _, _>(|s, _, _| async move {
            Ok(Pong { daemon_id: s.daemon_id.clone(), version: s.version.into(), uptime_ms: s.started.elapsed().as_millis() as i64 })
        })
        .add::<Info, _, _>(|s, _, _| async move {
            Ok(DaemonInfo {
                version: s.version.into(),
                instance: s.paths.instance.as_str().into(),
                pid: std::process::id(),
                started_ms: s.started_ms,
                data_dir: s.paths.data_dir.display().to_string(),
            })
        })
        .add::<projects::ProjectsList, _, _>(|s, _, p| async move { s.projects.list(p).await })
        .add::<projects::ProjectsAdd, _, _>(|s, _, p| async move { s.projects.add(p).await })
        .add::<projects::ProjectsRemove, _, _>(|s, _, p| async move { s.projects.remove(p).await })
        .add::<projects::ProjectsScan, _, _>(|s, _, p| async move { s.projects.scan(p).await })
}

/// 선언된 메서드·주제 전체(라우터 완전성 시험과 주제 검증에 쓴다).
pub fn declared() -> NameCollector {
    let mut n = NameCollector::default();
    devbox_protocol::system::export(&mut n);
    projects::export(&mut n);
    n
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_declared_method_is_routed() {
        let declared = super::declared();
        let t = tempfile::tempdir().unwrap();
        let s = crate::daemon::build_services_for_test(t.path());
        let r = super::router(s);
        let mut want = declared.methods.clone();
        want.sort_unstable();
        assert_eq!(r.names(), want);
    }
}
```

```rust
// crates/cli/src/daemon/mod.rs
pub mod routes;
pub mod server;
pub mod state;

use devbox_core::{log, Config, Db, Hub, Paths};
use state::Services;
use std::sync::Arc;
use std::time::Instant;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn migrations() -> Vec<devbox_core::Migration> {
    devbox_projects::MIGRATIONS.to_vec()
}

pub fn build_services(paths: Paths) -> anyhow::Result<Arc<Services>> {
    paths.ensure_dirs()?;
    let config = Config::load(&paths)?;
    let db = Db::open(&paths.db_path, &paths.backups_dir, crate::VERSION, &migrations())?;
    let hub = Arc::new(Hub::new());
    let projects = devbox_projects::ProjectsService::new(db.clone(), hub.clone(), paths.home.clone(), config.projects.roots.clone());
    Ok(Arc::new(Services {
        version: crate::VERSION,
        daemon_id: uuid::Uuid::new_v4().simple().to_string(),
        started: Instant::now(),
        started_ms: now_ms(),
        paths,
        db,
        hub,
        projects,
    }))
}

#[cfg(test)]
pub fn build_services_for_test(home: &std::path::Path) -> Arc<Services> {
    struct E(String);
    impl devbox_core::Env for E {
        fn var(&self, k: &str) -> Option<String> { (k == "HOME").then(|| self.0.clone()) }
        fn uid(&self) -> u32 { 1000 }
    }
    let paths = Paths::resolve(devbox_core::Instance::parse("test-r").unwrap(), &E(home.display().to_string())).unwrap();
    build_services(paths).unwrap()
}

pub async fn run(foreground: bool) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    paths.ensure_dirs()?;
    let _guard = log::init_logging(&paths, foreground)?;
    log::install_panic_hook(&paths, crate::VERSION);
    let services = build_services(paths.clone())?;
    let listener = server::listener(&paths.socket)?;
    server::notify_ready();
    tracing::info!(instance = paths.instance.as_str(), socket = %paths.socket.display(), "daemon ready");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let s2 = shutdown.clone();
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm");
        tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
        s2.cancel();
    });
    server::serve(listener, services, shutdown).await?;
    tracing::info!("daemon stopped");
    Ok(())
}
```

`build_services`의 `Config::load`·`Db::open`이 돌려주는 `CoreError`는 `anyhow`로 바뀐다. `CoreError`가 `std::error::Error`를 구현하므로 `?`로 충분하다.

```rust
// crates/cli/src/daemon/server.rs
use super::routes;
use super::state::Services;
use devbox_core::HubMessage;
use devbox_protocol::frame::{Frame, FrameCodec, FrameKind};
use devbox_protocol::message::{json_frame, parse_json, ClientMessage, Control, ServerMessage};
use devbox_protocol::{CommonError, Ctx, DispatchError, MethodKind, Router, PROTOCOL_VERSION};
use futures::{SinkExt, StreamExt};
use serde_json::value::RawValue;
use std::collections::{HashMap, HashSet};
use std::os::fd::FromRawFd;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;

const DEDUPE_TTL: Duration = Duration::from_secs(600);

pub struct Daemon {
    router: Router<Services>,
    services: Arc<Services>,
    topics: HashSet<&'static str>,
    dedupe: Mutex<HashMap<String, (Instant, Result<Box<RawValue>, DispatchError>)>>,
}

#[allow(unsafe_code)]
pub fn listener(path: &Path) -> std::io::Result<UnixListener> {
    if std::env::var("LISTEN_FDS").ok().as_deref() == Some("1") {
        // SAFETY: systemd passes exactly one listening socket as fd 3 when LISTEN_FDS=1.
        let l = unsafe { std::os::unix::net::UnixListener::from_raw_fd(3) };
        l.set_nonblocking(true)?;
        return UnixListener::from_std(l);
    }
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let l = UnixListener::bind(path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

pub fn notify_ready() {
    let Ok(addr) = std::env::var("NOTIFY_SOCKET") else { return };
    if let Ok(sock) = std::os::unix::net::UnixDatagram::unbound() {
        let _ = sock.send_to(b"READY=1", addr);
    }
}

pub async fn serve(listener: UnixListener, services: Arc<Services>, shutdown: CancellationToken) -> std::io::Result<()> {
    let declared = routes::declared();
    let daemon = Arc::new(Daemon {
        router: routes::router(services.clone()),
        topics: declared.topics.into_iter().collect(),
        services,
        dedupe: Mutex::new(HashMap::new()),
    });
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let d = daemon.clone();
                tokio::spawn(async move {
                    if let Err(e) = connection(stream, d).await {
                        tracing::debug!(error = %e, "connection closed with error");
                    }
                });
            }
        }
    }
}

#[derive(Default)]
struct ChannelState {
    hello: bool,
    subs: HashMap<u32, tokio::task::AbortHandle>,
}

type Inflight = Arc<Mutex<HashMap<(u32, u32), CancellationToken>>>;

async fn connection(stream: UnixStream, d: Arc<Daemon>) -> Result<(), devbox_protocol::frame::CodecError> {
    let (mut sink, mut source) = Framed::new(stream, FrameCodec::default()).split();
    let (tx, mut rx) = mpsc::channel::<Frame>(1024);
    let writer = tokio::spawn(async move {
        while let Some(f) = rx.recv().await {
            if sink.send(f).await.is_err() {
                break;
            }
        }
    });
    let mut channels: HashMap<u32, ChannelState> = HashMap::new();
    let inflight: Inflight = Arc::default();
    let mut next_sub: u32 = 1;
    let result = async {
        while let Some(frame) = source.next().await {
            let frame = frame?;
            let ch = frame.channel;
            match frame.kind {
                FrameKind::Control => match parse_json::<Control>(&frame) {
                    Ok(Control::ChannelOpen) => close_channel(&mut channels, &inflight, ch, true),
                    Ok(Control::ChannelClose) => close_channel(&mut channels, &inflight, ch, false),
                    Ok(Control::StreamCredit { .. }) | Err(_) => {}
                },
                FrameKind::Stream => {}
                FrameKind::Json => {
                    let Ok(msg) = parse_json::<ClientMessage>(&frame) else { continue };
                    let state = channels.entry(ch).or_default();
                    handle_message(msg, ch, state, &d, &tx, &inflight, &mut next_sub).await;
                }
            }
        }
        Ok(())
    }
    .await;
    for ch in channels.keys().copied().collect::<Vec<_>>() {
        close_channel(&mut channels, &inflight, ch, false);
    }
    writer.abort();
    result
}

fn close_channel(channels: &mut HashMap<u32, ChannelState>, inflight: &Inflight, ch: u32, reopen: bool) {
    if let Some(old) = channels.remove(&ch) {
        for (_, h) in old.subs {
            h.abort();
        }
    }
    inflight.lock().expect("inflight").retain(|(c, _), t| {
        if *c == ch {
            t.cancel();
            false
        } else {
            true
        }
    });
    if reopen {
        channels.insert(ch, ChannelState::default());
    }
}

fn response(id: u32, r: Result<Box<RawValue>, DispatchError>) -> ServerMessage {
    match r {
        Ok(result) => ServerMessage::Response { id, result: Some(result), error: None, diagnostic_id: None },
        Err(e) => ServerMessage::Response { id, result: None, error: Some(e.error), diagnostic_id: e.diagnostic_id },
    }
}

async fn handle_message(
    msg: ClientMessage,
    ch: u32,
    state: &mut ChannelState,
    d: &Arc<Daemon>,
    tx: &mpsc::Sender<Frame>,
    inflight: &Inflight,
    next_sub: &mut u32,
) {
    match msg {
        ClientMessage::Hello { protocol, .. } => {
            state.hello = protocol == PROTOCOL_VERSION;
            let _ = tx.send(json_frame(ch, &ServerMessage::Welcome {
                version: d.services.version.into(),
                protocol: PROTOCOL_VERSION,
                daemon_id: d.services.daemon_id.clone(),
            })).await;
        }
        ClientMessage::Request { id, .. } | ClientMessage::Subscribe { id, .. } if !state.hello => {
            let _ = tx.send(json_frame(ch, &response(id, Err(DispatchError::common(CommonError::HelloRequired))))).await;
        }
        ClientMessage::Request { id, method, params, client_request_id } => {
            let token = CancellationToken::new();
            inflight.lock().expect("inflight").insert((ch, id), token.clone());
            let (d, tx, inflight) = (d.clone(), tx.clone(), inflight.clone());
            tokio::spawn(async move {
                let r = d.dispatch(&method, Ctx { channel: ch, cancel: token }, params, client_request_id).await;
                inflight.lock().expect("inflight").remove(&(ch, id));
                let _ = tx.send(json_frame(ch, &response(id, r))).await;
            });
        }
        ClientMessage::Cancel { id } => {
            if let Some(t) = inflight.lock().expect("inflight").get(&(ch, id)) {
                t.cancel();
            }
        }
        ClientMessage::Subscribe { id, topic, .. } => {
            if !d.topics.contains(topic.as_str()) {
                let e = DispatchError::common(CommonError::MethodNotFound { method: topic });
                let _ = tx.send(json_frame(ch, &response(id, Err(e)))).await;
                return;
            }
            let sub_id = *next_sub;
            *next_sub += 1;
            let mut rx = d.services.hub.subscribe(&topic);
            let rev = d.services.db.rev().to_string();
            let _ = tx.send(json_frame(ch, &ServerMessage::Subscribed { id, sub_id, rev })).await;
            let tx2 = tx.clone();
            let handle = tokio::spawn(async move {
                while let Some(m) = rx.recv().await {
                    let msg = match m {
                        HubMessage::Event { rev, payload } => match RawValue::from_string(payload.to_string()) {
                            Ok(p) => ServerMessage::Event { sub_id, rev: rev.to_string(), payload: p },
                            Err(_) => continue,
                        },
                        HubMessage::Resync => ServerMessage::Resync { sub_id },
                    };
                    if tx2.send(json_frame(ch, &msg)).await.is_err() {
                        break;
                    }
                }
            })
            .abort_handle();
            state.subs.insert(sub_id, handle);
        }
        ClientMessage::Unsubscribe { sub_id } => {
            if let Some(h) = state.subs.remove(&sub_id) {
                h.abort();
            }
        }
    }
}

impl Daemon {
    async fn dispatch(&self, method: &str, ctx: Ctx, params: Box<RawValue>, crid: Option<String>) -> Result<Box<RawValue>, DispatchError> {
        let key = match (self.router.kind_of(method), crid) {
            (Some(MethodKind::Mutation), Some(c)) => Some(format!("{method}\u{0}{c}")),
            _ => None,
        };
        if let Some(k) = &key {
            let mut map = self.dedupe.lock().expect("dedupe");
            map.retain(|_, (at, _)| at.elapsed() < DEDUPE_TTL);
            if let Some((_, r)) = map.get(k) {
                return r.clone();
            }
        }
        let r = self.router.dispatch(method, ctx, params).await;
        if let Some(k) = key {
            self.dedupe.lock().expect("dedupe").insert(k, (Instant::now(), r.clone()));
        }
        r
    }
}
```

`listener`의 `unsafe` 한 곳에만 `#[allow(unsafe_code)]`를 붙였다. SAFETY 주석을 지운다.

- [ ] **Step 6: 통과·커밋**

```bash
cargo test -p devbox-cli
git add crates/cli && git commit -m "feat(cli): serve typed RPC and subscriptions over the daemon socket"
```

Expected: 통합 테스트 5개와 라우터 완전성 시험이 통과한다.

---

### Task 14: 브리지와 dev-gateway

**Files:**
- Modify: `crates/cli/src/bridge.rs`, `crates/cli/src/gateway.rs`
- Create: `crates/cli/tests/bridge_gateway.rs`

**Interfaces:**
- Consumes: Task 13의 소켓, `devbox_mux::FrameSplitter`
- Produces:
  - `devbox bridge`: 표지 `DEVBOX-BRIDGE/1\n`(16바이트)를 쓴 뒤 stdin↔socket을 중계한다. 소켓이 없으면 `systemctl --user start devbox@<i>.socket`을 한 번 실행하고, 5초까지 0.1초 간격으로 다시 연결한다.
  - `devbox dev-gateway --port P --origin O [--token T]`: `GET /ws?token=T`. Origin이 다르면 403, 토큰이 다르면 401. WebSocket 바이너리 메시지 하나는 완성된 프레임 하나다.
  - 03b의 Tauri 껍데기와 화면 E2E가 사용한다.

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/cli/tests/bridge_gateway.rs
mod support;
use futures::{SinkExt, StreamExt};
use std::process::Stdio;
use support::TestDaemon;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

fn hello_frame() -> Vec<u8> {
    let body = br#"{"type":"hello","version":"t","protocol":1,"client":"test","instance":"x"}"#;
    let mut f = ((5 + body.len()) as u32).to_le_bytes().to_vec();
    f.push(0);
    f.extend(0u32.to_le_bytes());
    f.extend_from_slice(body);
    f
}

#[tokio::test]
async fn bridge_writes_magic_then_relays_frames() {
    let d = TestDaemon::start();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_devbox"))
        .arg("bridge")
        .env("HOME", d.home.path())
        .env("XDG_RUNTIME_DIR", d.socket.parent().unwrap())
        .env("DEVBOX_INSTANCE", &d.instance)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let mut magic = [0u8; 16];
    out.read_exact(&mut magic).await.unwrap();
    assert_eq!(&magic, b"DEVBOX-BRIDGE/1\n");
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&hello_frame()).await.unwrap();
    let mut len = [0u8; 4];
    out.read_exact(&mut len).await.unwrap();
    let mut rest = vec![0u8; u32::from_le_bytes(len) as usize];
    out.read_exact(&mut rest).await.unwrap();
    assert!(String::from_utf8_lossy(&rest[5..]).contains("\"welcome\""));
    drop(stdin);
    assert!(tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.is_ok());
}

async fn gateway(d: &TestDaemon) -> (tokio::process::Child, u16) {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_devbox"))
        .args(["dev-gateway", "--port", &port.to_string(), "--token", "tok", "--origin", "http://localhost:1450"])
        .env("HOME", d.home.path())
        .env("XDG_RUNTIME_DIR", d.socket.parent().unwrap())
        .env("DEVBOX_INSTANCE", &d.instance)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    (child, port)
}

#[tokio::test]
async fn gateway_checks_token_and_origin_then_relays() {
    let d = TestDaemon::start();
    let (_g, port) = gateway(&d).await;
    let url = |t: &str| format!("ws://127.0.0.1:{port}/ws?token={t}");

    let mut bad = url("nope").into_client_request().unwrap();
    bad.headers_mut().insert("Origin", "http://localhost:1450".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(bad).await.is_err());

    let mut wrong_origin = url("tok").into_client_request().unwrap();
    wrong_origin.headers_mut().insert("Origin", "http://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(wrong_origin).await.is_err());

    let mut ok = url("tok").into_client_request().unwrap();
    ok.headers_mut().insert("Origin", "http://localhost:1450".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(ok).await.unwrap();
    ws.send(Message::Binary(hello_frame().into())).await.unwrap();
    let Some(Ok(Message::Binary(frame))) = ws.next().await else { panic!("expected a binary frame") };
    assert!(String::from_utf8_lossy(&frame[9..]).contains("\"welcome\""));
}
```

`[dev-dependencies]`에 `futures.workspace = true`와 `tokio = { workspace = true, features = ["process", "macros"] }`를 추가한다.

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-cli --test bridge_gateway`

- [ ] **Step 3: 구현 — 브리지**

```rust
// crates/cli/src/bridge.rs
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

pub const MAGIC: &[u8; 16] = b"DEVBOX-BRIDGE/1\n";

async fn connect(paths: &devbox_core::Paths) -> std::io::Result<UnixStream> {
    if let Ok(s) = UnixStream::connect(&paths.socket).await {
        return Ok(s);
    }
    let unit = format!("devbox@{}.socket", paths.instance.as_str());
    let _ = tokio::process::Command::new("systemctl").args(["--user", "start", &unit]).status().await;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(&paths.socket).await {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    let mut stdout = tokio::io::stdout();
    stdout.write_all(MAGIC).await?;
    stdout.flush().await?;
    let paths = crate::paths()?;
    let sock = connect(&paths).await.map_err(|e| anyhow::anyhow!("devbox daemon unreachable at {}: {e}", paths.socket.display()))?;
    let (mut sr, mut sw) = sock.into_split();
    let mut stdin = tokio::io::stdin();
    tokio::select! {
        r = tokio::io::copy(&mut stdin, &mut sw) => { r?; }
        r = tokio::io::copy(&mut sr, &mut stdout) => { r?; }
    }
    Ok(())
}
```

- [ ] **Step 4: 구현 — gateway**

```rust
// crates/cli/src/gateway.rs
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use devbox_mux::FrameSplitter;
use futures::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Gw {
    token: String,
    origin: String,
    socket: PathBuf,
}

#[derive(serde::Deserialize)]
struct Q {
    token: Option<String>,
}

pub async fn run(port: u16, origin: String, token: Option<String>) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let token = token.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    println!("dev-gateway ws://127.0.0.1:{port}/ws?token={token}");
    let st = Arc::new(Gw { token, origin, socket: paths.socket });
    let app = axum::Router::new().route("/ws", get(upgrade)).with_state(st);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn upgrade(ws: WebSocketUpgrade, Query(q): Query<Q>, headers: HeaderMap, State(st): State<Arc<Gw>>) -> Response {
    if q.token.as_deref() != Some(st.token.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if headers.get("origin").and_then(|o| o.to_str().ok()) != Some(st.origin.as_str()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let socket = st.socket.clone();
    ws.on_upgrade(move |w| async move {
        if let Err(e) = relay(w, socket).await {
            tracing::debug!(error = %e, "gateway relay ended");
        }
    })
}

async fn relay(ws: WebSocket, socket: PathBuf) -> anyhow::Result<()> {
    let unix = tokio::net::UnixStream::connect(&socket).await?;
    let (mut ur, mut uw) = unix.into_split();
    let (mut wtx, mut wrx) = ws.split();
    let up = async {
        let mut check = FrameSplitter::default();
        while let Some(Ok(m)) = wrx.next().await {
            if let Message::Binary(b) = m {
                let frames = check.push(&b)?;
                for f in frames {
                    uw.write_all(&f).await?;
                }
            }
        }
        anyhow::Ok(())
    };
    let down = async {
        let mut split = FrameSplitter::default();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = ur.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            for f in split.push(&buf[..n])? {
                wtx.send(Message::Binary(f.into())).await?;
            }
        }
        anyhow::Ok(())
    };
    tokio::select! { r = up => r?, r = down => r? }
    Ok(())
}
```

`FrameSplitter::push`의 오류는 `MuxError`이므로 `anyhow`로 바뀐다(`thiserror` 구현).

- [ ] **Step 5: 통과·커밋**

```bash
cargo test -p devbox-cli --test bridge_gateway
git add crates/cli && git commit -m "feat(cli): add the stdio bridge and the development WebSocket gateway"
```

---

### Task 15: `setup`과 `doctor`

**Files:**
- Modify: `crates/cli/src/setup.rs`, `crates/cli/src/doctor.rs`
- Modify: `crates/protocol/src/system.rs`(`system.doctor` 메서드), `crates/cli/src/daemon/routes.rs`

**Interfaces:**
- Produces:
  - `setup::render_units() -> Vec<(&'static str, String)>`(파일 이름, 내용)
  - `setup::capture_env(raw: &[u8]) -> String`(허용 목록 키만, EnvironmentFile 따옴표 처리)
  - `devbox setup [--json]`: 다음을 하고 `SetupReport { steps: Vec<{ id, ok, message }> }`를 출력한다.
    1. 사용자 환경 파일 작성(`~/.config/devbox/<i>/env`, 0600)
    2. unit 작성
    3. daemon-reload
    4. `devbox@<i>.socket`·`devbox@<i>.service`를 `enable --now`(이미 실행 중이면 restart)
  - `doctor::checks(&Probe) -> Vec<Check>`. `Check { id, status: ok|warn|fail, message, fix? }`, `Probe`는 외부 조회를 감싼 trait.
  - `devbox doctor [--json]`
  - RPC `system.doctor`(query) → `DoctorReport { checks }`. 03b의 첫 실행 화면·진단 화면이 쓴다.

- [ ] **Step 1: 실패하는 테스트**

```rust
// crates/cli/src/setup.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_capture_keeps_only_tool_paths_and_quotes_values() {
        let raw = b"PATH=/home/u/.local/bin:/usr/bin\0GITHUB_TOKEN=secret\0LANG=ko_KR.UTF-8\0NVM_DIR=/home/u/.nvm\0WEIRD=a\"b\0LC_ALL=C\0";
        let out = capture_env(raw);
        assert!(out.contains("PATH=\"/home/u/.local/bin:/usr/bin\"\n"));
        assert!(out.contains("LANG=\"ko_KR.UTF-8\"\n"));
        assert!(out.contains("NVM_DIR=\"/home/u/.nvm\"\n"));
        assert!(out.contains("LC_ALL=\"C\"\n"));
        assert!(!out.contains("GITHUB_TOKEN"));
        assert!(!out.contains("WEIRD"));
    }

    #[test]
    fn units_reference_instance_env_file_and_socket() {
        let units = render_units();
        let names: Vec<_> = units.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["devbox@.socket", "devbox@.service"]);
        let svc = &units[1].1;
        assert!(svc.contains("Type=notify"));
        assert!(svc.contains("EnvironmentFile=-%h/.config/devbox/%i/env"));
        assert!(svc.contains("Environment=DEVBOX_INSTANCE=%i"));
        assert!(svc.contains("ExecStart=%h/.local/share/devbox/bin/devbox daemon"));
        assert!(units[0].1.contains("ListenStream=%t/devbox-%i.sock"));
        assert!(units[0].1.contains("SocketMode=0600"));
    }
}
```

```rust
// crates/cli/src/doctor.rs 끝
#[cfg(test)]
mod tests {
    use super::*;

    struct Fake { pid1: &'static str, linger: &'static str, tmux: &'static str, inotify: &'static str, env_file: Option<&'static str> }
    impl Probe for Fake {
        fn pid1(&self) -> String { self.pid1.into() }
        fn linger(&self) -> String { self.linger.into() }
        fn tmux_version(&self) -> Option<String> { Some(self.tmux.into()) }
        fn inotify_instances(&self) -> String { self.inotify.into() }
        fn env_file(&self) -> Option<String> { self.env_file.map(Into::into) }
        fn which(&self, tool: &str, path: &str) -> bool { path.contains(".local/bin") || tool == "git" }
    }

    #[test]
    fn healthy_machine_is_all_ok_or_warn() {
        let c = checks(&Fake { pid1: "systemd", linger: "Linger=yes", tmux: "tmux 3.4", inotify: "1024", env_file: Some("PATH=\"/home/u/.local/bin:/usr/bin\"\n") });
        assert!(c.iter().all(|c| c.status != Status::Fail), "{c:?}");
        assert_eq!(c.iter().find(|c| c.id == "tmux").unwrap().status, Status::Ok);
    }

    #[test]
    fn reports_systemd_old_tmux_low_inotify_and_missing_env() {
        let c = checks(&Fake { pid1: "init", linger: "Linger=no", tmux: "tmux 3.1c", inotify: "128", env_file: None });
        let get = |id: &str| c.iter().find(|c| c.id == id).unwrap().status;
        assert_eq!(get("systemd"), Status::Fail);
        assert_eq!(get("tmux"), Status::Fail);
        assert_eq!(get("linger"), Status::Warn);
        assert_eq!(get("inotify"), Status::Warn);
        assert_eq!(get("env"), Status::Fail);
    }

    #[test]
    fn parses_tmux_versions() {
        assert_eq!(tmux_at_least("tmux 3.2", (3, 2)), true);
        assert_eq!(tmux_at_least("tmux 3.3a", (3, 2)), true);
        assert_eq!(tmux_at_least("tmux next-3.6", (3, 2)), true);
        assert_eq!(tmux_at_least("tmux 2.9", (3, 2)), false);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p devbox-cli setup doctor`

- [ ] **Step 3: 구현 — setup**

```rust
// crates/cli/src/setup.rs (테스트 위)
use serde::Serialize;

const ALLOWED: &[&str] = &["PATH", "LANG", "NVM_DIR", "NVM_BIN", "CARGO_HOME", "RUSTUP_HOME", "GOPATH", "PNPM_HOME", "BUN_INSTALL", "DENO_INSTALL", "JAVA_HOME", "EDITOR", "SHELL", "TZ"];

pub fn capture_env(raw: &[u8]) -> String {
    let mut out = String::new();
    for entry in raw.split(|b| *b == 0) {
        let Ok(entry) = std::str::from_utf8(entry) else { continue };
        let Some((k, v)) = entry.split_once('=') else { continue };
        if !(ALLOWED.contains(&k) || k.starts_with("LC_")) || v.contains('"') || v.contains('\n') {
            continue;
        }
        out.push_str(&format!("{k}=\"{}\"\n", v.replace('\\', "\\\\")));
    }
    out
}

pub fn render_units() -> Vec<(&'static str, String)> {
    vec![
        (
            "devbox@.socket",
            "[Unit]\nDescription=devbox daemon socket (%i)\n\n[Socket]\nListenStream=%t/devbox-%i.sock\nSocketMode=0600\n\n[Install]\nWantedBy=sockets.target\n".into(),
        ),
        (
            "devbox@.service",
            "[Unit]\nDescription=devbox daemon (%i)\nRequires=devbox@%i.socket\nAfter=devbox@%i.socket\n\n[Service]\nType=notify\nEnvironmentFile=-%h/.config/devbox/%i/env\nEnvironment=DEVBOX_INSTANCE=%i\nExecStart=%h/.local/share/devbox/bin/devbox daemon\nRestart=on-failure\nRestartSec=1\nKillMode=mixed\n\n[Install]\nWantedBy=default.target\n".into(),
        ),
    ]
}

#[derive(Serialize)]
pub struct Step {
    pub id: &'static str,
    pub ok: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct SetupReport {
    pub steps: Vec<Step>,
}

async fn systemctl(args: &[&str]) -> Result<(), String> {
    let out = tokio::process::Command::new("systemctl").arg("--user").args(args).output().await.map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()) }
}

pub async fn run(json: bool) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let i = paths.instance.as_str().to_owned();
    let mut steps = Vec::new();
    let mut step = |id, r: Result<(), String>| {
        let ok = r.is_ok();
        steps.push(Step { id, ok, message: r.err().unwrap_or_default() });
        ok
    };

    let env = tokio::process::Command::new("bash").args(["-lc", "env -0"]).output().await;
    let env_ok = match env {
        Ok(o) if o.status.success() => {
            let file = paths.instance_config_dir.join("env");
            std::fs::create_dir_all(&paths.instance_config_dir)
                .and_then(|_| std::fs::write(&file, capture_env(&o.stdout)))
                .and_then(|_| {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
                })
                .map_err(|e| e.to_string())
        }
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => Err(e.to_string()),
    };
    step("env", env_ok);

    let unit_dir = paths.home.join(".config/systemd/user");
    let units = std::fs::create_dir_all(&unit_dir)
        .and_then(|_| render_units().into_iter().try_for_each(|(name, body)| std::fs::write(unit_dir.join(name), body)))
        .map_err(|e| e.to_string());
    step("units", units);
    step("reload", systemctl(&["daemon-reload"]).await);
    let sock = format!("devbox@{i}.socket");
    let svc = format!("devbox@{i}.service");
    step("enable", systemctl(&["enable", &sock, &svc]).await);
    step("start", systemctl(&["restart", &svc]).await);

    let report = SetupReport { steps };
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        for s in &report.steps {
            println!("{} {} {}", if s.ok { "ok  " } else { "FAIL" }, s.id, s.message);
        }
    }
    if report.steps.iter().all(|s| s.ok) { Ok(()) } else { anyhow::bail!("setup failed") }
}
```

- [ ] **Step 4: 구현 — doctor와 `system.doctor`**

```rust
// crates/cli/src/doctor.rs (테스트 위)
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, ts_rs::TS)]
pub struct Check {
    pub id: String,
    pub status: Status,
    pub message: String,
    pub fix: Option<String>,
}

pub trait Probe {
    fn pid1(&self) -> String;
    fn linger(&self) -> String;
    fn tmux_version(&self) -> Option<String>;
    fn inotify_instances(&self) -> String;
    fn env_file(&self) -> Option<String>;
    fn which(&self, tool: &str, path: &str) -> bool;
}

pub fn tmux_at_least(v: &str, want: (u32, u32)) -> bool {
    let digits: String = v.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    let mut it = digits.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse::<u32>().unwrap_or(0));
    let (a, b) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
    (a, b) >= want
}

fn check(id: &str, status: Status, message: impl Into<String>, fix: Option<&str>) -> Check {
    Check { id: id.into(), status, message: message.into(), fix: fix.map(Into::into) }
}

pub fn checks(p: &dyn Probe) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(if p.pid1().trim() == "systemd" {
        check("systemd", Status::Ok, "systemd가 켜져 있습니다", None)
    } else {
        check("systemd", Status::Fail, "WSL에서 systemd가 꺼져 있습니다", Some("/etc/wsl.conf 에 [boot] systemd=true 를 넣고 wsl --shutdown"))
    });
    out.push(if p.linger().contains("Linger=yes") {
        check("linger", Status::Ok, "로그인 없이도 데몬이 유지됩니다", None)
    } else {
        check("linger", Status::Warn, "linger가 꺼져 있습니다(권장)", Some("앱 진단에서 [켜기] 또는 wsl.exe -u root loginctl enable-linger <user>"))
    });
    out.push(match p.tmux_version() {
        Some(v) if tmux_at_least(&v, (3, 2)) => check("tmux", Status::Ok, v, None),
        Some(v) => check("tmux", Status::Fail, format!("{v} — 3.2 이상이 필요합니다"), Some("sudo apt install tmux 또는 최신 tmux 설치")),
        None => check("tmux", Status::Fail, "tmux가 없습니다", Some("sudo apt install tmux")),
    });
    let inotify: u32 = p.inotify_instances().trim().parse().unwrap_or(0);
    out.push(if inotify >= 512 {
        check("inotify", Status::Ok, format!("max_user_instances={inotify}"), None)
    } else {
        check("inotify", Status::Warn, format!("max_user_instances={inotify} (여러 언어 서버를 쓰면 부족할 수 있음)"), Some("echo fs.inotify.max_user_instances=1024 | sudo tee /etc/sysctl.d/60-devbox.conf && sudo sysctl --system"))
    });
    match p.env_file() {
        None => out.push(check("env", Status::Fail, "사용자 환경 파일이 없습니다", Some("devbox setup"))),
        Some(text) => {
            let path = text.lines().find_map(|l| l.strip_prefix("PATH=\"")).map(|v| v.trim_end_matches('"').to_owned()).unwrap_or_default();
            out.push(check("env", Status::Ok, "사용자 환경을 잡아 두었습니다", None));
            for tool in ["git", "claude", "codex", "node", "pnpm"] {
                let found = p.which(tool, &path);
                out.push(check(
                    &format!("tool.{tool}"),
                    if found { Status::Ok } else if tool == "git" { Status::Fail } else { Status::Warn },
                    if found { format!("{tool} 사용 가능") } else { format!("{tool}를 찾지 못했습니다") },
                    (!found).then_some("설치한 뒤 앱 진단에서 [환경 다시 잡기]"),
                ));
            }
        }
    }
    out
}

pub struct OsProbe {
    pub env_file: std::path::PathBuf,
}

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    let o = std::process::Command::new(cmd).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

impl Probe for OsProbe {
    fn pid1(&self) -> String {
        std::fs::read_to_string("/proc/1/comm").unwrap_or_default()
    }
    fn linger(&self) -> String {
        let user = std::env::var("USER").unwrap_or_default();
        run_cmd("loginctl", &["show-user", &user, "-p", "Linger"]).unwrap_or_default()
    }
    fn tmux_version(&self) -> Option<String> {
        run_cmd("tmux", &["-V"])
    }
    fn inotify_instances(&self) -> String {
        std::fs::read_to_string("/proc/sys/fs/inotify/max_user_instances").unwrap_or_default()
    }
    fn env_file(&self) -> Option<String> {
        std::fs::read_to_string(&self.env_file).ok()
    }
    fn which(&self, tool: &str, path: &str) -> bool {
        path.split(':').any(|d| std::path::Path::new(d).join(tool).is_file())
    }
}

pub async fn run(json: bool) -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let list = checks(&OsProbe { env_file: paths.instance_config_dir.join("env") });
    if json {
        println!("{}", serde_json::to_string(&list)?);
    } else {
        for c in &list {
            println!("{:<5} {:<12} {}{}", format!("{:?}", c.status).to_lowercase(), c.id, c.message, c.fix.as_ref().map(|f| format!("  → {f}")).unwrap_or_default());
        }
    }
    Ok(())
}
```

`Check`·`Status`의 TS 타입이 필요하므로 이 둘을 `crates/protocol/src/system.rs`로 옮긴다. 그리고 `DoctorReport { checks: Vec<Check> }`와 `method!(pub Doctor = "system.doctor", query, PingParams => DoctorReport, SystemError);`를 추가한다. `doctor.rs`는 `use devbox_protocol::system::{Check, Status};`로 바꾼다. 라우터에 다음을 추가한다.

```rust
.add::<devbox_protocol::system::Doctor, _, _>(|s, _, _| async move {
    let probe = crate::doctor::OsProbe { env_file: s.paths.instance_config_dir.join("env") };
    let checks = tokio::task::spawn_blocking(move || crate::doctor::checks(&probe)).await.map_err(|e| devbox_protocol::Fail::Internal(e.to_string()))?;
    Ok(devbox_protocol::system::DoctorReport { checks })
})
```

`system::export`에 `x.method::<Doctor>();`를 추가한다.

- [ ] **Step 5: 통과·생성·실제 실행 확인·커밋**

```bash
cargo test -p devbox-cli && cargo run -q -p xtask -- gen-ts
cargo build -p devbox-cli && ./target/debug/devbox doctor || true
git add crates xtask app/src/rpc/gen && git commit -m "feat(cli): install systemd units, capture the user environment and run doctor checks"
```

`target`은 전역 target-dir일 수 있다. 바이너리는 `$(cargo metadata --format-version 1 | jq -r .target_directory)/debug/devbox`에서 찾는다.

- [ ] **Step 6: 실제 WSL에서 dev 인스턴스로 설치 시험(수동, 한 번)**

```bash
BIN=$(cargo metadata --format-version 1 | jq -r .target_directory)/debug/devbox
mkdir -p ~/.local/share/devbox/bin && cp "$BIN" ~/.local/share/devbox/bin/devbox
DEVBOX_INSTANCE=dev ~/.local/share/devbox/bin/devbox setup
systemctl --user status devbox@dev.service --no-pager | head -5
DEVBOX_INSTANCE=dev ~/.local/share/devbox/bin/devbox doctor
```

Expected:
- setup의 모든 단계가 `ok`이다.
- `active (running)`이다.
- doctor에 fail이 없다(warn은 허용).

S1 이후에도 dev 인스턴스를 그대로 개발용으로 쓴다.

- [ ] **Step 7: PR C**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo run -q -p xtask -- gen-ts --check && bash scripts/banned-words.sh
git push -u origin feat/daemon/core && gh pr create --fill --body "core(경로·설정·허브·DB·로그), projects 도메인, devbox 바이너리(daemon·bridge·dev-gateway·setup·doctor). 01-design §4·§5·§9.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

CI 통과 후 squash 머지, worktree 정리. 화면과 Windows 껍데기는 [03b-s0b-app](03b-s0b-app.md)으로 이어진다.
