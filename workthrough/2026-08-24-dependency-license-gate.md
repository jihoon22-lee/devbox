# Dependency, License, and Third-Party Notice Gate

## Overview

v0.5.0의 native-first·offline 우선 기능을 배포할 때 Cargo와 pnpm 의존성의 출처,
고정 버전, license, advisory, third-party 고지를 반복해서 수동 확인하지 않도록
저장소 수준의 검증 gate를 추가했다. Cargo graph에는 `cargo-deny 0.20.2` 정책을
적용하고, frontend graph에는 frozen install·license·integrity·audit 검사를 적용하며,
두 lockfile에서 결정적으로 `THIRD_PARTY_NOTICES.md`를 생성해 13개 Tauri 앱의
installer resource와 release asset에 연결한다.

이 문서는 `feat/workspace/dependency-gate` worktree의 현재 변경세트를 기록한다.
최종 집중 검토 기준 base는 `6d53ccbdeaf1d60933a261497bbdf67f414b42fb`
(`test(code-pad): synchronize LSP config save test (#368)`)이다. v0.4.2 hotfix와 안정판
배포 후 문서, 그리고 검토 중 발견한 Code Pad 테스트 race 수정까지 포함한 최신 `main`에
rebase했다. 이 변경세트에는 아직 PR 생성·원격 CI 실행·merge가 완료되지 않았다.

## Context

- v0.5.0 계획은 개발자가 인터넷에 연결되지 않은 환경에서도 설치 후 핵심 기능을
  사용할 수 있도록 native 구현과 작은 번들의 사용을 우선한다.
- 작은 library나 sidecar를 번들하는 경우에도 기능 구현만 검토하면 안 되고, 정확한
  source·version·digest·license·advisory·배포 고지를 함께 재현할 수 있어야 한다.
- Tauri 13개 앱과 10개 공용 Rust crate는 이 monorepo를 위해 배포되며, workspace
  crate를 crates.io에 실수로 publish하지 않도록 `publish = false`를 명시한다.
- release에는 installer뿐 아니라 portable 사용자가 확인할 수 있는 독립적인 notice
  asset도 필요하다. manifest가 실제 staging 파일의 이름·크기·SHA-256을 선언하고,
  verifier가 release asset을 다시 다운로드해 일치 여부를 확인하도록 경계를
  연결했다.
- 현재 graph에서 확인된 `h2 0.4.15`의 empty DATA frame memory-growth advisory
  (`RUSTSEC-2026-0258`)는 예외로 숨기지 않고 호환되는 `0.4.16`으로 갱신했다.

## Design Decisions

### 1. Cargo graph gate

`deny.toml`은 다음 두 target을 모두 대상으로 하고 모든 feature를 활성화한다.

```toml
[graph]
targets = [
  "x86_64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
]
all-features = true
```

검사 범위는 advisory, license, duplicate-version ban, registry/git source다. 외부
registry는 crates.io index만 허용하고 git dependency는 허용하지 않는다. multiple
version은 현재 Tauri·GTK·WebKit 및 앱 graph의 전이 의존성 때문에 `warn`으로 두되,
advisory·license·source 정책은 실패 기준으로 유지한다.

허용 Cargo license expression은 현재 대상 graph에서 실제 사용되는 다음 13개다.

```text
0BSD
Apache-2.0
Apache-2.0 WITH LLVM-exception
BSD-2-Clause
BSD-3-Clause
CC0-1.0
ISC
MIT
MIT-0
MPL-2.0
Unicode-3.0
Unlicense
Zlib
```

workspace package 자체는 `[licenses.private].ignore = true`로 third-party inventory
에서 제외한다. 이것은 devbox 자체의 license를 제3자 고지가 대신한다는 의미가
아니며, 공개 third-party package만 inventory에 남긴다.

### 2. pnpm graph gate

`.github/dependency-policy.json`이 pnpm license expression allowlist와 수동
clarification을 관리한다. CI는 `pnpm install --frozen-lockfile` 뒤에 전체 dependency
graph와 production graph를 각각 읽는다.

- 전체 graph: `pnpm licenses list --json` 기준 304 package version, 14개 reported
  license group
- production graph: `pnpm licenses list --prod --json` 기준 151 package version,
  9개 reported license group
- full allowlist는 `(MPL-2.0 OR Apache-2.0)`, `Apache-2.0`, `Apache-2.0 OR MIT`,
  `BSD-2-Clause`, `BSD-3-Clause`, `BlueOak-1.0.0`, `CC-BY-4.0`, `CC0-1.0`,
  `ISC`, `MIT`, `MIT OR Apache-2.0`, `MIT-0`, `Unlicense`다.
- package마다 lockfile의 integrity가 반드시 있어야 하며, 누락되거나 허용되지 않은
  expression은 실패한다.
- reported license가 `Unknown`이면 무조건 실패한다. 현재 유일한 `Unknown`은
  `khroma@2.1.0`이고, 아래의 exact clarification과 integrity가 일치할 때만
  upstream MIT license로 해석한다.

`pnpm audit --audit-level moderate`도 같은 dependency job에서 실행한다. audit
결과를 예외로 dismiss하는 별도 목록은 추가하지 않았다.

### 3. Exception is metadata, not a permanent ignore

`.github/dependency-policy.json`의 advisory exception은 다음 필드를 모두 요구한다.

```json
{
  "id": "RUSTSEC-or-GHSA-id",
  "package": "exact-package-name",
  "version": "exact-locked-version",
  "detector": "cargo-deny-or-dependabot",
  "scope": "affected-boundary",
  "expires": "2026-11-30",
  "reason": "current upstream constraint and removal plan"
}
```

총 17개 exception이 있으며 모두 만료일은 `2026-11-30`이다.

| Detector / advisory | Locked package(s) | Boundary and current reason |
|---|---|---|
| Dependabot `GHSA-wrw7-89jp-8q8g` | `glib@0.18.5` | Tauri GTK3 Linux runtime에만 존재하는 iterator unsoundness. 현재 Tauri 2.11.5 graph가 GTK3 0.18 line을 사용하며 Windows release에는 link되지 않으므로, Tauri GTK line 갱신 때 재검토한다. |
| cargo-deny `RUSTSEC-2024-0370` | `proc-macro-error@1.0.4` | `glib-macros 0.18`을 통한 Tauri Linux GTK build dependency이며 현재 GTK3 line 안에서 호환 대체가 없다. |
| cargo-deny `RUSTSEC-2024-0411`~`0420` | `gdkwayland-sys`, `gdk`, `atk`, `gdkx11-sys`, `gtk`, `atk-sys`, `gdkx11`, `gdk-sys`, `gtk3-macros`, `gtk-sys` — 모두 `0.18.2` | Tauri 2 Linux WebKit이 요구하는 archived GTK3 bindings다. Tauri가 GTK4 또는 유지되는 compatible line으로 이동할 때 제거한다. |
| cargo-deny `RUSTSEC-2025-0075`, `0080`, `0081`, `0098`, `0100` | `unic-char-range`, `unic-common`, `unic-char-property`, `unic-ucd-version`, `unic-ucd-ident` — 모두 `0.9.0` | `tauri-utils`의 `urlpattern 0.3`을 통한 rust-unic 전이 dependency다. 현재 Tauri가 허용하는 urlpattern line이 `0.6`이 아니므로 Tauri/urlpattern upgrade 때 제거한다. |

`deny.toml`의 RustSec ignore는 policy JSON에서 `detector = cargo-deny`인 ID와
정확히 같아야 한다. `check-dependencies.py`는 exception package/version이
`Cargo.lock`에 실제로 존재하는지, 날짜 형식이 ISO인지, 오늘 기준으로 만료되지
않았는지, `deny.toml`의 ID와 reason에 expiry가 들어 있는지, 두 목록의 ID set이
일치하는지를 검사한다. 따라서 package가 graph에서 사라지거나 exception만
날짜를 바꾸는 수정은 통과하지 않는다. 만료 연장은 최신 Tauri graph, Windows/Linux
영향, 대체 가능성, 제거 계획을 다시 검토하는 새 변경으로 처리해야 한다.

### 4. Manual license decisions

현재 자동 license 결과 중 별도 판단이 필요한 항목은 다음과 같이 문서화했다.

| Dependency / expression | Decision | Distribution obligation |
|---|---|---|
| `cssparser`, `selectors`, `dtoa-short`, `option-ext` 등 MPL-2.0 Rust crate | 허용. upstream source를 수정하지 않고 file-level copyleft 경계를 유지한다. | exact source와 digest를 notices에 남기며, 수정이 생기면 MPL source 제공 의무를 다시 검토한다. |
| `dompurify` — `(MPL-2.0 OR Apache-2.0)` | Apache-2.0 선택지로 허용 | notices에 expression, source, integrity를 보존한다. |
| `lru-cache` — BlueOak-1.0.0 | permissive license로 수동 허용 | license, source, integrity를 보존한다. |
| `caniuse-lite` — CC-BY-4.0 | browser support data attribution 조건으로 허용 | notices에 attribution과 source를 보존한다. |
| `khroma@2.1.0` — reported `Unknown` | exact upstream `v2.1.0` license 파일의 MIT와 lock integrity로만 clarification | package version 또는 integrity가 바뀌면 clarification mismatch로 실패하고 재검토한다. |

현재 locked graph에는 GPL, AGPL, SSPL, proprietary runtime 또는 git source가
없다. 이는 향후 dependency가 자동으로 허용된다는 뜻이 아니며, 새 runtime
dependency PR이 목적·대안·공식 출처·pin·license·크기·security·offline 동작·
maintenance owner를 별도로 기록해야 한다.

## Changes Made

### 1. Lockfile security patch and package publication boundary

- `Cargo.lock`
  - `h2`를 `0.4.15`에서 `0.4.16`으로 변경했다.
  - checksum을 `a9f37a958b41b3b19ee2707c06439c0e9e547e847223eb791ecb0cb821c65e27`로
    갱신했다.
  - `RUSTSEC-2026-0258`에 대한 exception은 만들지 않았다.
- 다음 13개 앱의 `src-tauri/Cargo.toml`에 `publish = false`를 추가했다.
  - `apps/api-playground/src-tauri/Cargo.toml`
  - `apps/code-pad/src-tauri/Cargo.toml`
  - `apps/devbox-manager/src-tauri/Cargo.toml`
  - `apps/developer-toolbox/src-tauri/Cargo.toml`
  - `apps/everything-plus/src-tauri/Cargo.toml`
  - `apps/knowledge-base/src-tauri/Cargo.toml`
  - `apps/life-log/src-tauri/Cargo.toml`
  - `apps/port-manager/src-tauri/Cargo.toml`
  - `apps/repo-manager/src-tauri/Cargo.toml`
  - `apps/run-manager/src-tauri/Cargo.toml`
  - `apps/webhook-lab/src-tauri/Cargo.toml`
  - `apps/workbench/src-tauri/Cargo.toml`
  - `apps/wsl-desktop/src-tauri/Cargo.toml`
- 다음 10개 공용 crate의 manifest에도 같은 경계를 추가했다.
  - `crates/applink/Cargo.toml`
  - `crates/filesystem/Cargo.toml`
  - `crates/git/Cargo.toml`
  - `crates/integration/Cargo.toml`
  - `crates/launch/Cargo.toml`
  - `crates/markdown/Cargo.toml`
  - `crates/process/Cargo.toml`
  - `crates/search/Cargo.toml`
  - `crates/secrets/Cargo.toml`
  - `crates/wsl/Cargo.toml`

이 변경으로 현재 23개 workspace Cargo package가 publish 대상이 아니라는 사실을
manifest 자체에서 확인할 수 있다. `repo-manager`, `webhook-lab`, `workbench`의
기존 trailing blank line도 해당 파일의 변경 중 정리되었지만 dependency 동작은
변경하지 않았다.

### 2. Cargo policy file

새 `deny.toml`은 다음을 고정한다.

- Windows MSVC와 Linux GNU target 및 `all-features = true`
- 위에서 설명한 16개 RustSec exception과 각 reason의 `2026-11-30` expiry
- 현재 대상 graph에서 실제 사용되는 13개 Cargo license allowlist와 private workspace ignore
- `multiple-versions = "warn"`, `wildcards = "allow"`, `highlight = "all"`
- workspace/external default feature 허용 설정
- crates.io index만 허용하는 registry source 정책과 empty git allowlist

multiple-version warning은 현재 graph의 bitflags, block-buffer, cpufeatures,
crypto-common, winreg 등 전이 dependency 차이를 보고하기 위한 것이며, local
`cargo deny --locked check`를 실패시키지는 않는다. advisory, ban, license,
source 결과는 별도의 성공 기준으로 유지된다.

### 3. Machine-readable pnpm policy

새 `.github/dependency-policy.json`은 다음을 source of truth로 제공한다.

- `schemaVersion: 1`
- 13개 허용 pnpm license expression
- `khroma@2.1.0`의 `Unknown → MIT` clarification
- detector, package, exact version, scope, expiry, reason을 포함한 17개 advisory
  exception

khroma clarification에는 다음 exact upstream license URL과 lock integrity를 함께
기록했다.

```text
https://github.com/fabiospampinato/khroma/blob/v2.1.0/license
sha512-Ls993zuzfayK269Svk9hzpeGUKob/sIgZzyHYdjQoAdQetRKpOLj+k/QQQ/6Qi0Yz65mlROrfd+Ev+1+7dz9Kw==
```

### 4. Deterministic dependency checker and notices generator

새 `.github/scripts/check-dependencies.py`는 `check`와 `generate` 두 mode를
제공한다. 핵심 흐름은 다음과 같다.

```python
policy = load_policy()
cargo_packages = cargo_lock_packages()
validate_exceptions(policy, cargo_packages)
integrities = parse_pnpm_integrities()

flatten_pnpm_licenses(
    run_json(["pnpm", "licenses", "list", "--json"]), policy, integrities
)
runtime_rows = flatten_pnpm_licenses(
    run_json(["pnpm", "licenses", "list", "--prod", "--json"]),
    policy,
    integrities,
)
notices = render_notices(rust_rows(cargo_packages), runtime_rows)
```

구체적인 검사 계약은 다음과 같다.

1. `Cargo.lock`은 `tomllib`으로 읽고 모든 exception의 exact package/version이
   locked graph에 있는지 확인한다.
2. `deny.toml`의 ignore ID는 policy JSON의 cargo-deny ID와 일치해야 하며 reason에
   같은 expiry date가 포함되어야 한다.
3. pnpm lockfile의 package key와 integrity를 파싱해 license inventory의 모든
   version과 1:1로 맞춘다. Unknown은 clarification이 있을 때만 허용한다.
4. `cargo metadata --locked --format-version 1`에서 외부 package의 license와
   Cargo.lock checksum을 읽고 `sha256:<lock checksum>` 형태의 digest를 만든다.
5. Rust rows와 frontend runtime rows를 package name/version 순으로 정렬하고,
   `Cargo.lock`과 `pnpm-lock.yaml` 자체의 SHA-256도 notice header에 기록한다.
6. `check` mode는 checked-in notice가 재생성 결과와 byte-for-byte로 같은지 확인하며,
   `generate` mode만 `THIRD_PARTY_NOTICES.md`를 갱신한다.

생성된 `THIRD_PARTY_NOTICES.md`의 현재 상태:

- 크기: 128,169 bytes
- 줄 수: 801 lines
- Rust external package rows: 629
- frontend runtime package rows: 151
- `Cargo.lock` SHA-256: `656bbab2ed7e4dfe28d88857224d02e8498ce0dc851a6aa7f17109b56f0486e1`
- `pnpm-lock.yaml` SHA-256: `ed799976f87b5dacc7ccdbce35b3d7394699d50cf4b2e47f739b922cafd51f65`

notice 파일의 시작 부분은 다음 생성 계약을 명시한다.

```markdown
# Third-Party Notices

This inventory is generated from the locked devbox dependency graph. It does not grant a
license for devbox itself; workspace packages are private and excluded from this third-party
inventory. Regenerate it with `.github/scripts/check-dependencies.py generate`.
```

frontend notice table은 production runtime만 담고 build-only package는 CI license
gate로 계속 검사한다. 따라서 notice의 151행과 full license graph의 304 version은
서로 다른 목적의 숫자이며, full graph를 누락한 것으로 해석하지 않는다.

### 5. Regression scripts

두 개의 focused regression script를 추가했다.

- `.github/scripts/test-check-dependencies.py`
  - 현재 policy와 lock을 정상적으로 검증한다.
  - `date.today()`를 2026-12-01로 대체해 만료 exception이 fail-closed 되는지
    확인한다.
  - `khroma@2.1.0`의 exact integrity가 현재 clarification과 일치하는지 고정한다.
- `.github/scripts/test-build-manifest.py`
  - 임시 staging에 demo app의 portable/installer와 notice 파일을 만들고
    `build-manifest.py`가 `schemaVersion: 1`, app asset digest, notice name/size/
    digest를 생성하는지 확인한다.
  - notice를 삭제한 뒤 `THIRD_PARTY_NOTICES.md is missing` 오류와 non-zero exit를
    확인한다.

`.github/scripts/check-catalog.sh`에도 release app의
`bundle.resources`에 `../../../THIRD_PARTY_NOTICES.md`가 반드시 있어야 한다는
검사를 추가했다.

### 6. Tauri bundle resources

다음 13개 `tauri.conf.json`의 `bundle.resources`에 같은 source를 추가했다.

```json
"resources": ["../../../THIRD_PARTY_NOTICES.md"]
```

적용 파일:

- `apps/api-playground/src-tauri/tauri.conf.json`
- `apps/code-pad/src-tauri/tauri.conf.json`
- `apps/devbox-manager/src-tauri/tauri.conf.json`
- `apps/developer-toolbox/src-tauri/tauri.conf.json`
- `apps/everything-plus/src-tauri/tauri.conf.json`
- `apps/knowledge-base/src-tauri/tauri.conf.json`
- `apps/life-log/src-tauri/tauri.conf.json`
- `apps/port-manager/src-tauri/tauri.conf.json`
- `apps/repo-manager/src-tauri/tauri.conf.json`
- `apps/run-manager/src-tauri/tauri.conf.json`
- `apps/webhook-lab/src-tauri/tauri.conf.json`
- `apps/workbench/src-tauri/tauri.conf.json`
- `apps/wsl-desktop/src-tauri/tauri.conf.json`

installer resource는 같은 checked-in notice를 참조하므로 별도 runtime network
dependency나 executable을 추가하지 않는다. portable 사용자는 release의 독립 notice
asset으로 같은 고지 파일을 받을 수 있다.

### 7. Release manifest and workflow

`.github/scripts/build-manifest.py`는 app별 portable/installer를 기존처럼 실제
staging 파일에서 계산한 뒤, 이제 staging root의 notice 파일을 필수로 확인한다.
manifest schemaVersion은 1을 유지하고 backward-compatible optional `notices` 필드를
추가한다.

```json
"notices": {
  "name": "THIRD_PARTY_NOTICES.md",
  "sha256": "<staging file SHA-256>",
  "size": 128169
}
```

`.github/scripts/verify-release.py`는 manifest에 `notices`가 있을 때 이를 expected
asset map에 넣고, release asset 존재·size·SHA-256을 다른 app asset과 같은 방식으로
검증한다. manifest에 선언되지 않은 release asset도 계속 실패시키며,
`release-manifest.json` 자신만 예외다.

`.github/workflows/release.yml`의 흐름은 다음과 같다.

1. Windows build job이 앱별 staging을 만든다.
2. `Stage third-party notices` step이 root `THIRD_PARTY_NOTICES.md`를
   `staging/THIRD_PARTY_NOTICES.md`로 복사한다.
3. Windows artifact에 staging 전체를 업로드한다.
4. staging을 대상으로 release manifest를 생성하고 별도 artifact로 업로드한다.
5. publish job이 `artifacts/staging/THIRD_PARTY_NOTICES.md`와
   `release-manifest.json`을 executable들과 함께 GitHub release에 게시한다.
6. verify job이 GitHub API에서 실제 asset을 내려받아 manifest의 size·SHA-256과
   비교한다.

v0.5.0 release 계획에서 15개 앱으로 확대될 경우 기대 asset은 30 binaries + notice
asset + manifest다. 현재 이 변경세트의 catalog/resource 검사는 현재 13개 앱 전체에
대해 적용되어 있다.

### 8. CI dependency job

`.github/workflows/ci.yml`에 `Dependency policy` job을 추가했다.

```yaml
- name: Install frontend dependencies
  run: pnpm install --frozen-lockfile

- name: Audit frontend dependencies
  run: pnpm audit --audit-level moderate

- name: Check pnpm policy and notices
  run: |
    python3 .github/scripts/check-dependencies.py check
    python3 .github/scripts/test-check-dependencies.py
    python3 .github/scripts/test-build-manifest.py

- name: Check Cargo licenses, advisories, bans, and sources
  uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25 # v2, cargo-deny 0.20.2
  with:
    rust-version: "1.98.0"
    arguments: --locked
    command: check
```

action은 floating tag가 아니라 `EmbarkStudios/cargo-deny-action`의
`3c6349835b2b7b196a839186cb8b78e02f7b5f25` commit으로 pin했다. CI job은 기존
catalog/frontend/Rust job과 별도로 dependency policy 실패를 구분해 보여준다.

### 9. Project documentation updates

- `docs/dependency-policy.md`
  - Cargo/pnpm graph, exception lifecycle, manual license decisions, notices,
    distribution, dependency PR evidence와 검증 명령을 source of truth로 설명한다.
- `README.md`
  - 의존성·제3자 고지 정책 문서 링크를 문서 table에 추가했다.
- `CONVENTIONS.md`
  - 새 dependency의 evidence 요구와 `deny.toml`/policy/notices/resource 규칙을
    공통 개발 규약에 추가했다.
- `docs/product-opportunities.md`
  - release manifest 예시에 optional `notices`를 추가하고, release checklist,
    notices asset publish, installer resource 검증 항목을 반영했다.

## Verification Results

검증은 이 worktree의 현재 파일을 대상으로 수행했다. 로컬 명령 결과는 다음과 같다.

### Dependency policy and release scripts

```text
cargo deny 0.20.2
python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

bash .github/scripts/check-catalog.sh
exit code: 0

pnpm audit --audit-level moderate
No known vulnerabilities found
```

`cargo deny --locked check`도 exit code 0으로 끝났으며 마지막 결과는 다음과 같다.

```text
advisories ok, bans ok, licenses ok, sources ok
```

duplicate-version warning은 `deny.toml`의 `multiple-versions = "warn"` 계약에 따른
warning이며, dependency gate 실패가 아니다.

### Rust and frontend build/test checks

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets -- -D warnings PASS
cargo test --workspace --all-targets                PASS
cargo check --workspace                             PASS
pnpm -r --workspace-concurrency=1 test              PASS
pnpm -r --workspace-concurrency=1 build             PASS
```

Rust workspace test output의 모든 test result가 `ok`이고 failed test는 0이었다.
API Playground Rust suite에는 현재 16개 테스트가 포함되어 있으며, redirect/body
suppression·sensitive destination block·persistence wire shape·generic network
error를 포함한 회귀가 통과했다.

frontend 검증은 host resource contention을 피하기 위해 workspace concurrency 1로
수행했다. 첫 preflight에서 Code Pad의 Apply 직후 Save 테스트 race를 재현했고 focused
실행에서도 확인했으므로 환경 실패로 숨기지 않았다. test-only PR #368을 분리해 먼저
merge한 뒤 이 branch를 새 base로 rebase하고 전체 실행을 처음부터 다시 수행했다.
최종 serial test는 13개 앱의 288개 테스트가 모두 통과했으며 실패는 0이었다. Code Pad는
13 files / 87 tests가 같은 전수 실행 안에서 통과했다. Everything Plus는 frontend test
file이 없어 `--passWithNoTests` 계약으로 성공했다. serial build도 TypeScript compilation을
포함해 전체 13개 앱에서 통과했다. Vite가 일부 큰 chunk에 대해 500 kB 초과 warning을
출력했지만 build exit는 0이었고 이 gate는 새 runtime frontend 코드를 추가하지 않는다.

마지막 변경 전 dependency gate diff에 대해 `git diff --check`도 통과했다. 이
workthrough 파일을 추가한 뒤에는 아래 명령을 다시 수행해 문서 자체의 trailing
whitespace까지 확인한다.

```bash
git diff --check
git status --short
```

### Scope and status verification

- `THIRD_PARTY_NOTICES.md` resource를 포함한 Tauri app 수: 13
- publish=false가 추가된 app manifest 수: 13
- publish=false가 추가된 common crate manifest 수: 10
- Cargo external notice row 수: 629
- pnpm production notice row 수: 151
- advisory exception 수: 17
- 모든 exception expiry: `2026-11-30`
- review base: `6d53ccbdeaf1d60933a261497bbdf67f414b42fb`

### Root-agent concentrated review findings

PR 직전 전체 변경세트를 다시 읽고 정책 판정 로직, release staging/manifest/verifier,
CI 실행 순서, 23개 Cargo package의 publication boundary, 13개 Tauri resource를 함께
대조했다. 이 검토에서 다음 경계를 보강했다.

1. 문서가 “만료 당일부터 merge 불가”라고 규정하므로 exception 검사를
   `expires <= today`로 맞췄고, 정확히 `2026-11-30`에 실패하는 회귀 테스트로 고정했다.
2. `Unknown` license clarification의 `acceptedLicense`도 pnpm allowlist 안에 있어야
   하며, 필수 metadata 누락·중복·현재 graph에서 사용되지 않는 stale clarification을
   모두 거부하도록 fail-closed 검사를 추가했다.
3. 회귀 스크립트가 import 과정에서 저장소 안에 `__pycache__`를 남기지 않도록
   bytecode 생성을 비활성화하고, 실제 생성됐던 임시 bytecode는 삭제했다.
4. `h2` 역방향 dependency tree가 0.4.16 하나만 사용함을 확인했고, GitHub의 열린
   Dependabot alert는 policy에 기록한 Linux GTK `glib` 중간 등급 1건뿐임을 API로
   독립 확인했다.
5. 전체 frontend preflight 중 기존 Code Pad UI 테스트 race를 재현했다. 의존성 PR에
   섞지 않고 test-only PR #368로 분리해 반복 검증·CI 통과 후 먼저 merge했으며, 이
   branch를 해당 merge commit 위로 다시 rebase했다.
6. cargo-deny가 현재 target graph에서 만나지 않은 `BSL-1.0`과
   `CDLA-Permissive-2.0` allow를 경고했다. 미래 의존성이 검토 없이 통과하지 않도록 두
   항목을 제거하고 현재 실제 사용되는 13개 expression으로 allowlist를 최소화했다.

## Next Steps

1. 집중 검토와 최신 base 전수 검증을 통과한 현재 변경세트로 기능 단위 PR을 만들고,
   GitHub Actions required CI가 모두 성공한 뒤에만 merge한다. 현재 문서 시점에는
   dependency-gate PR·원격 CI·merge 완료 사실을 기록하지 않는다.
2. 이후 v0.5.0에서 새 runtime dependency나 sidecar를 추가할 때
   `docs/dependency-policy.md`의 dependency evidence table을 PR body 또는
   workthrough에 채운다. 목적·native/offline 가치·대안·공식 source·pin·license·
   크기·security·offline fallback·maintenance owner가 없는 dependency PR은
   merge하지 않는다.
3. `2026-11-30` 전에 17개 upstream exception을 최신 Tauri graph와 함께 다시
   검토하고, 제거 가능하면 exception과 deny ignore를 함께 삭제한다. 만료일만
   연장하지 않는다.
