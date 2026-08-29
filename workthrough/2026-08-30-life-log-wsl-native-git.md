# Life Log WSL-native Git projects

## Overview

Life Log이 Windows absolute path와 함께 WSL UNC project path를 안전하게 저장·중복 제거하고,
해당 WSL 배포판 내부의 Git으로 day/week/month digest와 export를 수집하도록 보강했다.
Settings 저장은 subprocess를 시작하지 않으며, 사용자가 누르는 `연결 확인`만 bounded read-only
probe를 수행한다. 한 저장소의 실패는 다른 source와 정상 저장소의 digest를 막지 않는다.

## Context

프로젝트가 `/mnt/e/projects`에서 `/home/jihoon/projects`로 이동한 뒤 Windows 앱에
`\\wsl$\Ubuntu\home\jihoon\projects\...` 또는 `//wsl$/Ubuntu/home/...`를 등록하면
기존 native Git runner가 UNC path를 Windows Git에 그대로 전달했다. 그 결과 설정값 자체는
absolute path로 보였지만 digest의 Git source를 읽을 수 없었고, 여러 digest/error whitelist가
서로 달라 새 오류가 전체 응답 실패로 번질 여지도 있었다.

## Changes

### Shared WSL path and Git boundary

- `crates/wsl/src/path.rs`에 `WslUncPath` parser를 추가했다. `wsl$`와 `wsl.localhost`, slash
  표기, distro 대소문자는 같은 identity로 정규화하지만 Linux path tail의 대소문자는
  보존한다. missing distro/tail, traversal, control character, oversize input은 거부한다.
- `crates/filesystem/src/project_path.rs`가 같은 parser를 사용해 WSL alias를 중복 제거하면서
  Linux case-sensitive path를 서로 다른 project로 유지한다.
- `crates/git/src/lib.rs`에 `GitTarget::Native`와 `GitTarget::Wsl`을 추가했다. WSL target은
  shell 없이 fixed argv로 `wsl.exe -d <distro> -- /usr/bin/timeout ... /usr/bin/env ... --
  git -C <linux-path> ...`를 호출하고, 기존 outer timeout 안에 distro 내부 timeout을 둔다.
- stdin은 닫고 Git override/interactive 환경은 제거하며 stderr와 submitted path를 사용자
  오류에 반향하지 않는다. WSL 경계는 `git_wsl_unavailable`, `git_wsl_failed`, 기존 bounded
  runner 경계는 `git_timeout`, `git_output_too_large` 같은 stable code로 반환한다.

핵심 target 분기는 다음 형태다.

```rust
match GitTarget::from_project_path(path)? {
    GitTarget::Native { cwd } => run_native_bounded(cwd, args, limits, cancellation),
    GitTarget::Wsl { distro, cwd } => {
        run_wsl_bounded(distro, cwd, args, limits, cancellation)
    }
}
```

### Life Log settings and digest

- project setting은 backend에서 최대 개수/경로/총 byte와 safe identity를 한 번에 검증하고,
  원자 저장에 성공한 authoritative normalized list만 frontend에 반환한다.
- frontend는 저장 완료 전 optimistic state를 확정하지 않는다. 실패 시 마지막 확인된 목록을
  유지하고, 이전 settings request가 늦게 도착해 방금 저장한 목록을 덮어쓰지 않도록 request
  lifecycle을 view/date digest lifecycle과 분리했다.
- Settings에 명시적 `연결 확인`을 추가했다. repository 여부와 Native/WSL target만 안전하게
  표시하고 raw Git/OS 오류는 노출하지 않는다.
- export와 day/week/month digest가 모두 target-aware runner를 사용한다. source error contract를
  한 모듈로 통합해 WSL 또는 process-tree 오류가 partial response 전체를 무효화하지 않는다.
- README에 허용되는 WSL UNC 표기, identity, fixed argv, probe 및 partial-failure 계약을 기록했다.

### Regression findings fixed while testing

- 현재 날짜가 일요일일 때 week chart test가 다음 주 날짜를 잘못 선택할 수 있던 fixture를
  실제 `weekRange` 안의 날짜를 고르도록 수정했다.
- Settings 전환 중 오래된 비동기 settings/history 응답이 확정된 저장 또는 최신 refresh를
  덮어쓸 수 있는 경쟁 조건을 재현하고 request ordering regression으로 고정했다.

## Files changed

- `crates/wsl/src/path.rs` — strict WSL UNC parser and canonical identity
- `crates/filesystem/{Cargo.toml,src/project_path.rs}` — shared WSL identity integration
- `crates/git/{Cargo.toml,src/lib.rs}` — target-aware bounded Git runner
- `apps/life-log/src-tauri/src/core/{aggregate,digest,error_codes,export,handoff,mod}.rs` — WSL
  collection, validation, and one stable error contract
- `apps/life-log/src-tauri/src/commands/life.rs`, `src-tauri/src/lib.rs` — authoritative settings
  save and explicit project probe command
- `apps/life-log/src/{App.tsx,App.css,api.ts,App.contextMenu.test.tsx}` — settings UX, safe status,
  stale-response protection, and regressions
- `apps/life-log/README.md` — user and security contract
- `Cargo.lock` — local workspace dependency edges only

## Verification

The following checks passed in the dedicated worktree:

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-life-log-wsl \
  cargo test -p wsl -p filesystem -p git -p life-log -j2
  PASS (wsl 33, filesystem 18, git 16, life-log 104; 171 unit tests plus doc-tests)
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-life-log-wsl \
  cargo check -p life-log -p git -p filesystem -p wsl -j2
  PASS
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-life-log-wsl \
  cargo clippy -p wsl -p filesystem -p git -p life-log --all-targets -j2 -- -D warnings
  PASS
cargo fmt --all -- --check
  PASS
git diff --check
  PASS
pnpm --filter life-log test
  PASS (4 files, 51 tests)
pnpm --filter life-log build
  PASS (TypeScript and Vite production build)
```

The exact fixed-argv WSL Git probe also returned `true` for every configured migrated repository:

```text
NaverBlogAutomation, PointBook, SoolJang, devbox, ici, idk,
FamilyCard, family-care, wsl-resource-guard                    PASS (9/9)
```

The WSL probe used `/usr/bin/timeout` with seconds syntax accepted by the installed Ubuntu GNU
coreutils (`0.1s` and `1.900s`). An initial `100ms` spelling was rejected by the live tool and was
corrected before the verification above.

The first parallel Clippy attempt exposed that the user-level Cargo config points every worktree at
one shared target directory. A concurrently compiled branch could therefore supply a stale artifact
for the same local package name/version. Final evidence excludes that attempt and uses the dedicated
Linux-native target directory shown above; other concurrent worktrees use separate targets as well.

GitHub Actions and Windows packaged acceptance remain separate PR/release gates and are not claimed
by this local workthrough.
