# Catalog v2 Revision Fallback

## Overview

P1-03-C [#238](https://github.com/jihoon22-lee/devbox/issues/238)의 catalog v2·
revision freshness·capability filter 기반을 구현했다. 기존 Manager 전용 v1 구조체를
공용 순수 crates/catalog 계약으로 교체하고, build-time catalog와 runtime catalog를
revision으로 비교해 안전하게 선택할 수 있는 parser와 회귀 fixture를 추가했다.

이 변경은 devbox가 Manager 없이 단독 실행되는 환경과, Manager가 갱신한 catalog를
사용하는 환경에서 같은 schema와 capability 의미론을 공유하도록 만든다. 최신이고
유효한 runtime 사본은 사용하지만, runtime 사본이 없거나 손상되었거나 v1이거나
build-time보다 오래되면 build-time 사본으로 폴백한다. 실제 설치 여부·executable
해석·runtime 파일 쓰기·UI 메뉴 연결은 이 기능의 경계 밖에 둔다.

## Context

v0.4.x의 apps/catalog.json은 schema v1 identity 목록이었고, 앱 간 capability와
catalog 사본의 freshness를 표현하지 못했다. 이 상태에서는 consumer가 path나
handoff를 받을 수 있는 앱을 각자 하드코딩해야 하며, Manager가 사용하는 parser와
향후 다른 consumer의 해석이 달라질 수 있었다.

v0.5.0 interop 설계는 다음을 요구한다.

- schema v1은 계속 읽을 수 있어야 한다.
- schema v2는 positive catalogRevision을 요구해야 한다.
- accepts, produces, actions는 정적이고 versioned인 선언이어야 한다.
- runtime 사본은 build-time revision 이상일 때만 우선해야 한다.
- malformed 또는 stale runtime은 앱 시작 실패가 아니라 build-time fallback으로
  격리해야 한다.
- capability filter는 순수 catalog 결과만 반환하고 설치 상태 판단은 launch 계층이
  맡아야 한다.
- parser 오류는 입력 JSON, secret, 경로 등 신뢰하지 않은 값을 그대로 반향하지
  않아야 한다.

## Changes Made

### 1. Shared crates/catalog crate

다음 파일을 추가하고 workspace에 등록했다.

- crates/catalog/Cargo.toml
- crates/catalog/src/lib.rs
- crates/catalog/tests/catalog.rs
- crates/catalog/tests/fixtures/v1-legacy.json
- crates/catalog/tests/fixtures/v2-build.json
- crates/catalog/tests/fixtures/v2-runtime-newer.json
- crates/catalog/tests/fixtures/v2-runtime-stale.json
- crates/catalog/tests/fixtures/runtime-corrupt.json
- crates/catalog/tests/fixtures/fake-sixteenth.json

공용 crate는 serde와 serde_json만 사용하며, workspace Cargo.toml의 member와
Cargo.lock에 반영했다. crate 자체는 publish=false 경계를 유지한다.

공개 API는 다음 책임으로 나뉜다.

- parse_catalog: v1/v2 문서 parsing·validation·legacy normalization
- select_catalog: build-time/runtime 사본의 revision freshness 선택
- capable_targets: accepts exact capability를 받는 앱 조회
- capable_producers: produces exact capability를 발행하는 앱 조회
- Catalog, CatalogApp, CatalogAction: Manager와 consumer가 공유하는 정규화된 모델
- CatalogSource, RuntimeFallbackReason, CatalogSelection: 선택 source와 안전한 fallback
  원인을 표현하는 결과 모델
- SCHEMA_V1, SCHEMA_V2: 지원 schema의 단일 상수

### 2. Strict parser and validation

Raw document는 camelCase field와 deny-unknown-fields 규칙으로 읽는다. v1 문서는
schemaVersion 1을 허용하고 catalogRevision을 None으로 정규화한다. v1 entry에
capability/action 배열이 존재하더라도 v2 routing으로 활성화하지 않고 accepts,
produces, actions를 빈 배열로 반환한다. 이로써 구버전 문서가 새 계약을 우회하지
않는다.

v2 문서는 catalogRevision이 반드시 존재해야 하며 0 또는 음수에 해당하는 값은
거부한다. 앱별로 다음 identity 계약을 검증한다.

- id가 유효한 slug이며 앱 ID가 중복되지 않는다.
- displayName과 productName이 비어 있지 않고 control character를 포함하지 않는다.
- identifier가 com.devbox. 접두사와 허용된 소문자·숫자·점 형식을 만족한다.
- cargoPackage가 유효한 slug다.
- appDir가 정확히 apps/<id> 형식이다.
- identifier, cargoPackage, appDir의 identity도 서로 중복되지 않는다.

capability는 다음 shape만 허용한다.

- accepts: path, workspace, query, profile 또는 versioned handoff
- produces: versioned handoff 또는 snapshot:<producer>/<kind>/v<n>
- 모든 배열은 중복 항목을 허용하지 않는다.
- snapshot producer는 선언한 앱 ID와 일치해야 한다.

action은 actionId, actionVersion, label, target, payloadKind의 고정 field 집합을
사용한다. actionVersion은 양수여야 하며, payloadKind는 versioned handoff여야 한다.
target 앱이 실제로 해당 payloadKind를 accepts에 선언했는지도 parser에서 검증한다.
실행 시점의 profile/job/query 상태와 secret은 catalog에 넣지 않는다.

### 3. Revision freshness and safe fallback

select_catalog는 build-time 사본을 먼저 parsing한다. build-time 사본 자체가
invalid이면 runtime을 대신 사용하지 않고 명시적인 parser error를 반환한다. 유효한
runtime의 선택 규칙은 다음과 같다.

- runtime 입력이 없으면 build-time 선택, fallback reason은 Missing
- runtime JSON이 손상되었거나 schema/entry 계약을 위반하면 build-time 선택,
  fallback reason은 Invalid
- runtime이 v1이라 revision이 없으면 build-time 선택, fallback reason은
  MissingRevision
- runtime revision이 build-time revision보다 낮으면 build-time 선택, Stale에 두
  revision을 기록
- runtime이 유효한 v2이고 revision이 build-time과 같거나 높으면 runtime 선택
- equal revision도 runtime을 선택해 Manager의 atomic replacement 결과를 보존한다.

fallback reason은 사용자에게 raw input을 보여주기 위한 것이 아니라 안전한 진단과
회귀 검증을 위한 제한된 enum이다. runtime write와 낮은 revision overwrite 방지는
후속 Manager 기능에서 이 선택 규칙을 전제로 구현한다.

### 4. Capability filtering boundary

capable_targets는 catalog entry의 accepts에 capability가 정확히 포함되는지
확인하고 AppRef만 반환한다. capable_producers는 produces를 같은 방식으로 조회한다.
이 crate는 설치된 executable, registry, custom install root, process launch를
확인하지 않는다. 실제 설치 상태와 실행 경로의 결합은 crates/launch의 책임이다.

따라서 fake-sixteenth가 path를 선언하면 기존 앱 코드나 하드코딩 allowlist를
수정하지 않아도 pure filter 결과에 나타난다. fixture에서는 path consumer가
wsl-desktop, knowledge-base, code-pad, workbench, repo-manager,
fake-sixteenth으로 재현된다.

### 5. Manager build-time compatibility

apps/devbox-manager/src-tauri/src/core/catalog.rs의 기존 private v1 구조체를
제거하고 shared catalog crate의 Catalog와 CatalogApp을 re-export한다. Manager의
parse_catalog 함수는 shared parser를 호출하고, 사용자 경계에는 안전한 String error만
반환한다.

build-time catalog include_str! 경계는 shared parser를 사용하도록 유지했다.
Manager focused test는 실제 apps/catalog.json이 schema v2 revision 1과 13개 app을
통과하는지, malformed 입력의 untrusted value가 error에 반향되지 않는지를 검증한다.
doctor command의 catalog-ids 점검은 새 shared model을 계속 사용한다.

이번 변경에는 runtime catalog 원자적 write, downgrade 방지, install-root registry,
executable discovery 또는 Manager UI가 포함되지 않는다.

### 6. Actual catalog v2

apps/catalog.json을 schemaVersion 2와 catalogRevision 1로 갱신했다. 현재 구현된
13개 release app을 identity와 release/managerVisible/selfManaged metadata와 함께
등록하고, 모든 entry에 accepts, produces, actions 배열을 추가했다.

현재 선언은 실제 구현된 기능만 광고한다.

- path accepts: wsl-desktop, code-pad, workbench
- code-pad accepts: workspace도 포함
- knowledge-base는 snapshot:knowledge-base/activity/v1을 produces
- run-manager는 snapshot:run-manager/status/v1을 produces
- 아직 handoff/action 또는 향후 계획된 capability를 구현하지 않은 앱은 빈 배열을
  유지한다.

Devbox Launcher, Log Lens, fake-sixteenth는 actual release catalog에 추가하지
않았다. 계획된 신규 앱은 각 bootstrap 기능 PR에서 catalog/workspace/CI/release
등록을 함께 처리하며, fake-sixteenth는 parser capability regression fixture에서만
사용한다.

### 7. Catalog consistency gate

.github/scripts/check-catalog.sh를 schema v2 계약에 맞게 확장했다. gate는 다음을
검사한다.

- top-level field 집합이 schemaVersion, catalogRevision, apps와 정확히 일치하는지
- schemaVersion이 2이고 catalogRevision이 positive integer인지
- app ID, directory 집합, Cargo workspace package 집합이 일치하는지
- app identity와 required field 집합이 중복·누락 없이 존재하는지
- appDir, package.json, Cargo.toml, tauri.conf.json의 metadata가 catalog와 일치하는지
- identifier 형식, release matrix와 third-party notice resource 조건이 맞는지
- accepts/produces capability shape와 중복 여부가 유효한지
- snapshot producer가 app ID와 일치하는지
- action field 집합, 양수 version, payload kind, target 수신 capability가 유효한지

기존 release/catalog 검사와 새 v2 검사를 하나의 CI gate에서 실행할 수 있도록
유지했다. CI 오류는 소스 트리의 app ID·field·metadata 불일치를 복구 가능하게
표시하며, runtime catalog 입력은 이 script가 아니라 raw 값을 반향하지 않는 Rust
`CatalogError` 경계에서 처리한다.

### 8. Dependency notices and documentation

crates/catalog workspace 등록과 devbox-manager path dependency로 Cargo.lock을
갱신했다. THIRD_PARTY_NOTICES.md의 Cargo.lock SHA-256 inventory를 재생성하고
dependency notice 검사를 통과시켰다. 새 crate의 직접 dependency는 기존 serde/
serde_json graph를 사용하며 별도 외부 runtime이나 network dependency를 추가하지
않는다.

다음 문서를 구현 상태에 맞게 갱신했다.

- AGENTS.md: 공용 crates/catalog 저장소 사실 추가
- docs/architecture.md: catalog crate의 구현 상태, revision fallback과 runtime
  file I/O 경계 반영
- THIRD_PARTY_NOTICES.md: lockfile fingerprint 갱신

## Design Decisions

### Schema compatibility

v1은 읽기 호환만 유지하고 v2 capability를 암묵적으로 추론하지 않는다. v2는
revision과 정적 capability/action 계약을 엄격히 검증한다. 이 분리는 단독 설치의
legacy catalog를 안전하게 읽으면서도 새 consumer가 불완전한 선언을 신뢰하지 않게
한다.

### Build-time-first safety

build-time catalog는 애플리케이션 바이너리에 포함된 신뢰 가능한 바닥값이다.
runtime 사본이 최신이라는 사실만으로 build-time parse 실패를 덮어쓰지 않으며, runtime
오류는 build-time 선택으로 격리한다. runtime 사본의 freshness가 통과한 경우에만
runtime 내용을 사용한다.

### Pure crate boundary

parser, normalization, validation, selection, capability filter는 OS와 filesystem
I/O가 없는 crates/catalog에 둔다. install root와 executable 해석은 crates/launch,
runtime 사본 write는 Manager 후속 기능으로 남겨 dependency 방향과 테스트 경계를
분리했다.

### Declared capability only

actual catalog는 현재 구현된 capability만 선언한다. 계획 문서에 존재하는 future
handoff/action을 미리 광고하지 않아 consumer가 아직 구현되지 않은 경로를 실행하려
하지 않도록 했다. capability filter가 새 app을 자동으로 발견하는 동작은 fake
fixture로 검증하고, release catalog에 없는 앱은 실제 배포 목록에 영향을 주지 않는다.

### Safe diagnostics

CatalogError의 Display는 schema/index/field와 제한된 revision 정보만 반환한다.
raw JSON, secret, absolute path, untrusted identifier를 오류 문자열에 삽입하지 않는다.
Manager adapter도 이 안전한 error boundary를 그대로 유지한다.

### One gate for source and release consistency

catalog 자체의 schema 검증만으로는 workspace나 release resource 불일치를 잡을 수
없으므로 check-catalog.sh에서 source tree, Cargo workspace, Tauri metadata,
third-party notices까지 함께 검사한다. dependency inventory는 lockfile fingerprint를
기준으로 재현한다.

## Fixtures

유효한 fixture 5개는 모두 정확히 16개 app entry를 포함한다. 각 app entry는
id, displayName, productName, identifier, cargoPackage, appDir, release,
managerVisible, selfManaged, accepts, produces, actions를 갖는다. 마지막 entry는
항상 fake-sixteenth이며 accepts에 path를 선언한다.

| Fixture | Schema | Revision | 검증 목적 |
|---|---:|---:|---|
| v1-legacy.json | 1 | 없음 | legacy parsing과 capability 빈 배열 normalization |
| v2-build.json | 2 | 5 | build-time revision floor |
| v2-runtime-newer.json | 2 | 6 | newer runtime 선택 |
| v2-runtime-stale.json | 2 | 4 | stale runtime build fallback |
| runtime-corrupt.json | 손상 | 해당 없음 | invalid runtime build fallback |
| fake-sixteenth.json | 2 | 5 | 16번째 entry와 path capability filter |

fixture는 crates/catalog/tests/catalog.rs에 통합되어 별도 수동 harness가 아니라
Rust regression suite에서 함께 소비된다. fake-sixteenth fixture에는 action target과
versioned payload link 검증을 재현할 수 있는 선언도 포함한다.

## Verification

### Catalog integration tests

crates/catalog/tests/catalog.rs의 11개 integration test가 모두 통과했다.

검증한 항목은 다음과 같다.

- v1 legacy revision/capability/action normalization
- v2 revision과 capability/action parsing
- fake-sixteenth를 포함한 16개 app capability filter
- repository actual catalog의 schema v2·revision 1·13개 app parsing
- missing, corrupt, v1 runtime의 build-time fallback
- stale revision 4의 build revision 5 fallback
- equal revision 5와 newer revision 6 runtime 선택
- invalid build-time catalog의 명시적 오류
- missing/zero/unsupported revision 및 schema 거부
- invalid direction, duplicate capability, broken action, duplicate app ID 거부
- duplicate identity, invalid identifier, spoofed snapshot producer 거부
- duplicate action, unknown field, empty catalog, untrusted error redaction 거부

결과: 11 passed, 0 failed.

### Manager focused tests

Devbox Manager catalog adapter의 focused test 2개가 통과했다.

- actual build-time catalog를 shared v2 contract로 읽는지 확인
- malformed input의 untrusted value가 adapter error에 포함되지 않는지 확인

결과: 2 passed, 0 failed.

### Dependency and source checks

- dependency notices regeneration: PASS
- regenerated lockfile fingerprint consistency check: PASS
- `bash .github/scripts/check-catalog.sh`: PASS
- `cargo fmt --all -- --check`: PASS
- `cargo test --workspace --all-targets -j 2`: PASS
- `cargo check --workspace --all-targets -j 2`: PASS
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: PASS
- `pnpm build`: 전용 worktree에 `node_modules`가 없어 `tsc: not found`로 즉시 중단.
  이 PR은 frontend source를 변경하지 않으며, 자원을 과도하게 사용하지 않도록
  worktree 전체 install/build를 반복하지 않고 PR CI의 frozen install·frontend 행렬에서
  완료한다.
- Windows packaged smoke: 이 WSL 기능 PR에서 로컬 미실행, issue가 지정한 W1
  checkpoint에서 확인 예정

최초 `cargo test` 기본 병렬도 실행은 13개 Tauri 링커가 동시에 메모리와 swap을
소진해 소스 오류 없이 정체되었다. 해당 프로세스만 중단한 뒤 동일 검증을
`-j 2`로 재실행해 2분 20초에 통과했다. 이후 로컬 workspace 검증은
변경 범위에 맞는 targeted check를 우선하고 전체 target 행렬은 CI에 위임한다.

이 변경 문서에는 아직 실행하지 않은 PR GitHub Actions CI, Windows smoke, merge,
release 결과를 성공으로 기록하지 않는다.

## Follow-ups

### PR·CI 남은 검증

로컬 Rust·catalog·dependency 검증은 위와 같이 완료했다. PR을 올리면 GitHub
Actions에서 다음을 최신 commit에 대해 완료해야 한다.

- frozen `pnpm install`과 frontend test/build
- Linux/Windows Rust check·test·Clippy
- catalog·dependency policy·audit·notice gate
- 모든 required check이 green인 상태에서만 merge

Windows 환경에서는 Manager build-time catalog parsing과 packaged 앱의 startup
smoke를 W1 checkpoint에서 확인해야 한다. 이 문서에 기록된 로컬 검증은
Windows packaged-runtime acceptance의 대체가 아니다.

### 후속 기능 경계

다음은 catalog v2 parser PR 이후의 별도 기능으로 유지한다.

- Manager runtime catalog의 atomic temp+rename write
- runtime catalog downgrade 방지와 revision provenance
- versioned install-root registry 및 custom root migration
- crates/launch의 installed target/executable discovery
- capability 기반 context menu와 각 앱 UI 연결
- applink protocol v2 handoff
- Devbox Launcher와 Log Lens bootstrap
- actual catalog에 신규 앱과 구현된 capability/action을 추가하는 catalog update

후속 기능은 parser가 검증한 catalog model과 freshness 규칙을 재사용하며, 현재 actual
catalog가 선언하지 않은 future capability를 임의로 활성화하지 않는다.

### Release evidence

PR 번호, CI run, merge commit과 release evidence는 아직 생성되지 않았으므로 이
문서에 기록하지 않는다. PR 전 전체 검증과 기능 PR의 CI가 완료된 뒤, 실제 실행 결과와
변경된 acceptance evidence를 이 workthrough에 추가한다.
