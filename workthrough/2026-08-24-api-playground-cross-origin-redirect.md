# API Playground Cross-Origin Redirect Hardening

## Overview

API Playground 0.3.2의 수동 redirect 처리에서 307/308 응답이 method와 함께 request
body를 다른 origin으로 전달할 수 있던 보안 공백을 닫았다. cross-origin 전환 뒤에는
method 보존 여부와 관계없이 credential, 알려진 secret이 든 header, request body와
body metadata를 전달하지 않는다. 민감정보가 destination URL 자체에 포함된 경우에는
두 번째 origin에 연결하기 전에 요청을 안전하게 차단한다.

## Context

- 기존 구현은 cross-origin redirect에서 Authorization, Cookie, API-key 계열 header와
  auth만 억제했다.
- 301/302 POST와 303은 GET으로 바뀌면서 body가 제거됐지만, HTTP 의미상 method를
  보존하는 307/308에서는 body가 그대로 남았다.
- secret이 일반 이름의 custom header 값이나 redirect destination query에 들어간
  경우에도 이름 기반 header filter만으로는 안전을 보장할 수 없었다.
- v0.4.2 RC 문서는 이 경계를 실제 코드와 회귀 테스트로 증명한 뒤 확정해야 했다.

## Architecture Decisions

### Sticky cross-origin safety state

redirect chain이 한 번이라도 다른 origin으로 넘어가면 `allow_sensitive`와
`include_body`를 모두 `false`로 유지한다. 307/308에서는 POST 등의 method만 보존하고
body는 보존하지 않는다. body를 제거한 요청에는 원 요청의 `Content-*`,
`Transfer-Encoding`, `Trailer`, `Expect`, digest header도 재사용하지 않는다.

### Value-aware header filtering

header 이름이 민감하지 않더라도 값에 request 또는 environment secret이 들어 있거나
알려진 token 패턴이 있으면 cross-origin 요청에서 제외한다. 따라서 `X-Debug` 같은
일반 custom header를 통한 우회 전달도 차단한다.

### Sensitive destination fail-closed

cross-origin destination URL을 redaction한 결과가 원 URL과 다르면 follow를 거부한다.
민감한 query key, URL userinfo, 알려진 request secret이나 token pattern이 path/query에
포함된 경우가 이에 해당한다. 오류는 URL, port, secret을 포함하지 않는 고정된 한국어
메시지만 반환한다.

## Changes Made

- `apps/api-playground/src-tauri/src/commands/request.rs`
  - cross-origin 전환 시 request body를 상태 코드와 무관하게 억제했다.
  - body가 없는 후속 요청에서 stale body metadata header를 제거했다.
  - 일반 header 값에 든 알려진 secret도 cross-origin에서 제거했다.
  - 민감한 cross-origin destination URL을 연결 전에 차단했다.
  - 로컬 두 서버를 사용하는 302, 307, 308 회귀 테스트와 destination 미접속 증명을
    추가했다.
- `apps/api-playground/README.md`
  - cross-origin body 억제와 sensitive destination fail-closed 경계를 문서화했다.

## Verification Results

집중 검토와 로컬 두 서버 회귀 테스트로 다음 경로를 확인했다.

- 307/308 POST가 method는 유지하되 body와 body metadata를 다른 origin에 보내지 않는지
- Authorization, Cookie, API-key 및 secret 값이 든 일반 custom header가 제거되는지
- 안전한 302 destination은 정상 follow하고 응답 body/header/final URL을 redaction하는지
- 민감한 destination URL은 두 번째 listener에 연결하지 않고 고정 오류로 끝나는지
- 기존 same-origin method 규칙과 전체 workspace 검증이 유지되는지

```text
API Playground Rust tests             14 passed / 0 failed
API Playground frontend tests         55 passed / 0 failed
cargo fmt --all -- --check            PASS
cargo clippy -p api-playground ...    PASS
cargo test --workspace --all-targets  PASS
cargo check --workspace               PASS
16-package frontend test workspace    PASS
16-package sequential frontend build PASS
catalog consistency                   PASS
git diff --check                      PASS
```

새 worktree에는 처음에 `node_modules`가 없어 frontend test가 `vitest: not found`로
실행 전 중단됐다. `pnpm install --frozen-lockfile`로 lockfile 변경 없이 workspace
의존성을 복원한 뒤 전체 frontend test와 build를 다시 실행해 모두 통과했다.

## Follow-up

공식 `v0.4.2-rc1` Windows package에서 307/308 body 억제와 민감 destination 차단을
H1 시나리오로 다시 확인한다. 통과한 증거만 issue #176에 secret 없이 기록하고,
그 뒤 `v0.4.2` stable tag를 승격한다.
