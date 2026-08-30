# Launcher Typed Sources and Revision-bound Actions (#487)

## Overview

Devbox Launcher의 미구현 동적 source였던 Workbench profile, Repo Manager repository/worktree,
WSL Desktop profile producer를 실제 named snapshot으로 연결했다. 기존 Run Manager와 Everything+
source도 같은 consumer 계약 아래 정리하고, 검색에서 본 exact catalog/snapshot entry revision을
preview·favorite·launch 직전에 다시 확인한다.

Launcher 자체 저장소에는 bounded opaque result ID만 남는다. label, raw path/query/payload,
clipboard text와 secret은 favorites/recency 파일에 저장하지 않는다. source 하나가 없거나 오래됐거나
손상됐거나 권한이 없거나 link/reparse 경로여도 다른 source와 catalog 검색은 계속된다.

```text
producer-owned store / scan result
            │
            ├─ Workbench ───── profiles.json
            ├─ Repo Manager ── repositories.json
            ├─ Run Manager ─── jobs-services.json (legacy summary fallback)
            ├─ Everything+ ─── summary.json / saved-queries
            └─ WSL Desktop ─── profiles.json
                              │
                    bounded + no-follow read
                              │
                   strict typed entry validation
                              │
              opaque result ID + SHA-256 revision
                              │
          preview/favorite/launch command revalidation
                              │
              target app validates typed AppLink again
```

## Context

Launcher bootstrap은 다섯 source의 consumer shape를 이미 알았지만, Workbench·Repo Manager·WSL
Desktop은 실제 producer를 아직 발행하지 않았다. 또한 renderer가 검색 결과 ID만 다시 보내 실행할
때, 같은 ID의 payload가 검색 뒤 바뀌었는지 구분할 exact revision이 없었다. favorites/recency도
아직 없었고 source 진단은 symbolic link/reparse point를 별도 이유로 보여주지 않았다.

이 작업은 #487의 한 사용자 흐름으로 묶었다. producer의 primary CRUD/scan 동작은 각 앱이 계속
소유하고, snapshot publication 실패는 그 결과를 뒤집지 않는다. Launcher는 producer DB나 private
schema를 직접 열지 않는다.

## Scope

### Included

- Workbench `profiles.json` named producer
- Repo Manager `repositories.json` scan/worktree producer
- WSL Desktop `profiles.json` named producer
- Run Manager named sidecar 우선 및 flat summary fallback 유지
- Everything+ saved-query strict payload 소비 유지
- exact entry SHA-256 revision과 command-bound revalidation
- stale result의 명시적 확인과 현재 source 재검증
- opaque-ID-only favorites/recency, bounded atomic preference store
- missing/stale/corrupt/permission/linked source 진단
- producer remove/rename/change와 stale selection 회귀 테스트
- catalog revision 14 및 producer capability 문서화

### Excluded

- producer source DB를 Launcher가 직접 읽는 방식
- raw path/query/payload/label 또는 clipboard history 저장
- stale snapshot 자동 실행
- arbitrary command/argv 실행
- 앱별 version bump와 v0.6.0 release tag/publication
- Windows packaged physical acceptance 완료 주장

## Changes Made

### 1. Three producer-owned named snapshots

Workbench는 유효한 profile store의 opaque profile ID, 안전한 label, 고정 detail과 `{id}` payload만
`workbench/v1/profiles.json`에 발행한다. project path, Git root, WSL metadata와 Run Manager service
ID는 projection에서 제외한다. list/create/update/delete의 primary 결과 뒤 best-effort로 갱신한다.

WSL Desktop도 profile ID와 안전한 표시 정보만 `wsl-desktop/v1/profiles.json`에 발행한다. distro,
cwd, start command, tab/pane layout은 snapshot에 들어가지 않는다. 기존 profile store가 corrupt하면
빈 writable store로 바꾸지 않고 mutation과 publication을 거부해 원본과 last-good snapshot을
보존한다.

Repo Manager는 완성된 scan 결과로 process-local bounded map을 교체하고, 성공한 worktree 조회의
repository를 canonical identity 기준으로 합친다. canonical key는 namespace-separated SHA-256
opaque ID로만 노출하고, 안전한 basename label·고정 detail·검증된 absolute path payload만
`repo-manager/v1/repositories.json`에 넣는다. branch, status, Git 출력과 canonical key 원문은
복제하지 않는다.

### 2. Exact revision and stale action boundary

catalog 결과는 embedded catalog 전체 bytes에, snapshot 결과는 producer/view와 exact entry JSON에
length-delimited SHA-256을 계산한다. lower-case 64자리 revision은 검색 결과에 포함되지만 저장하지
않는다. renderer의 launch/preview/favorite 요청은 opaque result ID와 expected revision을 함께 보내며,
native command가 현재 catalog와 source를 새로 읽어 ID·revision·payload kind를 다시 확인한다.

rename/change/remove된 entry는 이전 selection으로 실행되지 않는다. stale entry는 ordinary launch에서
거부되고, 사용자가 확인 modal의 `계속 열기`를 선택한 요청에만 `allowStale=true`를 전달한다. 그
경우에도 같은 command에서 현재 source와 exact revision을 다시 확인한다.

### 3. Privacy-safe preferences

`launcher-preferences.json`은 version 1, 최대 64 KiB이며 favorite 64개와 recent 64개의 opaque result
ID만 보관한다. ID는 256 bytes와 제한된 ASCII 문자 집합을 적용하고 credential-shaped value,
duplicate, unknown JSON field, unsupported version과 over-bound 파일을 거부한다. corrupt 파일은
검색 정렬만 neutral fallback으로 낮추고 write에서 덮어쓰지 않는다.

저장은 app-local directory 안에서 atomic replace한다. recent는 실제 앱 launch 또는 text handoff가
성공한 뒤에만 기록하고, favorite는 현재 ID/revision을 검증한 뒤 변경한다. 검색 match score가 우선이고
동일 score 안에서 favorite, recent 순서를 적용한다.

### 4. No-follow source reads and diagnostics

Launcher는 다섯 source의 full path와 모든 ancestor에 symbolic link/reparse point가 없는지 확인한다.
공용 integration reader는 exact no-follow handle로 bounded bytes를 읽고, 읽은 뒤 path가 같은
filesystem identity를 가리키며 ancestor가 계속 plain인지 재검증한다. Launcher도 진단용 handle과
소비 완료 시점 identity를 다시 비교한다. 교체 경쟁이나 unsafe path는 payload 실행으로 넘어가지
않는다.

UI는 `fresh`, `stale`, `missing`, `corrupt`, `permission`, `linked`를 서로 다른 한국어 label과
설명으로 표시한다. unsafe source 하나는 빈 결과로 격리되고 다른 source와 catalog는 유지된다.

### 5. Catalog, UI and documentation

`apps/catalog.json`을 revision 14로 올리고 Workbench profiles, Repo Manager repositories, WSL
Desktop profiles capability를 선언했다. Launcher result에는 favorite/recent badge를 표시하고,
listbox option과 분리된 footer action으로 현재 선택을 즐겨찾기하거나 recent history를 지운다.
실행 성공 뒤 hidden persistent window의 현재 검색을 best-effort refresh해 다음 호출의 ordering을
갱신한다.

앱 README와 architecture/roadmap에는 named sidecar ownership, 최소 공개 데이터, revision/stale
경계와 아직 남은 Windows packaged acceptance를 구분해 기록했다.

## Verification Results

WSL에서 실행 가능한 source/core, frontend와 repository policy gate를 완료했다. Rust 병렬도는
리소스 사용을 제한하기 위해 `-j2`를 사용했다.

```text
cargo test --workspace --all-targets -j2 --quiet                         PASS
cargo check --workspace -j2                                              PASS
cargo clippy --workspace --all-targets -j2 -- -D warnings                PASS
cargo fmt --all -- --check                                               PASS
pnpm install --frozen-lockfile                                           PASS
pnpm build                                                               PASS
pnpm test                                                                PASS
bash .github/scripts/run-frontend-scope.sh typecheck all ''              PASS
bash .github/scripts/check-catalog.sh                                    PASS
python3 .github/scripts/check-dependencies.py check                      PASS
python3 .github/scripts/test-check-dependencies.py                       PASS
python3 .github/scripts/test-build-manifest.py                           PASS
python3 .github/scripts/test-validate-release-input.py                   PASS
pnpm audit --audit-level moderate                                        PASS (0 known vulnerabilities)
cargo deny --locked check                                                PASS (policy-allowed warnings only)
git diff --check                                                         PASS
```

Targeted suites additionally covered Launcher 29 Rust/12 frontend tests, Repo Manager 128 Rust
tests, Workbench 129 Rust tests, WSL Desktop 102 Rust tests, catalog 11 tests and integration 19
tests. Full workspace Rust finished with no failures; one pre-existing ignored test remained ignored.

Windows compile behavior is delegated to the required PR CI Windows job. Packaged install, global
shortcut, NTFS reparse/ACL and actual cross-app handoff physical checks remain release acceptance
work and are not represented as completed here.

## Integration Status

The implementation belongs to the grouped #487 PR because all four touched apps and the shared
integration reader form one Launcher source-discovery and execution-revalidation flow. App version
bumps are intentionally deferred to the v0.6.0 release-preparation issue so each package receives its
final minor or patch version once, after all milestone feature work is merged.
