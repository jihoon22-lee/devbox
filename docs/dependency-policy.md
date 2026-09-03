# Dependency and Third-Party Notice Policy

devbox의 P1·P2 native 기능은 설치 뒤 오프라인에서 동작해야 한다. 라이브러리를 번들하는
경우 기능 구현과 같은 수준으로 출처·고정 버전·라이선스·보안·배포 고지를 관리한다. 이
문서는 `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` §1.3과 P1-01을
실행 가능한 gate로 구체화한다.

## Release evidence boundary

- 현재 v0.7.0 stable은 annotated tag object
  `ec41ceb2ed4b4864d34afe383e5ff816481b3d37`에서 peeled source
  `3a23f49c85aa3c3d04b86f227e8aa184ef964085`를 가리킨다. candidate workflow
  `33782002859`는 packaged runtime과 installer lifecycle을 각각 15/15로 통과했고, release
  workflow `33785966618`과 fresh-download verifier가 15개 앱·32개 public asset·31개
  manifest-declared asset·missing/undeclared/failure 0을 확인했다.
- v0.6.0 historical stable은 annotated tag object
  `a974adf975862da3d5ada16c6c6efe704387ddd7`, peeled source
  `d2fa25a0a1f087459838449daded00c0b09764b4`, candidate `33384213398`, release workflow
  `33390009009`의 evidence를 보존한다.
- 공개 v0.5.0 stable은 tag `efc98dd3c91b77ee7c9024010ac012a6c68f2b54`, workflow `33216176818`,
  15개 앱·32개 public asset·31개 manifest-declared asset·mismatch 0의 evidence를 가진다.
- v0.5.1 historical stable source/bundle은 #470/#473/#477/#478/#479를 포함한다. 15-app/32-public-asset/
  31-manifest-declared/mismatch-0 contract와 정확한 tag commit·workflow·asset digest·Latest
  metadata는 GitHub Release가 권위 있는 publication source다.
- v0.5.0 stable `Cargo.lock` SHA-256은
  `5b4bb7641d6b9350c30b21b19de38fc48f742ed7941dbbce1f6657746ef33551`, stable
  `THIRD_PARTY_NOTICES.md` SHA-256은
  `018e8191ba4a2e019516d2423fd081b224b228ec39c0ca135c2aa74c7da9f181`이다. 현재
  v0.5.1 stable source 비교값은 각각
  `a3398f535faeba6be0a8f7a05a8ae57f1141808310c42344c132d769740fde3a`와
  `b2cc4ca07b0886700e04364b4fb0eb0c98da99b6dde10fb58c47ab03bb563d35`다. v0.6.0 historical stable
  source의 비교값은 각각 `ebe22c7df176d95685cc9ff9c0eb3760ac08b95829498d7c5884f89dd10977c7`와
  `6cbc242562e62ac8892bc88e3ca8fcad5e1dd7911db2b46a200cb8d1786a26d9`이며 release별 evidence를
  혼용하지 않는다.
- 현재 v0.7.0 stable source의 `Cargo.lock` SHA-256은
  `aec85df4631d0f7ebb0d8d4b6f0e6b4ca38fb9eed3b4fa164525e7b085a5db3d`, generated
  `THIRD_PARTY_NOTICES.md` SHA-256은
  `e964b2b711a8b80e793a230d89e20581fe7874dcf841fa9c9d839297440f84a4`다. 공개 notice asset도
  같은 digest이며 release manifest SHA-256은
  `e92dda897d40de1891d8a204d20f28524674c0b336f09923d52381fd764217ab`다.

## Enforced gates

| Gate | Source of truth | CI behavior |
|---|---|---|
| Cargo graph | `Cargo.lock`, `deny.toml` | Windows/Linux target의 license, advisory, duplicate ban, registry/git source를 `--locked`로 검사 |
| pnpm graph | `pnpm-lock.yaml`, `.github/dependency-policy.json` | frozen install 뒤 full transitive license와 audit를 검사; 미허용 표현·integrity 부재·미등록 `Unknown`을 거부 |
| Exceptions | `.github/dependency-policy.json` | ID, package, exact locked version, detector, scope, reason, ISO date expiry를 모두 요구; 만료 당일부터 merge 불가 |
| Notices | 두 lockfile + package metadata | 732 Rust package와 160 frontend runtime package의 version/license/source/digest를 결정적으로 재생성해 checked-in 파일과 byte 비교 |
| Distribution | `tauri.conf.json`, release manifest | 모든 release 앱 installer에 notices resource를 넣고, release에는 notices와 그 size/SHA-256을 manifest-declared asset으로 게시 |

v0.6.0 historical stable source의 `THIRD_PARTY_NOTICES.md`는 145,317 bytes이며 위 SHA-256과 함께
기록된 비교값이다. installer에서는 동일 파일을 압축 resource로 포함하므로 새 executable
runtime이나 network dependency를 추가하지 않는다. portable 사용자는 release의 독립 notice
asset을 받을 수 있다. release manifest는 schemaVersion 1을 유지하고 optional `notices` 필드를
추가하므로 기존 Devbox Manager parser와 호환된다. v0.5.0 stable evidence는 15개 앱 기준
30 binaries + notices + manifest의 32 assets였고, v0.6.0 release workflow도 같은 contract를
독립 검증했다.

v0.7.0 stable notices는 146,028 bytes다. WebGL addon과 Quick Summon의 공식 Tauri plugin graph를
포함하고, app version 동기화는 workspace package version과 lock hash만 바꾸며 third-party
inventory를 추가하지 않았다. source generator read-back과 `cargo deny --locked check`, Windows
installer resource digest, candidate/public release fresh download를 모두 통과했다.

## Current decisions

`h2 0.4.15`의 empty DATA frame memory-growth advisory(RUSTSEC-2026-0258)는 호환되는
0.4.16으로 즉시 갱신했다. 이 취약점에는 예외를 만들지 않는다.

2026-08-31 v0.6.0 준비에서는 yanked `chacha20 0.10.1`을 동일한 `rand 0.10.2` 전이
경계 안의 non-yanked `0.10.2`로 갱신했다. `lopdf 0.44.0`과 `tungstenite 0.30.0`의 상위
version이나 feature는 바꾸지 않았고, lockfile·notices·전체 Rust 회귀를 함께 재생성한다.

다음 license는 자동 허용 후보 외의 수동 검토 결과다.

| Dependency | Decision | Distribution obligation |
|---|---|---|
| `cssparser`, `selectors`, `dtoa-short`, `option-ext` 등 MPL-2.0 Rust crates | 허용. devbox가 upstream source를 수정하지 않고 파일 단위 copyleft 경계를 유지한다. | exact crate source와 digest를 notices에 남기고, 수정 시 MPL source 제공 의무를 다시 검토 |
| `dompurify` `(MPL-2.0 OR Apache-2.0)` | Apache-2.0 선택지로 허용 | notices에 expression/source/integrity 유지 |
| [`axe-core 4.13.0`](https://github.com/dequelabs/axe-core/tree/v4.13.0) MPL-2.0 | 15개 frontend의 jsdom 구조 접근성 회귀 검사에만 쓰는 test-only package로 허용. 전역 MPL allowlist가 아닌 package/version/integrity 고정 승인을 사용하고, upstream source를 수정하지 않으며 앱 runtime import와 production 초기 bundle에서 제외한다. 설치된 package는 3,113,323 logical bytes/3,174,400 allocated bytes이며 test가 외부 network·executable을 사용하지 않는다. | CI/source checkout의 exact package license·source·integrity를 유지하고, runtime notices·installer에는 포함하지 않는다. package 수정 또는 runtime 편입 시 MPL source 제공 의무와 배포 경계를 다시 검토 |
| `lru-cache` BlueOak-1.0.0 | permissive license로 수동 허용 | license/source/integrity 유지 |
| `caniuse-lite` CC-BY-4.0 | browser support data attribution 조건으로 허용 | notices attribution과 source 유지 |
| `khroma 2.1.0` | package metadata의 `Unknown`을 exact upstream tag의 MIT 파일과 lock integrity로만 명확화 | version 또는 integrity가 바뀌면 clarification이 자동 실패하고 재검토 필요 |
| `tauri-plugin-clipboard-manager 2.3.2` | 공식 Tauri plugin의 MIT/Apache-2.0 선택지를 허용. Developer Toolbox 입력과 Knowledge·Code Pad CodeMirror 메뉴의 명시적 Paste에 사용하고 이후 WSL clipboard에 재사용 | app capability는 `allow-read-text`만 부여하고 image/write/clear command는 허용하지 않음. 앱은 설치 뒤 network·sidecar 없이 OS clipboard를 in-process로 사용 |
| `@tauri-apps/plugin-dialog`·`tauri-plugin-dialog 2.7.2` | 공식 Tauri plugin의 MIT/Apache-2.0 선택지를 허용. API Playground multipart file part와 Code Pad managed LSP native archive 또는 Node reviewed `.tgz` closure set의 명시적 파일 선택에 사용하고, Devbox Manager는 WinGet Configuration 입력 파일의 native-only picker에 재사용한다. 전이 `tauri-plugin-fs 2.5.1`은 MIT/Apache-2.0, `rfd 0.16.0`·`mime_guess 2.0.5`는 MIT다. 수제 platform picker와 외부 도구 연결은 Windows/Linux 권한·취소·경로 전달 회귀 위험이 더 크다. | API Playground와 Code Pad에 각각 `dialog:allow-open`만 부여하고 save/directory 권한을 쓰지 않는다. Manager는 `app.dialog().file().blocking_pick_file()`을 Rust command 안에서만 호출하며 renderer capability에는 `dialog:allow-open`을 부여하지 않는다. Manager 입력은 `.winget`/`.yaml`/`.yml` filter 뒤 native regular-file·256 KiB 경계로 다시 읽고 원본 path를 DTO에 넣지 않는다. API 경로는 runtime request state에만 두며 Rust가 regular file·25 MiB/전체 50 MiB를 재검증해 stream한다. Code Pad 선택 archive set은 regular file·크기·SHA-256·archive layout 및 Node lock integrity를 native에서 재검증해 app-owned cache로 복사하고 원본 path를 index/status/log/IPC 오류에 저장·반향하지 않는다. 설치 뒤 network·sidecar가 없고 notices는 기존 graph 항목을 재사용한다. API 기능 전체 frontend bundle은 JS +9,996 bytes(gzip +2,535), CSS +811 bytes(gzip +95)이며 packaged native 증분은 W1 checkpoint에서 기록한다. |
| `@xterm/addon-search 0.16.0`, `@xterm/addon-web-links 0.12.0` | xterm 공식 monorepo의 MIT addon. WSL scrollback search와 일반 URL link provider를 직접 구현할 때의 buffer/Unicode/link-range 회귀 위험보다 작아 허용 | unpacked package 838,673 bytes/45,573 bytes. 같은 toolchain production build에서 WSL JS는 580.64→623.61 kB(+42.97 kB), gzip 164.78→177.98 kB(+13.20 kB)다. 설치 뒤 network·sidecar 없이 동작하며 URL 실행은 별도 HTTP(S) allowlist·credential 거부·사용자 확인 경계를 거침. xterm major 갱신 때 호환 version과 bundle delta를 재검토 |
| `clipboard-win 5.4.1`, `error-code 3.4.0` BSL-1.0 | clipboard plugin의 Windows 전이 경로. [Boost 공식 license](https://www.boost.org/LICENSE_1_0.txt) 조건상 상업 사용·수정·배포가 가능한 permissive license이고 machine-executable object code 배포에는 source notice 재현을 요구하지 않으므로 허용 | source 또는 수정 source를 별도 배포할 때 copyright와 BSL-1.0 전문을 유지. binary installer에도 exact package/license/source/digest를 notices로 추가 고지 |
| qrcode 0.14.1 | MIT OR Apache-2.0. QR 표준 matrix/version/error-correction을 직접 재구현하지 않고 검증된 pure-Rust encoder를 사용한다. default image/svg/pic features는 끄고 byte mode API만 사용해 offline native 경계를 작게 유지한다. upstream은 passively-maintained이므로 version update 때 표준 fixture와 advisory를 재검토한다. | official registry source와 checksum, 선택 license expression, exact version을 notices에 고지. 입력 4 KiB·version 1–40·출력 4 MiB bounds는 app 경계에서 다시 검증한다. |
| png 0.18.1 | MIT OR Apache-2.0. QR의 필요한 grayscale PNG encoder만 사용하며 qrcode의 더 넓은 optional image feature를 켜지 않는다. 이미 lock graph에 있던 crate의 direct edge 승격이므로 새 license family를 추가하지 않는다. | image-rs repository/source, exact checksum과 license를 notices에 유지하고 raw image 4 MiB·base64 bound 및 no-path/no-auto-save 정책을 앱에서 재검증한다. |
| qrcode-generator 2.0.4 | MIT, upstream repository의 의존성 없는 순수 JS encoder. packaged WebView 또는 browser fallback에서 runtime download 없이 native와 같은 payload/option/UTF-8 byte 계약을 제공하기 위해 선택한다. 외부 QR service나 remote URL fetch를 사용하지 않는다. | pnpm integrity와 repository/license를 notices에 고정한다. browser matrix/PNG canvas 결과는 native와 algorithm이 다를 수 있으므로 metadata·bounds·failure fixture를 공통 계약으로 유지하고 update 때 parity를 재검토한다. |
| `libc 0.2.189`·`windows 0.61.3` | 이미 workspace lock graph와 notices에 존재하는 MIT OR Apache-2.0 platform bindings의 target-specific direct edge를 filesystem crate에 추가해, Webhook Lab fixture sidecar에 Unix `flock`/Windows `LockFileEx`를 사용한다. 새 registry package나 network/runtime dependency를 추가하지 않는다. | 기존 Cargo.lock version/checksum과 notices inventory를 유지한다. lock API는 non-blocking primitive만 제공하고 caller가 500ms bounded retry/fixed error를 적용하며, lock sidecar를 삭제·교체하지 않는다. |
| `bytes 1.12.1`·`getrandom 0.4.3` | 이미 workspace lock graph와 notices에 존재하는 MIT(`bytes`) 및 MIT OR Apache-2.0(`getrandom`) crate의 direct edge를 API Playground와 Devbox Manager에 추가한다. `bytes`는 reqwest의 bounded response stream item을 명시적으로 다루고, `getrandom`은 API Playground MCP connection ID에는 OS CSPRNG 128-bit 값을, Manager Dev Setup preview/apply temporary-file ID에는 256-bit 값을 직접 채운다. 더 높은 수준의 `rand`/UUID API나 외부 executable을 이 경계에 도입하는 것보다 권한·surface가 작다. | 새 registry package·license family·runtime download는 없다. exact locked source/checksum을 generated notices에 유지하고, response는 4 MiB·retained list는 종류별 16 MiB로 제한한다. Manager ID는 `devsetup-`/`apply-` prefix와 32 random bytes를 사용하며, RNG 실패는 raw OS 오류 없이 fixed state error로 종료하고 약한 fallback을 만들지 않는다. |
| `reqwest 0.13.4`·`futures-util 0.3.33` | Repo Manager Dependency Lens의 사용자가 승인한 OSV/deps.dev metadata 조회를 위해 direct edge로 연결한다. 두 package와 모든 전이는 이미 workspace `Cargo.lock`에 있었고, `reqwest`는 `default-features = false`와 `json`·`rustls`만 사용하며 `futures-util`은 bounded concurrent GET scheduling에 사용한다. 새 license family나 새 resolved package는 추가하지 않는다. | notices에 기존 exact source/checksum/license를 유지한다. native endpoint는 fixed HTTPS host만 사용하고 proxy·redirect를 끄며 4초 timeout과 OSV 4 MiB/deps.dev 512 KiB·2 MiB response bounds를 적용한다. IPC가 URL·header·proxy를 지정할 수 없고, 원격 오류는 fixed error로 닫힌다. |

### Devbox Manager Dev Setup YAML parser decision (2026-08-30)

The following direct and transitive packages are approved as one narrow
dependency decision for the Manager’s WinGet Configuration v3 package-only
flow. They are new entries in this candidate’s Cargo lock graph. The decision
does not permit generic YAML execution or arbitrary DSC resources.

| Field | Decision record |
|---|---|
| Purpose | `serde_yaml_ng 0.10.0` provides Serde-based YAML deserialization for the bounded external WinGet Configuration v3 import. Canonical export/apply output is rendered manually from the validated package model rather than serialized from the imported document. Its locked transitive parser/formatting packages are `unsafe-libyaml 0.2.11` and `ryu 1.0.23`. Imported YAML is never passed to WinGet. The Manager also adds the existing lock-graph `zeroize = "1"` as a direct edge and creates the raw byte buffer inside `Zeroizing` before reading, then borrows its UTF-8 view; this does not introduce a new resolved package or claim that parser-owned allocations are wiped. |
| Alternatives | A hand-written YAML parser would duplicate YAML quoting/escaping behavior and increase parser correctness risk. A generic YAML `Value`/full DSC interpreter or direct `winget configure --file <imported-path>` would broaden the attack and execution surface. JSON cannot read the user-facing WinGet Configuration format. The maintained `serde_yaml_ng` fork gives a small Serde API while the application supplies the stricter lexical and schema boundary. |
| Source | [`serde_yaml_ng` crates.io](https://crates.io/crates/serde_yaml_ng/0.10.0) and [upstream](https://github.com/acatton/serde-yaml-ng); [`unsafe-libyaml` crates.io](https://crates.io/crates/unsafe-libyaml/0.2.11) and [upstream](https://github.com/dtolnay/unsafe-libyaml); [`ryu` crates.io](https://crates.io/crates/ryu/1.0.23) and [upstream](https://github.com/dtolnay/ryu). All are registry packages; no git dependency is used. |
| Pin | Manifest direct edges: `serde_yaml_ng = "0.10.0"` and `zeroize = "1"`. `Cargo.lock` resolves `serde_yaml_ng 0.10.0` checksum `7b4db627b98b36d4203a7b458cf3573730f2bb591b28871d916dfa9efabfd41f`, `unsafe-libyaml 0.2.11` checksum `673aac59facbab8a9007c7f6108d11f63b603f7cabff99fabf650fea5c32b861`, and `ryu 1.0.23` checksum `9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f`. `zeroize 1.9.0` (and its existing `zeroize_derive 1.5.0` entry) is already in the workspace lock graph; no new resolved package is expected from making it direct. Release/build checks use the locked graph. |
| License | `serde_yaml_ng` is MIT; `unsafe-libyaml` is MIT; `ryu` is Apache-2.0 OR BSL-1.0, with the Apache-2.0 branch selected for distribution engineering. Preserve exact package/version/source/checksum/license in generated `THIRD_PARTY_NOTICES.md`; do not edit that generated file by hand. The permissive licenses do not replace devbox’s own license. |
| Size | Pending an actual packaged Manager checkpoint. No unpacked-crate, installer/resource, or runtime-memory delta is claimed here. Measure the dependency and packaged Manager delta at the package checkpoint before release and record the evidence. |
| Security | `unsafe-libyaml` is an unsafe Rust translation of libyaml and is treated as an untrusted-input parser boundary. Before deserialization, the Manager caps input at 256 KiB/4,096 lines/8 KiB per line/32-space indentation, rejects controls, aliases, anchors, tags, merges, directives, and multiple documents, then uses `deny_unknown_fields` typed structs and exact resource/property allowlists. Package IDs, versions, resource counts, and canonical output are bounded; no YAML tag is executed. Cargo advisory/license/source checks remain mandatory and this record creates no advisory exception. |
| Offline | Parsing, review-model construction, and canonical export are in-process and work without a network. Package observation and apply intentionally require Windows WinGet/App Installer and network access; there is no runtime YAML/parser download or external YAML execution fallback. The fixed `winget` source-name check does not validate the locally registered source URL or officialness. |
| Maintenance | Devbox Manager maintainers own the direct dependency and its lock/notices record. Monitor `serde-yaml-ng` releases and the `unsafe-libyaml`/`ryu` upstream advisories; updates require the same parser fixtures, bounds/allowlist review, `cargo deny --locked check`, lock regeneration, and generated-notice comparison. If the parser line becomes unsuitable, remove the feature or replace it behind the same typed normalization boundary; do not widen the accepted DSC surface. |

### API Playground dynamic gRPC dependency decision (2026-08-30)

The following packages are approved as one bounded dependency family for the native gRPC panel in
Protocol Lab. This decision does not permit arbitrary HTTP/2 paths, request metadata, generated user
code, a downloaded compiler, or a certificate-verification bypass.

| Field | Decision record |
|---|---|
| Purpose | `tonic 0.14.6` supplies the maintained HTTP/2 gRPC client and rustls channel; `prost`/`prost-types 0.14.4` and `prost-reflect 0.16.5` supply descriptor-backed dynamic messages and canonical ProtoJSON; `protox 0.9.1`/`protox-parse 0.9.0` compile a user-selected local source in process; `tonic-reflection 0.14.6` and its locked `tonic-prost 0.14.6` client messages support reflection v1/v1alpha; `tokio-stream 0.1.19` supplies bounded one-shot streaming inputs; the already locked `rustls-pki-types 1.15.1` PEM API validates certificate chains and one unencrypted private key. Test-only `tonic` server and `tonic-reflection` server features plus a direct dev edge to the already resolved `tonic-prost` run an in-process HTTP/2 fixture for both reflection eras and all four RPC kinds; they are not enabled by a production app build. The direct `rustls-pemfile` candidate was removed after `cargo deny` reported RUSTSEC-2025-0134; no advisory exception was added. |
| Alternatives | Hand-writing HTTP/2 framing, reflection, protobuf wire encoding, descriptor linking, and ProtoJSON would create a larger protocol/security surface. Static generated clients cannot represent runtime user-selected schemas. External `grpcurl`, a system `protoc`, or a downloaded/vendored compiler would add executable discovery, platform/version drift, and offline/package obligations. `protox` keeps local compilation in process and the backend still supplies a contained, no-link resolver. |
| Source | Official registry/upstream sources are [`tonic`](https://crates.io/crates/tonic/0.14.6) / [hyperium/tonic](https://github.com/hyperium/tonic), [`prost`](https://crates.io/crates/prost/0.14.4) / [tokio-rs/prost](https://github.com/tokio-rs/prost), [`prost-reflect`](https://crates.io/crates/prost-reflect/0.16.5), [`protox`](https://crates.io/crates/protox/0.9.1), [`tokio-stream`](https://crates.io/crates/tokio-stream/0.1.19), and [`rustls-pki-types`](https://crates.io/crates/rustls-pki-types/1.15.1). Protocol behavior follows the official gRPC reflection/deadline/cancellation references and the protobuf ProtoJSON specification linked from the feature contract. All resolved sources are crates.io registry entries; there is no git or binary dependency. |
| Pin | Production manifest constraints are `tonic = 0.14.6` with only channel/codegen/aws-lc/native-root features, `tonic-reflection = 0.14.6` without server defaults, `prost`/`prost-types = 0.14`, `prost-reflect = 0.16.5`, `protox = 0.9.1`, `tokio-stream = 0.1.17`, and `rustls-pki-types = 1.15.1`. Dev-only edges enable `tonic`/`tonic-reflection` `server` and pin `tonic-prost = 0.14.6`. The lock resolves the exact versions above; primary checksums include tonic `ac2a5518c70fa84342385732db33fb3f44bc4cc748936eb5833d2df34d6445ef`, prost `528ac67416ff8646872a3c02cad9cc4ee5dc9f9540c9b10771855c95cb2e5ae1`, prost-reflect `01b80ea363c31af2de2b92e3c07ed1156628f7838c4afb4df75ee78a37fedbd1`, protox `4f25a07a73c6717f0b9bbbd685918f5df9815f7efba450b83d9c9dea41f0e3a1`, and tonic-reflection `acccd136a4bf19810a1fde9c74edc6129b42a66b44d0c1c8aaa67aeb49a146a7`. The generated notices retain every full checksum and transitive version. |
| License | Tonic and its reflection/prost adapter are MIT; prost is Apache-2.0; prost-reflect/protox and their parser/codegen closure are MIT OR Apache-2.0; tokio-stream is MIT; rustls-pki-types is MIT OR Apache-2.0. The 28 newly resolved package-version entries introduce no GPL/AGPL/SSPL/proprietary or git source. `cargo deny --locked check` passes licenses/sources, and generated `THIRD_PARTY_NOTICES.md` records the exact expression, source, and checksum. |
| Size | The 28 newly resolved crate source directories occupy 9,855,956 logical bytes and 13,352,960 allocated bytes in this Cargo cache; no frontend package was added. Under the same pnpm/Vite toolchain with deterministic `gzip -n`, API Playground changes from JS 553,195 to 589,137 bytes (+35,942; gzip 165,529 to 174,662, +9,133) and CSS 39,934 to 52,330 bytes (+12,396; gzip 6,982 to 8,170, +1,188). These are source/build checkpoints, not installer claims. Measure and record the Windows packaged installer/resource delta in #493 before release. |
| Security | Runtime source, descriptor, method, JSON, stream, timeout, connection, and response budgets are enforced on both IPC and native boundaries. Local imports use an authorized no-link root and fresh filesystem identities; reflection falls back only on explicit v1 `UNIMPLEMENTED`; method paths come only from the connected descriptor. TLS has no trust-all path, mTLS PEM never crosses IPC, and DPAPI storage is Windows-only, strict-schema, bounded, atomic, and separated from ordinary environment secrets by distinct optional-entropy domains. `cargo deny` found no new actionable advisory after replacing the unmaintained PEM crate; the existing workspace yanked/advisory warnings remain governed by the time-bounded baseline policy and are not widened here. |
| Offline | Local `.proto` compilation, descriptor projection, ProtoJSON conversion, TLS credential management, and history work entirely in process after installation. No `protoc`, code generator executable, runtime crate download, or sidecar is used. Reflection and RPC calls intentionally require the user-selected server; network failure never causes a protocol downgrade or request retry. |
| Maintenance | API Playground maintainers own the aligned tonic/prost/protox family. Monitor RustSec and the official tonic, prost, prost-reflect, protox, tokio, and rustls-pki-types releases; upgrades must preserve Linux/Windows CI, reflection fixtures, all four RPC kinds, ProtoJSON compatibility, TLS roots/mTLS, bounds, notices, and package evidence. The rollback boundary is the gRPC tab and its native commands; removing it removes the direct family without changing MCP or REST persistence. |

### WSL Desktop WebGL renderer decision (2026-09-02)

`@xterm/addon-webgl` is approved as the terminal renderer for WSL Desktop only. This decision does
not permit any other renderer addon, a WebGL dependency in another app, or removal of the DOM
renderer fallback.

| Field | Decision record |
|---|---|
| Purpose | WSL Desktop shipped on xterm's DOM renderer, the slowest one xterm provides. Heavy command output and full-screen TUIs repaint through DOM nodes per cell. The official WebGL addon renders the same buffer on the GPU. The terminal design document already named this addon with a canvas/DOM fallback in its usability table; it was excluded from #262's acceptance and is completed here. |
| Alternatives | Keeping the DOM renderer leaves the app's core surface on its slowest path. The canvas addon is deprecated upstream in favour of WebGL. Writing a renderer against xterm's internal render API would duplicate maintained upstream code and break on every xterm release. Doing nothing was rejected because rendering speed is the terminal's primary perceived quality. |
| Source | Official registry entry [`@xterm/addon-webgl`](https://www.npmjs.com/package/@xterm/addon-webgl/v/0.19.0) from the same [xtermjs/xterm.js](https://github.com/xtermjs/xterm.js) project that already supplies `@xterm/xterm`, `addon-fit`, `addon-search`, `addon-unicode11` and `addon-web-links`. Registry package only; no git or binary dependency. |
| Pin | Manifest constraint `@xterm/addon-webgl: ^0.19.0`; the lock resolves exactly `0.19.0` with integrity `sha512-b3fMOsyLVuCeNJWxolACEUED0vm7qC0cy4wRvf3oURSzDTYVQiGPhTnhWZwIHdvC48Y+oLhvYXnY4XDXPoJo6A==`. It adds no transitive package. |
| License | MIT, the same expression as every other xterm package already bundled, and already on the pnpm allowlist. Generated `THIRD_PARTY_NOTICES.md` records the expression, source and integrity. |
| Size | The addon is loaded through a dynamic import, so it is a separate chunk and stays out of the initial bundle. Measured with the repository's own budget check: initial raw 665,650 to 679,733 bytes, initial gzip 190,941 to 196,300; the lazy chunk is 110,225 bytes raw and 29,794 gzip. Both initial budgets (755,000 raw, 220,000 gzip) still pass. These are build checkpoints, not installer claims. |
| Security | The addon renders the terminal buffer already held in the renderer; it opens no network, filesystem or IPC path and receives no new data. `pnpm audit --audit-level moderate` reports no known vulnerability. Failure to load the chunk, failure to obtain a WebGL context, and later context loss all fall back to the DOM renderer silently; the terminal, its PTY connection and its scrollback are unaffected in every case. |
| Offline | The chunk is built into the installed app, so the renderer works with no network. There is no runtime download and no external GPU driver requirement beyond what WebView2 already provides; without a context the app simply keeps the DOM renderer. |
| Maintenance | WSL Desktop maintainers own it and upgrade it together with `@xterm/xterm`, whose version it must match. Monitor the official xterm.js releases and npm advisories. The rollback boundary is one dynamic import in `TermPane`; removing it restores the DOM renderer without touching any other behaviour. |

### WSL Desktop Quick Summon dependency decision (2026-09-03)

| Field | Decision record |
|---|---|
| Purpose | `tauri-plugin-global-shortcut` provides maintained system-wide registration and event dispatch for the already-running WSL Desktop window. Tauri's existing `tray-icon` feature provides the optional tray without another package. Both paths preserve live PTYs in the same process and work offline. |
| Alternatives | Hand-written Win32 `RegisterHotKey` code would add Windows-only unsafe FFI and message-loop ownership. Guest JavaScript registration would require broader webview permissions. AutoHotkey, PowerToys, shell scripts, or process restart would add an external dependency or lose live terminal state. |
| Source | Official Tauri plugin documentation and `tauri-apps/plugins-workspace`; native adapter `tauri-apps/global-hotkey`; Linux-only keysym helper `notgull/xkeysym`. All resolved artifacts are crates.io registry packages, not git or downloaded binaries. |
| Pin | Direct constraint `tauri-plugin-global-shortcut = "2.3.2"`; lock resolves plugin 2.3.2 (`b4dd9f4c5136c09cd962da0c86dc4accd4666db2ea591cf16e6597435843bd2b`), `global-hotkey` 0.8.0 (`8c386b0a4a70cb2d39fffd74480f985b6f0bfbcb934b6a6b6b7e630e448f242e`), and Linux-only `xkeysym` 0.2.1 (`b9cc00251562a284751c9973bace760d86c0276c471b4be569fe6b068ee97a56`). Existing Tauri 2.11.5 already resolved `tray-icon` 0.24.2. |
| License | Plugin and `global-hotkey`: Apache-2.0 OR MIT. `xkeysym`: MIT OR Apache-2.0 OR Zlib. All expressions are allowed and exact source/checksum/license remains in generated notices. |
| Size | Three new source trees measured 1,266,479 logical / 1,572,864 allocated bytes in the development cache; `xkeysym` is outside the Windows normal graph. No frontend package was added. Initial WSL Desktop JS changed +4,442 raw/+1,244 gzip and CSS +333 raw/+63 gzip, within repository budgets. Installer and Windows runtime memory are measured only by the Windows candidate, not inferred from source size. |
| Security | Rust accepts only four fixed shortcut presets; the webview receives no arbitrary plugin registration permission. Changes are serialized, failed replacement restores the previous registration where possible, renderer errors are fixed enums, and tray failure cannot enable close interception. No new advisory exception was added. |
| Offline | Shortcut registration, show/hide/focus, tray/menu actions, and settings migration are in-process. No network, runtime download, shell, WSL command, or external helper is used. |
| Maintenance | Update with the Tauri/plugin family and re-run Linux/Windows compile, shortcut conflict/serialization/UI tests, dependency policy, notices, bundle and package measurements. Removing the module, one direct edge and tray feature restores the prior close behavior without changing terminal/session storage. |

### Manual review record

- `glib` [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g)는 Linux-only
  Tauri GTK transitive dependency다. 2026-09-01 post-release review에서 GitHub advisory의
  affected range가 `>=0.15.0,<0.20.0`, patched version이 `0.20.0`임을 확인했다. 공식
  [최신 Tauri release `2.11.5`](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.11.5)와
  [upstream `dev` manifest](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/Cargo.toml) 모두
  `gtk = "0.18"`을 유지하고, 로컬 `cargo tree -i glib@0.18.5 --workspace -e normal`은
  `tauri 2.11.5 → gtk/webkit2gtk → glib 0.18.5` 경로를 확인했다.
  `cargo update -p glib@0.18.5 --precise 0.20.0 --dry-run`은 `gtk 0.18.2`의 `glib ^0.18`
  제약으로 해석에 실패하므로 compatible patched graph는 아직 없다. Windows installer에
  link되지 않는다는 engineering boundary만 기록하고 Dependabot alert는 open 상태로 유지한다.
  exception expiry `2026-11-30`은 연장하지 않았으며, 그 전 또는 Tauri update 때 graph를 다시
  검토한다. 이는 vulnerability-free 또는 법적 면책 선언이 아니다.
- MPL transitive crates는 upstream source를 수정하지 않은 engineering 상태와 exact version/source/
  digest를 notices에 남긴다. source를 수정하면 MPL source-distribution 검토를 다시 한다.
- `dompurify`의 `(MPL-2.0 OR Apache-2.0)` 중 Apache branch를 선택한 것은 배포 engineering
  decision이다. notices/source/integrity를 보존하며 MPL 의무가 법적으로 사라진다고 주장하지 않는다.
- `r-efi` 5.3.0/6.0.0의 `(MIT OR Apache-2.0 OR LGPL-2.1-or-later)`는 shipped version별 MIT
  또는 Apache branch를 선택한 engineering record로 남긴다. legal clearance나 법률 자문을
  주장하지 않는다.

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
