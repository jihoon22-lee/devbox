# API Playground Response Header and Cookie Tabs

## Overview

Issue #271의 P1-09 여덟 번째 범위로 API Playground 응답 영역을 Body/Headers/Cookies 전용 탭으로
나눴다. 일반 화면과 기본 복사는 마스킹된 DTO만 사용한다. 원문 response header는 데스크톱 Rust
backend의 현재 응답 전용 bounded memory vault에만 보관하며, 사용자가 별도 경고를 확인한 뒤
clipboard로 복사할 때만 Tauri command 경계를 통과한다.

```text
reqwest response headers
        │
        ├─ redact + Cookie projection ─► ApiResponse DTO ─► Viewer / masked copy
        │
        └─ max 100 rows / 64 KiB ──────► current-response vault
                                                │
                                   confirm + opaque response ID
                                                │
                                                └─► clipboard only

new request ─► previous vault entry removed
stale concurrent response ─► raw store rejected
non-text / overflow ─► partial raw copy disabled
```

Windows packaged UI와 native clipboard smoke는 계획된 W1 P1 묶음 checkpoint에서 수행한다. 이
작업은 Linux/WSL에서 검증 가능한 Rust capture/vault 계약, React UI, accessibility, masking,
loopback fixture와 production build를 완료한다.

## Scope

### Included

- 응답 Body/Headers/Cookies 탭과 공통 status·duration·size header
- Body JSON pretty toggle과 마스킹된 body 복사
- 응답 header의 ordered name/value table과 마스킹 복사
- `Set-Cookie` name, 항상 `[REDACTED]`인 value, bounded attribute 표시
- 현재 native 응답 1건의 raw header memory vault
- raw headers와 raw Set-Cookie의 별도 확인·복사 command
- 100 header / raw line 합계 64 KiB 상한과 partial-copy fail-closed
- 비텍스트 header, stale response ID, concurrent completion과 ID 소진 경계
- tab roving focus, ArrowLeft/ArrowRight/Home/End keyboard navigation
- browser preview의 bounded header DTO와 Fetch `Set-Cookie` 제약 안내
- 앱 README, architecture, roadmap, opportunity와 상세 계획 문서 동기화

### Excluded

- persistent cookie jar, cookie 수정·재전송과 domain/path matching
- response body streaming, download 또는 binary/hex viewer
- GraphQL/SSE/WebSocket/OpenAPI import
- response 원문의 History·Collection·localStorage 저장 또는 export
- raw response 전체 dump와 자동 clipboard fallback
- 새 dependency, schema version, localStorage key 또는 network service
- Windows packaged smoke의 개별 실행

## Response DTO Contract

기존 `ApiResponse`에 다음 필드를 추가했다.

```ts
interface ApiResponse {
  // existing status/body/header/redirect fields
  cookies: ResponseCookie[];
  response_id: string | null;
  raw_headers_available: boolean;
  headers_truncated: boolean;
}

interface ResponseCookie {
  name: string;
  value: string;          // 일반 DTO에서는 항상 [REDACTED]
  attributes: KeyValue[];
}
```

`response_id`는 storage key나 서버 identifier가 아닌 process-local opaque string이다.
`raw_headers_available`은 현재 native vault에 완전한 원문이 있고 ID가 여전히 유효할 때만 true다.
상한 초과 또는 비텍스트 값이 있으면 `headers_truncated`가 true이고 `response_id`는 null인 채로
반환된다. 따라서 frontend는 partial 원문이나 화면 DTO를 조합해 원문처럼 제공하지 않는다.

Browser `fetch` 경로도 header를 100행·64 KiB로 제한하지만, 브라우저가 forbidden response header인
`Set-Cookie`를 노출하지 않으므로 `cookies=[]`, `response_id=null`,
`raw_headers_available=false`다. Cookies 빈 상태에 이 제약을 명시한다.

## Native Capture and Vault

`capture_response_headers`는 reqwest `HeaderMap`의 반복 순서를 유지하며 각 name/value를 한 번만
순회한다.

| Boundary | Behavior |
|---|---|
| Header count | 최대 100개 |
| Raw line bytes | `name + ": " + value` 합계 최대 64 KiB |
| Sensitive name | value 전체 `[REDACTED]` |
| Location | URL query/userinfo와 알려진 request secret redaction |
| Other text value | request/environment secret과 알려진 token pattern redaction |
| Non-text value | safe placeholder만 DTO에 두고 raw 전체 비활성화 |
| Overflow | 가능한 safe placeholder까지만 표시하고 raw 전체 비활성화 |

`ResponseHeaderVault`와 raw entry는 `Serialize`/`Debug`를 구현하지 않는다. raw name/value는 기존
`zeroize::Zeroizing<String>`으로 감싸 partial capture, 교체와 app 종료 때 backing buffer를 지운다.
Tauri managed state의 `Mutex` 안에는 가장 최근 요청의 bounded `(name, value)` 목록 하나만 둔다.
요청 시작 시 다음 순서로 동작한다.

1. lock을 획득한다.
2. 이전 current ID와 raw entry를 먼저 폐기한다.
3. monotonically 증가하는 새 opaque ID를 예약한다.
4. 요청이 끝날 때 그 ID가 아직 current인 경우에만 완전한 raw 목록을 저장한다.

동시 요청 A 뒤에 B가 시작되면 A가 늦게 끝나도 B의 current ID를 덮어쓰지 못한다. B가 validation
또는 network 오류로 끝나더라도 A의 과거 원문을 복구하지 않는다. 이론적인 `u64` ID 소진에서도
새 ID 발급을 실패하기 전에 이전 entry를 제거해 fail-closed한다.

`copy_raw_response_headers`는 현재 ID의 전체 header line을, `copy_raw_response_cookies`는 이름이
`Set-Cookie`인 line만 결합한다. 쿠키가 없는 cookies-only 요청, stale/unknown ID, poisoned lock은
고정된 안전 오류 하나로 반환한다. backend의 raw 오류·header value·ID 내부 상태를 UI 오류에
포함하지 않는다.

## Cookie Projection

각 `Set-Cookie`는 first `name=value` segment에서 이름만 파싱한다. 이름은 최대 120자의 HTTP token만
허용하고, 그 외에는 `(unparsed)`로 표시한다. 이름 자체가 알려진 secret/token과 일치할 때도
redactor를 통과한다. value는 예외 없이 `[REDACTED]`다.

attribute는 최대 20개, 이름은 최대 64자의 HTTP token으로 제한한다.

- `Domain`, `Path`, `Expires`, `Max-Age`, `SameSite`, `Priority`: redactor 적용 후 최대 256자
- `Secure`, `HttpOnly` 같은 flag: 빈 value로 이름만 표시
- 알 수 있는 이름이지만 allowlist 밖의 attribute value: `[REDACTED]`
- 잘못된 attribute 이름 또는 상한 밖 항목: 표시하지 않음

이 projection은 cookie jar가 아니며 expires/domain/path semantics를 실행하지 않는다. 사용자에게
원문 value를 기본 화면에 노출하지 않으면서 debugging에 필요한 cookie 존재와 안전한 metadata만
제공한다.

## Viewer and Clipboard Flow

응답 status, status text, duration과 size는 어떤 탭에서도 보이는 공통 header에 둔다.

- Body: JSON response의 pretty toggle, 현재 마스킹된 body 복사
- Headers: ordered two-column table, 마스킹 header 복사, 확인 후 원문 header 복사
- Cookies: name/value/attributes table, 마스킹 cookie 복사, 확인 후 원문 Set-Cookie 복사

원문 버튼은 native vault availability, non-null response ID와 해당 row 존재 조건을 모두 만족해야
활성화된다. 클릭하면 session/token/Cookie가 포함될 수 있다는 경고를 먼저 보여준다. 취소 시 backend
command와 clipboard를 호출하지 않는다. 승인 뒤 command 결과는 React state, History, Collection,
localStorage, telemetry나 log에 넣지 않고 즉시 `navigator.clipboard.writeText`에 전달한다. command
또는 clipboard 오류는 raw exception을 버리고 고정된 안전 오류만 표시한다.

각 tab button은 `role=tab`, `aria-selected`, `aria-controls`와 roving `tabIndex`를 사용한다. 좌우
화살표는 순환하고 Home/End는 첫/마지막 탭으로 이동한 뒤 focus도 함께 옮긴다. 새 response object가
들어오면 Body 탭과 raw-copy busy state를 초기화해 과거 응답 조작 상태가 이어지지 않는다.

## Dependency and Resource Decision

새 frontend/Rust dependency, capability, lockfile, third-party notice와 runtime download는 없다. 기능은
기존 React, Tauri command, reqwest, secrets redactor만 사용하며 설치 후 오프라인으로 동작한다.

동일 Node/Vite toolchain의 #270 main 대비 production bundle은 다음과 같다.

| Asset | Main | Feature | Delta |
|---|---:|---:|---:|
| JS exact | 251,115 B | 257,010 B | +5,895 B |
| JS gzip | 77,644 B | 79,372 B | +1,728 B |
| CSS exact | 12,111 B | 13,282 B | +1,171 B |
| CSS gzip | 2,750 B | 2,912 B | +162 B |

Raw header memory는 현재 응답 1건과 64 KiB 원문 상한으로 제한되고 drop 때 zeroize한다. masked
DTO/Cookie projection도 100 header와 attribute별 상한을 공유한다. 별도 worker, daemon, polling,
sidecar나 background task를 추가하지 않았다. Windows packaged binary/installer와 clipboard runtime
evidence는 W1 checkpoint에서 기록한다.

## Verification

### Frontend

- full API Playground Vitest: 13 files, 114 tests — passed, single worker
- `ResponseViewer.test.tsx`: 4 tests — passed
- `App.contextMenu.test.tsx`: 6 tests — passed
- `pnpm --filter api-playground build` — passed
- TypeScript compile and Vite production build — passed
- production main/feature exact + gzip comparison — recorded

Viewer coverage는 전용 탭, arrow-key focus, 기본 masked copy, raw value 비노출, confirm 이전 command
미호출, 승인 뒤 raw clipboard 전달, stale/truncated disabled state와 backend raw exception 비노출을
포함한다. 기존 App/context-menu mock에도 새 command를 추가해 unrelated interaction 회귀를 막았다.

### Rust

- `cargo test -p api-playground --jobs 1` — 28 passed
- `cargo check -p api-playground --jobs 1` — passed
- `cargo clippy -p api-playground --all-targets --jobs 1 -- -D warnings` — passed
- `cargo fmt --all --check` — passed

Rust coverage는 repeated Set-Cookie masking, 안전/미지 attribute projection, 정확한 raw vault 내용,
stale ID 거부, header size overflow, ID 소진 시 이전 entry 폐기와 localhost cross-origin fixture의
raw Cookie 분리를 포함한다. 기존 request validation, redirect, secret redaction, cookie, multipart
fixture도 같은 suite에서 통과했다.

### Repository Review

- `git diff --check` — passed
- 새 dependency/lockfile/capability/storage schema 변화 없음 — confirmed
- raw response value의 DTO/debug/serialize/persistence/log 경로 부재 — reviewed
- 변경 전체와 issue acceptance를 PR 직전에 직접 재검토 — completed

로컬 검증은 사용자 요청에 따라 `CARGO_BUILD_JOBS=1`, `--jobs 1`, Vitest single worker와 Node
768 MiB heap cap으로 순차 실행했다. 전체 workspace Linux/Windows build, dependency/catalog/security
gate는 PR GitHub Actions에서 최종 확인한다.

## Remaining Checkpoints and Next Scope

- W1: packaged Windows에서 Body/Headers/Cookies rendering, keyboard navigation, masked copy, confirm
  cancel/approve, clipboard permission/error, 새 요청 뒤 stale raw 차단 evidence
- W1: localhost native response의 multiple Set-Cookie와 100행/64 KiB 경계 smoke
- GraphQL/SSE/WebSocket/OpenAPI, binary body viewer와 persistent cookie jar는 #271에 포함하지 않는다.
- API Playground 0.4.0 version bump는 Wave 9 release preparation에서 별도로 수행한다.
- 다음 P1-09 독립 issue는 계획 순서에 따라 별도 branch/PR에서 착수한다.
