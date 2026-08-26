# Webhook Lab example curl (#283)

## Overview

Webhook Lab의 response rule에서 현재 실행 중인 로컬 서버를 호출할 수 있는 example curl을
명시적으로 복사할 수 있게 했다. Windows desktop 사용자를 위한 `PowerShell curl.exe`와
WSL/POSIX 사용자를 위한 `POSIX sh curl`을 별도 메뉴 항목으로 제공하며, formatter와
clipboard 경계를 모두 fail-closed로 유지한다. `cmd.exe` 생성, raw secret reveal, request
replay, network/IPC 추가는 범위에 포함하지 않았다.

## Context

이전 context-menu 초안에는 example curl 위치만 예약되어 있고 단일 disabled 항목만 있었다.
Rule의 `ServerStatus.address`와 method/path는 이미 앱에 존재했지만, 메뉴를 연 뒤 서버가
중지되거나 bind address가 바뀔 수 있으므로 복사 시점의 fresh 값을 재검증해야 했다. 다음 위험을 먼저
해결하지 않으면 자동 문자열 조합이 잘못된 route를 호출하거나 응답 rule의 민감한 값을
복사할 수 있었다.

- 서버가 메뉴를 연 뒤 중지되거나 bind 주소가 달라질 수 있음
- backend는 path 마지막의 `*`만 trailing prefix wildcard로 취급함
- response rule의 status/headers/body/delay는 request input이 아닌 반환 metadata임
- shell quoting, curl URL glob/dot-segment 정규화, path/query token, mixed placeholder가
  secret 또는 다른 route로 이어질 수 있음
- malformed rule/URI/parser/clipboard 예외를 그대로 DOM에 표시하면 원문이 노출될 수 있음

## Changes Made

### 1. Pure formatter and shell contracts

File: `apps/webhook-lab/src/lib/exampleCurl.ts`

- `buildExampleCurl(rule, address, shell)`을 추가하고 모든 예외를 `null`로 감싸 UI가
  parser/URI 오류를 반향하지 않도록 했다. 기존 호출 호환을 위해 기본 shell은 POSIX다.
- `posixShellQuote`와 `powershellQuote`를 독립 구현했다. POSIX는 single quote를
  close/escape/reopen하고, PowerShell은 single quote를 두 개로 바꾼다. `cmd.exe`는
  허용하지 않는다.
- command는 다음처럼 request method/path만 request argument로 포함한다.

  ```text
  curl --globoff --path-as-is --include --request POST 'http://127.0.0.1:9000/events/example'
  curl.exe --globoff --path-as-is --include --request POST 'http://127.0.0.1:9000/events/example'
  ```

  `--globoff`는 wildcard expansion을 끄고 `--path-as-is`는 curl의 dot-segment path
  정규화를 막는다. `--include`는 실제 서버 response를 출력한다.
- response rule의 status, response headers, response body, delay는 명시적인 주석 metadata로
  출력한다. 이 값을 `--header`나 `--data`로 보내지 않으며, `null` method는 backend의
  empty method/all semantics를 보존하기 위해 POST 예시와 설명 주석으로 표시한다.
- trailing `*`는 backend `path.starts_with(rule.path[..len-1])`와 맞도록 마지막 `*`를
  제거하고 `example`을 붙인다. `/events/*`는 `/events/example`이 되며, wildcard가 아닌
  path와 query는 trim/decode/re-encode하지 않는다.

### 2. Address, URI, and privacy boundaries

`exampleCurl.ts`의 formatter는 다음 정책을 적용한다.

- fresh `serverStatus`에서 loopback `127.0.0.1`, `localhost`, `[::1]`만 허용하고,
  wildcard bind `0.0.0.0`, `[::]`는 각각 loopback destination으로 canonicalize한다.
  외부 IPv4/IPv6와 bracket 없는 IPv6는 null이다.
- path에는 absolute URL/`//` host escape/fragment/공백·control/malformed percent를
  허용하지 않는다. percent-decoded path/query에도 whitespace/control을 적용하고,
  encoded placeholder와 known token도 검사한다. sensitive query를 masking하면 exact
  backend route가 달라지므로 해당 rule 전체를 중단한다.
- header/body 값은 exact whole-value `${NAME}` 또는 `{{NAME}}`만 보존한다. `Bearer
  ${TOKEN}`와 `prefix ${TOKEN}` 같은 mixed 값은 전체 `[REDACTED]`이며, path와 JSON
  object key의 placeholder는 거부한다. Authorization/Cookie/API key/token/password
  계열 값과 known token/PEM 패턴은 redact한다.
- formatter의 status(100~599), delay(0~60,000ms), path(4,096), headers(100개, name
  256, value 16,384, raw 합계 64,000), body(256,000), JSON(depth 32/node 10,000/
  value·object key string 64,000), 최종 출력(512,000) bounds를 고정했다.

### 3. Context menu and async action safety

Files: `apps/webhook-lab/src/lib/contextMenus.ts`, `apps/webhook-lab/src/App.tsx`

- Rule menu를 `PowerShell curl.exe 복사`와 `POSIX sh curl 복사` 두 action으로 분리했다.
  서버가 현재 실행 중이 아니거나 address가 없으면 disabled이며, 실제 click 직전에
  `serverStatus()`와 `listRules()`를 다시 읽어 바뀐 address를 fresh 값으로 검증한다.
- fresh status가 stopped/주소 없음, 선택 rule 삭제, malformed rule/address이면 clipboard
  write를 하지 않고 고정 안내를 표시한다. 선택 rule의 ID를 다시 찾기 때문에 stale
  context snapshot이 새 rule을 가리키지 않는다.
- operation ref와 busy state로 pending clipboard/action 중 두 번째 action을 차단한다.
  기존 history/rule mutation clipboard도 같은 busy 경계를 사용한다.
- clipboard와 backend exception은 allowlist 외 원문을 generic message로 바꾸며, error
  element에는 `role="alert"`/assertive live region을 적용했다. 메뉴 공용 package의
  Shift+F10/Menu key와 Escape focus restore 계약을 유지한다.

### 4. Tests and documentation

Files:

- `apps/webhook-lab/src/lib/exampleCurl.test.ts`
- `apps/webhook-lab/src/lib/contextMenus.test.ts`
- `apps/webhook-lab/src/App.test.tsx`
- `apps/webhook-lab/README.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

Formatter tests include deterministic POSIX/PowerShell goldens, independent quote
metacharacter behavior, response-metadata-only output, trailing wildcard samples, exact/mixed
placeholder and token masking, sensitive query fail-closed, IPv4/IPv6/wildcard destination
policy, URI/curl normalization, every declared bound, malformed input and JSON key rejection.
App/menu fixtures cover fresh running address, PowerShell action, stale rule, server stop,
clipboard failure without raw DOM echo, busy/double action, disabled state, and keyboard focus
restore. README and design/release plan document the two shell contract and all safety bounds.

## Verification Results

### Static check

```text
$ git diff --check
exit 0
```

### Frontend fixtures

PR-wide review reused the existing workspace dependencies without running an install. The first
targeted run exposed six invalid or mismatched assertions and also led to a formatter review that
closed the fragment-path and desktop-only safe-message gaps. After those fixes:

```text
$ pnpm --dir apps/webhook-lab test -- --maxWorkers=2
Test Files  3 passed (3)
Tests       29 passed (29)
exit 0
```

### PR-wide local gates

The final branch was rebased onto `main` after #280 and verified with bounded frontend
concurrency:

```text
$ pnpm -r --workspace-concurrency=2 test -- --maxWorkers=2
17 workspace projects passed, including webhook-lab 3 files / 29 tests

$ pnpm -r --workspace-concurrency=2 build
17 workspace projects passed

$ source ~/.cargo/env && cargo test --workspace
all workspace unit, integration, and doc tests passed

$ source ~/.cargo/env && cargo fmt --all --check
$ source ~/.cargo/env && cargo check --workspace
$ source ~/.cargo/env && cargo clippy --workspace --all-targets -- -D warnings
exit 0
```

The repository policy gates also passed: frontend audit reported no known vulnerabilities,
dependency-policy and release-manifest regressions passed, catalog consistency passed, and
`cargo deny --locked check` reported advisories/bans/licenses/sources OK with the repository's
existing duplicate-version warnings. CI scope detection selected only the `webhook-lab`
frontend and no Rust packages for this branch.

## Remaining Risks and Follow-up

- Windows packaged WebView2 clipboard and actual PowerShell/curl.exe execution still require the
  normal W1 desktop smoke check; no external command is launched by this feature.
- A server can stop in the small interval after fresh status is read; the generated command is
  still a local, user-triggered sample and no replay/network action is performed. A backend
  generation/version token would be needed for transactional status guarantees.
- Token detection is intentionally conservative for known token forms and sensitive field names;
  arbitrary domain-specific secrets cannot be identified without changing the response rule data
  model. Such values remain response comments, never request arguments.
- The worktree intentionally remains a dirty draft. No commit, push, or PR was created.
