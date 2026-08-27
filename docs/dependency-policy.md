# Dependency and Third-Party Notice Policy

devbox의 P1·P2 native 기능은 설치 뒤 오프라인에서 동작해야 한다. 라이브러리를 번들하는
경우 기능 구현과 같은 수준으로 출처·고정 버전·라이선스·보안·배포 고지를 관리한다. 이
문서는 `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` §1.3과 P1-01을
실행 가능한 gate로 구체화한다.

## Enforced gates

| Gate | Source of truth | CI behavior |
|---|---|---|
| Cargo graph | `Cargo.lock`, `deny.toml` | Windows/Linux target의 license, advisory, duplicate ban, registry/git source를 `--locked`로 검사 |
| pnpm graph | `pnpm-lock.yaml`, `.github/dependency-policy.json` | frozen install 뒤 full transitive license와 audit를 검사; 미허용 표현·integrity 부재·미등록 `Unknown`을 거부 |
| Exceptions | `.github/dependency-policy.json` | ID, package, exact locked version, detector, scope, reason, ISO date expiry를 모두 요구; 만료 당일부터 merge 불가 |
| Notices | 두 lockfile + package metadata | 666 Rust package와 157 frontend runtime package의 version/license/source/digest를 결정적으로 재생성해 checked-in 파일과 byte 비교 |
| Distribution | `tauri.conf.json`, release manifest | 모든 release 앱 installer에 notices resource를 넣고, release에는 notices와 그 size/SHA-256을 manifest-declared asset으로 게시 |

현재 `THIRD_PARTY_NOTICES.md`는 135,107 bytes다. installer에서는 동일 파일을 압축 resource로
포함하므로 새 executable runtime이나 network dependency를 추가하지 않는다. portable 사용자는
release의 독립 notice asset을 받을 수 있다. release manifest는 schemaVersion 1을 유지하고
optional `notices` 필드를 추가하므로 기존 Devbox Manager parser와 호환된다. v0.5.0 release의
기대 asset은 15개 앱 기준 30 binaries + notices + manifest다.

## Current decisions

`h2 0.4.15`의 empty DATA frame memory-growth advisory(RUSTSEC-2026-0258)는 호환되는
0.4.16으로 즉시 갱신했다. 이 취약점에는 예외를 만들지 않는다.

다음 license는 자동 허용 후보 외의 수동 검토 결과다.

| Dependency | Decision | Distribution obligation |
|---|---|---|
| `cssparser`, `selectors`, `dtoa-short`, `option-ext` 등 MPL-2.0 Rust crates | 허용. devbox가 upstream source를 수정하지 않고 파일 단위 copyleft 경계를 유지한다. | exact crate source와 digest를 notices에 남기고, 수정 시 MPL source 제공 의무를 다시 검토 |
| `dompurify` `(MPL-2.0 OR Apache-2.0)` | Apache-2.0 선택지로 허용 | notices에 expression/source/integrity 유지 |
| `lru-cache` BlueOak-1.0.0 | permissive license로 수동 허용 | license/source/integrity 유지 |
| `caniuse-lite` CC-BY-4.0 | browser support data attribution 조건으로 허용 | notices attribution과 source 유지 |
| `khroma 2.1.0` | package metadata의 `Unknown`을 exact upstream tag의 MIT 파일과 lock integrity로만 명확화 | version 또는 integrity가 바뀌면 clarification이 자동 실패하고 재검토 필요 |
| `tauri-plugin-clipboard-manager 2.3.2` | 공식 Tauri plugin의 MIT/Apache-2.0 선택지를 허용. Developer Toolbox 입력과 Knowledge·Code Pad CodeMirror 메뉴의 명시적 Paste에 사용하고 이후 WSL clipboard에 재사용 | app capability는 `allow-read-text`만 부여하고 image/write/clear command는 허용하지 않음. 앱은 설치 뒤 network·sidecar 없이 OS clipboard를 in-process로 사용 |
| `@tauri-apps/plugin-dialog`·`tauri-plugin-dialog 2.7.2` | 공식 Tauri plugin의 MIT/Apache-2.0 선택지를 허용. API Playground multipart file part와 Code Pad managed LSP native archive 또는 Node reviewed `.tgz` closure set의 명시적 파일 선택에 사용한다. 전이 `tauri-plugin-fs 2.5.1`은 MIT/Apache-2.0, `rfd 0.16.0`·`mime_guess 2.0.5`는 MIT다. 수제 platform picker와 외부 도구 연결은 Windows/Linux 권한·취소·경로 전달 회귀 위험이 더 크다. | API Playground와 Code Pad에 각각 `dialog:allow-open`만 부여하고 save/directory 권한을 쓰지 않는다. API 경로는 runtime request state에만 두며 Rust가 regular file·25 MiB/전체 50 MiB를 재검증해 stream한다. Code Pad 선택 archive set은 regular file·크기·SHA-256·archive layout 및 Node lock integrity를 native에서 재검증해 app-owned cache로 복사하고 원본 path를 index/status/log/IPC 오류에 저장·반향하지 않는다. 설치 뒤 network·sidecar가 없고 notices는 기존 graph 항목을 재사용한다. API 기능 전체 frontend bundle은 JS +9,996 bytes(gzip +2,535), CSS +811 bytes(gzip +95)이며 packaged native 증분은 W1 checkpoint에서 기록한다. |
| `@xterm/addon-search 0.16.0`, `@xterm/addon-web-links 0.12.0` | xterm 공식 monorepo의 MIT addon. WSL scrollback search와 일반 URL link provider를 직접 구현할 때의 buffer/Unicode/link-range 회귀 위험보다 작아 허용 | unpacked package 838,673 bytes/45,573 bytes. 같은 toolchain production build에서 WSL JS는 580.64→623.61 kB(+42.97 kB), gzip 164.78→177.98 kB(+13.20 kB)다. 설치 뒤 network·sidecar 없이 동작하며 URL 실행은 별도 HTTP(S) allowlist·credential 거부·사용자 확인 경계를 거침. xterm major 갱신 때 호환 version과 bundle delta를 재검토 |
| `clipboard-win 5.4.1`, `error-code 3.4.0` BSL-1.0 | clipboard plugin의 Windows 전이 경로. [Boost 공식 license](https://www.boost.org/LICENSE_1_0.txt) 조건상 상업 사용·수정·배포가 가능한 permissive license이고 machine-executable object code 배포에는 source notice 재현을 요구하지 않으므로 허용 | source 또는 수정 source를 별도 배포할 때 copyright와 BSL-1.0 전문을 유지. binary installer에도 exact package/license/source/digest를 notices로 추가 고지 |
| qrcode 0.14.1 | MIT OR Apache-2.0. QR 표준 matrix/version/error-correction을 직접 재구현하지 않고 검증된 pure-Rust encoder를 사용한다. default image/svg/pic features는 끄고 byte mode API만 사용해 offline native 경계를 작게 유지한다. upstream은 passively-maintained이므로 version update 때 표준 fixture와 advisory를 재검토한다. | official registry source와 checksum, 선택 license expression, exact version을 notices에 고지. 입력 4 KiB·version 1–40·출력 4 MiB bounds는 app 경계에서 다시 검증한다. |
| png 0.18.1 | MIT OR Apache-2.0. QR의 필요한 grayscale PNG encoder만 사용하며 qrcode의 더 넓은 optional image feature를 켜지 않는다. 이미 lock graph에 있던 crate의 direct edge 승격이므로 새 license family를 추가하지 않는다. | image-rs repository/source, exact checksum과 license를 notices에 유지하고 raw image 4 MiB·base64 bound 및 no-path/no-auto-save 정책을 앱에서 재검증한다. |
| qrcode-generator 2.0.4 | MIT, upstream repository의 의존성 없는 순수 JS encoder. packaged WebView 또는 browser fallback에서 runtime download 없이 native와 같은 payload/option/UTF-8 byte 계약을 제공하기 위해 선택한다. 외부 QR service나 remote URL fetch를 사용하지 않는다. | pnpm integrity와 repository/license를 notices에 고정한다. browser matrix/PNG canvas 결과는 native와 algorithm이 다를 수 있으므로 metadata·bounds·failure fixture를 공통 계약으로 유지하고 update 때 parity를 재검토한다. |

현재 graph에는 GPL, AGPL, SSPL, proprietary runtime이나 git source가 없다. devbox workspace
crate는 배포용 공개 crate가 아니므로 각 Cargo manifest에 `publish = false`를 명시하며,
devbox 자체 라이선스를 제3자 notices가 대신한다고 주장하지 않는다.

## Time-bounded upstream exceptions

아래 항목은 모두 2026-11-30에 만료된다. `deny.toml`의 RustSec ignore와 policy JSON의
metadata가 정확히 일치해야 한다. 만료 연장은 새 PR에서 최신 Tauri graph, 대체 가능성,
Windows/Linux 영향과 제거 계획을 다시 검토해야 하며 날짜만 바꾸는 수정은 허용하지 않는다.

| Upstream path | Advisory IDs | Boundary and removal plan |
|---|---|---|
| Tauri 2 → GTK3 0.18 | RUSTSEC-2024-0411~0420 | Linux WebKit runtime에만 존재하는 archived binding. Tauri가 GTK4 또는 유지되는 compatible line을 제공하면 제거 |
| GTK3 0.18 → `proc-macro-error 1.0.4` | RUSTSEC-2024-0370 | Linux GTK build dependency. glib-macros upgrade와 함께 제거 |
| tauri-utils → urlpattern 0.3 → rust-unic | RUSTSEC-2025-0075, 0080, 0081, 0098, 0100 | 현재 Tauri가 허용하는 urlpattern line의 unmaintained transitive crates. Tauri/urlpattern upgrade 시 제거 |
| Tauri 2 → `glib 0.18.5` | GHSA-wrw7-89jp-8q8g | Dependabot이 탐지하는 Linux-only iterator unsoundness. Windows installer에는 link되지 않으며 Tauri GTK line 갱신 시 제거 |

`cargo-deny`가 다루지 않는 Dependabot GHSA도 policy에 exact locked package로 고정한다. GitHub
alert를 닫거나 dismiss하지 않으며, package가 graph에서 사라지면 stale exception으로 CI가
실패하도록 한다.

## Adding or updating a dependency

runtime dependency PR은 다음 표를 PR body 또는 해당 workthrough에 채운다. gate PR merge 전에는
새 runtime dependency PR을 merge하지 않는다.

| Field | Required evidence |
|---|---|
| Purpose | 가능하게 하는 사용자 기능과 native/offline 가치 |
| Alternatives | 자체 구현, 다른 library, external tool 연결의 비용·제약 비교 |
| Source | official repository와 registry/archive URL |
| Pin | manifest constraint, resolved lock version, binary면 SHA-256와 signature |
| License | chosen SPDX branch, 배포·수정·고지 의무와 notices 결과 |
| Size | installer/resource와 runtime memory 증가 실측 |
| Security | advisory 결과, untrusted input limit, sandbox/permission boundary |
| Offline | 설치 뒤 network 없이 동작하는 경로와 optional download fallback |
| Maintenance | update owner, monitoring source, rollback/removal strategy |

검증 명령:

```bash
pnpm install --frozen-lockfile
pnpm audit --audit-level moderate
python3 .github/scripts/check-dependencies.py generate  # dependency 변경 PR에서만
python3 .github/scripts/check-dependencies.py check
python3 .github/scripts/test-build-manifest.py
cargo deny --locked check
bash .github/scripts/check-catalog.sh
```

`THIRD_PARTY_NOTICES.md`는 직접 편집하지 않는다. 새 license expression, `Unknown`, git registry,
checksum/integrity 없는 package, 만료되거나 lockfile과 맞지 않는 exception은 기본적으로 실패한다.
