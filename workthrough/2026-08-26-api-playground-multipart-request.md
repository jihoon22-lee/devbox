# API Playground Multipart Request

## Overview

Issue #270의 P1-09 일곱 번째 범위로 API Playground에 `multipart/form-data` 요청을 추가했다.
사용자는 Body 탭에서 ordered text/file part를 편집하고 데스크톱 file picker로 파일을 선택할 수
있다. 파일 bytes를 앱에 복사하거나 저장하지 않고 Rust backend가 전송 시점에 bounded stream으로
읽는다.

```text
MultipartEditor
  ├─ text part ─► template reference ─► backend-only resolve ─► reqwest text Part
  └─ file part ─► Tauri single-file picker
                       │
                       ├─ UI: basename만 표시
                       ├─ persistence: path 제거 + basename만 보존
                       └─ send: canonicalize + metadata bounds + streamed file Part

History / Collection / masked cURL
  ├─ raw path 없음
  ├─ raw file bytes/backup 없음
  ├─ sensitive text literal masking
  └─ 저장된 file part는 재선택 필요
```

Windows packaged file-picker/send smoke는 계획대로 W1 P1 묶음 checkpoint에서 수행한다. 이 PR은
Linux/WSL에서 검증 가능한 순수 모델, persistence, loopback wire, compile과 policy evidence를
완료한다.

## Scope

### Included

- Body kind `multipart`
- ordered text/file part와 enabled 상태
- part name, text value/file picker, part별 Content-Type
- 현재 environment의 봉인된 secret 이름 reference picker
- immutable add/update/kind switch/duplicate/remove
- 50 part, text 1,000,000 UTF-8 bytes, file 25 MiB each/50 MiB total bounds
- native file existence, regular-file와 read failure validation
- `reqwest::multipart` streaming request와 derived boundary
- History·Collection legacy normalization, masking, path removal와 deep clone
- browser text-only `FormData` preview
- masked cURL placeholder와 confirmed backend one-shot revealed cURL
- response/error redaction seed와 redirect body suppression
- dependency policy와 generated third-party notices

### Excluded

- response header/cookie viewer
- OpenAPI/GraphQL/SSE/WebSocket
- raw file body 또는 file byte backup
- directory/multiple/save dialog
- browser file stream과 part별 Content-Type emulation
- storage schema version bump 또는 새 localStorage key
- Windows packaged smoke의 개별 실행

## Data Contract

Frontend와 Rust template에 다음 shape를 추가했다.

```ts
interface MultipartPart {
  kind: "text" | "file";
  name: string;
  value: string;
  file_path: string;
  file_name: string;
  content_type: string;
  enabled?: boolean;
}
```

- `value`는 text에서만 사용한다.
- `file_path`는 현재 실행의 picker→request command 경로이며 persistence에서 항상 빈 문자열이다.
- `file_name`은 경로 구분자와 제어 문자를 제거한 최대 255자의 basename metadata다.
- `enabled` 누락은 기존 header/cookie와 같이 true로 읽는다.
- 기존 History·Collection v2에서 `multipart` 누락은 빈 배열로 정규화한다.
- 다른 body kind의 stale multipart 배열은 전송·reference resolve·file read에 사용하지 않는다.

`body_kind === "multipart"`일 때 과거 textarea의 `body`는 전송하지 않으며 저장 전 빈 문자열로
지운다. file bytes를 body에 fallback하거나 backup하는 경로는 만들지 않았다.

## Editor and Validation

`MultipartEditor`의 각 행은 다음 control을 제공한다.

- enabled checkbox
- Text/File kind select
- part name
- text value 또는 file select button
- optional Content-Type
- text 전용 secret reference select
- duplicate/delete
- 행별 validation message

file picker 결과의 전체 경로는 button text/title 또는 오류에 표시하지 않는다. picker가 반환한
basename도 다시 경로 구분자·제어 문자를 제거한 뒤 표시한다. picker 오류는 원문 exception 대신
고정 권한 안내만 표시한다.

Frontend와 backend의 공통 의미 경계는 다음과 같다.

| Boundary | Limit / behavior |
|---|---|
| Part rows | 배열 최대 50개 |
| Empty row | enabled라도 모든 입력이 비면 전송 제외 |
| Part name | 필수, 최대 120 bytes/chars의 ASCII HTTP token |
| Content-Type | 선택, 최대 127자, parameter 없는 `type/subtype` token |
| Text | 활성 part 전체 UTF-8 1,000,000 bytes |
| File | runtime path 필수, regular file, each 25 MiB, total 50 MiB |
| Disabled | shape는 저장하되 resolve/read/send/cURL/redaction seed에서 제외 |

저장본의 file part는 path가 없고 basename이 있으므로 `'<basename>' 파일을 다시 선택하세요.`를
표시하고 Send/cURL UI를 막는다. 복제된 file part는 같은 runtime path를 임시로 공유하지만 저장
경계에서는 두 행 모두 제거된다.

## Native Send Boundary

Rust command는 다음 순서로 fail-closed한다.

1. unresolved template의 row count/name/kind/Content-Type/text byte/path presence 검증
2. 활성 reference 이름만 수집하고 secret을 backend memory에서 해제
3. resolved text의 byte bound와 shape 재검증
4. 활성 file path canonicalize
5. metadata로 regular file, per-file와 aggregate byte bound 확인
6. response/error redactor seed 구성
7. URL/client 구성 후 multipart form 생성·전송

`reqwest::multipart::Part::file`을 사용하므로 frontend나 persistence가 file bytes를 보유하지 않는다.
Text part는 `Part::text`, optional Content-Type은 `mime_str`로 적용한다. file은 canonical path의 실제
basename을 사용하므로 frontend가 전달한 임의 filename metadata를 wire filename으로 신뢰하지
않는다.

사용자 header의 `Content-Type`, `Content-Length`, `Transfer-Encoding`은 multipart 요청에서
제외한다. reqwest가 boundary와 body length/transfer semantics를 일관되게 만든다. loopback fixture는
사용자가 넣은 `Content-Type: text/plain`이 wire top-level header가 되지 않고
`multipart/form-data; boundary=...`가 생성되는지 확인한다.

Same-origin 307/308처럼 body를 보존하는 redirect는 각 hop에서 form/file stream을 새로 구성한다.
모든 cross-origin redirect는 기존 API Playground 정책대로 body와 derived body header를 억제한다.
파일 오류는 missing/read/each-size/total-size의 고정 문구만 반환하고 path나 filename을 포함하지
않는다.

## Secret and Persistence Boundary

Text part reference는 활성 `kind=text` 행의 value에서만 찾고 해제한다. disabled 행의 손상된 sealed
envelope는 열지 않는다. 민감한 part name(`token`, `password`, `secret`, `api_key` 등)의 resolved
text는 response/body/error redaction seed가 된다.

Frontend persistence sanitizer는 다음을 수행한다.

- file `file_path`는 enabled와 무관하게 `""`
- file `value`는 항상 `""`
- file `file_name`은 safe basename + known token pattern redaction
- multipart mode의 stale `body`는 `""`
- sensitive text 직접값과 mixed reference는 `[REDACTED]`
- sensitive text의 exact single `${NAME}`/`{{NAME}}` reference만 보존
- 알려진 GitHub/AWS/JWT/private-key token pattern redaction
- 값 변경 시 `requiresSecretReview = true`

Backend persistence sanitizer는 `multipart` array를 allowlisted field만으로 재구성한다. 따라서
crafted `raw_backup`, byte data나 임의 추가 field도 제거한다. multipart request object의 `body`도
강제로 비운다. 현재 environment의 sealed plaintext가 file basename이나 text에 포함되면 기존
backend secret scan이 제거하고 serialized 결과에 남지 않았음을 재검증한다.

History·Collection duplicate는 sanitized multipart array를 deep clone한다. 저장본을 request editor로
불러오면 file path를 복구하려고 추측하거나 filesystem을 검색하지 않고 사용자의 재선택을 요구한다.

## cURL and Browser Preview

기본 cURL은 먼저 persistence sanitizer를 거친다.

- text: `--form 'name="masked-or-reference";type=...'`
- file: `--form 'name=@"[RESELECT_FILE:basename]";type=...'`
- 전체 path와 raw file bytes 없음
- user multipart derived header 없음

사용자가 민감정보 경고를 확인한 뒤 호출하는 backend one-shot cURL만 resolved text와 현재 canonical
file path를 사용한다. 결과는 저장하지 않고 clipboard에 한 번 전달한다. missing file은 revealed
cURL 생성 전에도 안전 오류로 차단한다. curl form parser가 쉼표·세미콜론·`@`·quote를 shell과
별도로 해석하므로 text와 file path는 항상 form double-quote하고 내부 backslash/double-quote를
escape한 뒤 전체 argument를 shell single-quote한다.
[curl `--form` 공식 문서](https://curl.se/docs/manpage.html#-F)의 comma/semicolon 및 quoted
filename/path 규칙을 따른다.

Tauri 밖의 browser preview는 secret environment가 없는 text-only multipart를 `FormData`로 보낸다.
File part 또는 part별 Content-Type은 native semantics와 안전한 path access를 보장할 수 없어 고정된
데스크톱 전용 오류를 반환한다. FormData boundary를 브라우저가 만들도록 user `Content-Type`은
제외한다.

## Dependency Decision

### Added or activated

- `@tauri-apps/plugin-dialog 2.7.2`
- `tauri-plugin-dialog 2.7.2`
- reqwest 0.13.4 existing dependency의 `multipart`, `stream` features
- transitive `tauri-plugin-fs 2.5.1`, `rfd 0.16.0`, `mime_guess 2.0.5`

### Rationale and boundary

공식 Tauri dialog plugin은 Windows/Linux native picker의 permission, cancel과 path 전달을 제공한다.
platform별 picker를 수제로 구현하거나 사용자가 외부 도구에서 경로를 복사하게 하는 것보다 유지·
권한·사용성 위험이 작다. capability는 main window의 `dialog:allow-open` 하나뿐이고 save/directory/
multiple API는 사용하지 않는다. plugin과 reqwest feature는 설치물에 정적으로 포함되며 실행 후
download, sidecar와 network lookup이 없다.

Plugin과 tauri-plugin-fs는 Apache-2.0 OR MIT, rfd와 mime_guess는 MIT다. generated notices에 exact
version/source/checksum을 기록했고 policy check와 cargo-deny를 통과했다. runtime package graph는
Rust 662→666, frontend 156→157이며 notices는 133,921→135,107 bytes(+1,186)다.

### Bundle and memory

동일 Node/Vite toolchain의 #269 main 대비 production bundle은 다음과 같다.

| Asset | Main | Feature | Delta |
|---|---:|---:|---:|
| JS exact | 241,119 B | 251,115 B | +9,996 B |
| JS gzip | 75,109 B | 77,644 B | +2,535 B |
| CSS exact | 11,300 B | 12,111 B | +811 B |
| CSS gzip | 2,655 B | 2,750 B | +95 B |

File bytes는 frontend state나 Rust `String` body로 materialize하지 않는다. metadata 검사는 bounded
scalar만 유지하고 reqwest file Part가 stream한다. file당 25 MiB/전체 50 MiB 상한은 요청 크기와
재전송 비용을 제한한다. Windows packaged binary/installer exact delta는 W1 checkpoint에서 잰다.

## Verification

### Frontend

- `pnpm --filter api-playground build` — passed
- full API Playground Vitest snapshot: 12 files, 105 tests — passed; final additions 후 영향 범위
  App/MultipartEditor/multipart/environment/persistence/collections 6 files, 81 tests — passed (현재
  suite 110 tests, 나머지 파일 불변)
- focused multipart model/editor/persistence/environment/cURL tests — passed
- production bundle main/feature exact + gzip comparison — recorded

Coverage includes immutable row operations, normalization, max rows, name/Content-Type/text byte errors,
cross-platform basename, picker path non-display, picker failure redaction, secret reference insertion,
saved-file reselection, legacy multipart default, file path/body removal, sensitive text masking,
environment application to text only and masked cURL placeholders.

### Rust

- `cargo test --manifest-path apps/api-playground/src-tauri/Cargo.toml --jobs 1` — 26 passed
- `cargo check --manifest-path apps/api-playground/src-tauri/Cargo.toml --jobs 1` — passed
- `cargo clippy --manifest-path apps/api-playground/src-tauri/Cargo.toml --all-targets --jobs 1 -- -D warnings` — passed
- `cargo fmt --all --check` — passed

Multipart integration fixtures cover text + file loopback wire, derived boundary, per-part Content-Type,
missing-file path-free error, disabled secret reference skip, legacy default and backend safe persistence.
기존 cookie/header/redirect/redaction/network tests 22건도 함께 통과했다.

### Policy and repository gates

- `pnpm install --frozen-lockfile` — passed
- `pnpm audit --audit-level moderate` — no known vulnerabilities
- dependency policy check and regression tests — passed
- generated notices byte match — passed
- build-manifest notice tests — passed
- catalog check — passed
- `cargo deny --locked check` — advisories/bans/licenses/sources passed
- `git diff --check` — passed

`cargo deny`의 duplicate reports는 기존 allow/warn 상태이며 새 license, source 또는 advisory 실패는
없다. 전체 workspace frontend/Rust compile은 lockfile 때문에 CI scope가 all로 선택되므로 PR의
Linux/Windows GitHub Actions에서 최종 확인한다. 로컬은 사용자 요청에 따라 한 worker와
`CARGO_BUILD_JOBS=1`, Linux target cache, Node 768 MiB cap을 사용했다.

## Remaining Checkpoints and Next Scope

- W1: packaged Windows native picker cancel/select, text+file loopback send, missing/deleted file 오류,
  History/Collection reload 후 재선택, masked/revealed cURL과 process/log cleanup evidence
- 다음 독립 P1-09 issue: response header/cookie viewer
- GraphQL/SSE/WebSocket, OpenAPI, raw file backup은 #270에 포함하지 않으며 각 계획 issue를 유지한다.
- API Playground 0.4.0 version bump는 Wave 9 release preparation에서 별도로 수행한다.
