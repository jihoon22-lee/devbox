# Dependency Lens Remote Enrichment

## Overview

Repo Manager의 Dependency Lens에 선택적 OSV/deps.dev 보강 경계를 추가했다. 기존 offline
lock graph 분석은 항상 먼저 실행되며, 사용자가 정확한 전송 좌표를 확인하고 별도로 승인한
경우에만 bounded remote metadata 조회가 시작된다. 원격 결과는 Repo Manager에만 남고
Workbench의 `dependency-summary/v1` offline aggregate 계약은 변경하지 않는다.

## Context

- 별도 Dependency Lens 앱을 만들지 않고 Repo Manager/Workbench 내부 패널에서 먼저 검증한다.
- 사용자가 package 이름·정확한 resolved version의 외부 전송 여부를 판단할 수 있어야 하며,
  repository path, project identity, lockfile/graph/source/integrity/credential은 전송하면
  안 된다.
- OSV와 deps.dev의 공식 API 계약은 설계 문서
  [`docs/superpowers/specs/2026-08-30-dependency-lens-enrichment.md`](../docs/superpowers/specs/2026-08-30-dependency-lens-enrichment.md)에
  인용된 [OSV querybatch](https://google.github.io/osv.dev/post-v1-querybatch/),
  [OSV schema](https://ossf.github.io/osv-schema/#affectedpackage-field),
  [deps.dev API v3](https://docs.deps.dev/api/v3/)를 기준으로 삼았다.

## Changes Made

### 1. Two-step preview and execution boundary

- 기존 offline 분석 뒤 OSV/deps.dev 선택과 **전송 내용 검토**를 분리했다. preview 단계는
  network를 호출하지 않고 service host, 정확한 coordinate, cache 상태, 생략 수와 요청 수를
  보여 준다.
- **검토한 정보 보내기** 단계는 5분 TTL·일회성 opaque token을 소비한다. native는 repository
  canonical identity와 lock revision을 다시 확인한 뒤 preview에 저장된 service/coordinate
  계획만 실행한다. repository, 분석, service 선택이 바뀌거나 token이 만료되면 실행하지
  않는다.
- `apps/repo-manager/src/api.ts`
- `apps/repo-manager/src/components/DependencyLensPanel.tsx`
- `apps/repo-manager/src/components/DependencyLensPanel.test.tsx`
- `apps/repo-manager/src/App.css`

### 2. Bounded service adapters and result projection

- `apps/repo-manager/src-tauri/src/core/dependency_enrichment.rs`
  - Cargo/pnpm/npm/Python/uv 좌표를 OSV ecosystem 또는 deps.dev system으로 매핑하고
    중복 좌표를 deduplicate한다. OSV는 최대 256개 resolved coordinate, deps.dev는 최대
    48개 direct coordinate만 선택한다.
  - advisory IDs, SPDX-like license strings, default version, deprecation과 pagination
    truncation을 bounded·validated DTO로 정규화하고 local package IDs에 다시 연결한다.
  - fresh/cached/stale/failed/notRequested 상태와 partial service result를 유지한다.
  - cache key는 normalized remote coordinate의 SHA-256 digest이고, cache는 최대 2,048개
    entry·4 MiB, fresh 24시간·stale fallback 7일로 제한한다.
- `apps/repo-manager/src-tauri/src/commands/dependency_enrichment.rs`
  - 고정된 `api.osv.dev`와 `api.deps.dev` HTTPS endpoint만 사용한다.
  - redirect/proxy를 끄고 4초 timeout, OSV 4 MiB, deps.dev version 512 KiB/package 2 MiB
    body bound를 적용한다. deps.dev는 좌표 4개 단위, 최대 8개 GET 동시성으로 실행하며
    process-wide execution single-flight를 유지한다.
  - 응답은 incremental bounded read 뒤 JSON parse하고, cache read/write는 symlink/reparse
    point 검사·atomic replacement·fixed failure 경계를 따른다.
- `apps/repo-manager/src-tauri/src/core/mod.rs`
- `apps/repo-manager/src-tauri/src/commands.rs`
- `apps/repo-manager/src-tauri/src/lib.rs`
  - core와 Tauri command를 등록했다.

### 3. Dependency graph and policy documentation

- `apps/repo-manager/src-tauri/Cargo.toml`
- `Cargo.lock`
  - `reqwest 0.13.4`를 `default-features = false`, `json`, `rustls`로 직접 연결하고,
    `futures-util 0.3.33`을 bounded scheduling에 직접 연결했다. 두 package는 이미 lock
    graph에 존재하므로 새 resolved package/license family를 만들지 않는다.
- `apps/repo-manager/README.md`
  - opt-in flow, supported metadata, fixed endpoints/caps, cache와 failure semantics를
    기록했다.
- `docs/dependency-policy.md`
  - direct dependency edge, rustls/no-default-feature 선택, fixed-host/no-proxy/no-redirect/
    timeout/body bounds와 notices 의무를 남겼다.
- `docs/architecture.md`
  - Workbench aggregate는 offline·read-only로 유지하면서 Repo Manager 상세 패널에서만
    명시적 검토·승인 뒤 원격 보강을 수행하는 경계를 반영했다.
- `docs/superpowers/specs/2026-08-30-dependency-lens-enrichment.md`
  - preview integrity, data minimization, service mapping과 공식 API references의 구현
    계약을 고정했다.

## Security and privacy decisions

- 네트워크는 offline 분석 또는 preview 중 자동으로 시작되지 않는다. 실행은 사용자가 검토한
  one-time plan으로 제한되고 repository 재검증과 revision 재검증을 통과해야 한다.
- 전송되는 값은 ecosystem/system, package name, exact resolved version뿐이다. path, opaque
  project ID, manifest/lockfile, graph edge, source URL, integrity/checksum, credential,
  environment와 user identity는 전송·cache 저장·Workbench snapshot 게시 대상이 아니다.
- endpoint·scheme·headers·proxy는 renderer가 지정할 수 없고 request body는 저장된 검토
  계획에서 native가 생성하며 redirect를 따르지 않는다. opaque preview token은 service
  credential이 아니라 저장된 일회성 계획의 식별자다. 응답은 명시적 byte bound 안에서만
  읽고 package/version/license/advisory 값도 추가 검증한다.
- malformed, oversized, unknown-schema, symlink/reparse-point cache는 fail-closed하고,
  cache/network/partial failure는 local graph를 삭제하지 않는다. renderer에는 response body,
  URL, header, server text, native error chain 대신 fixed error와 상태만 전달한다.
- license와 default version은 각각 정보 제공과 service default 표시일 뿐 법률 자문이나 안전한
  upgrade 보장이 아니다. 원격 metadata는 `dependency-summary/v1`에 들어가지 않는다.

## Code Examples

### Explicit native transport boundary

```rust
// apps/repo-manager/src-tauri/src/commands/dependency_enrichment.rs
fn production_client() -> Result<Client, String> {
    enrichment_client_builder()
        .https_only(true)
        .build()
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())
}

fn enrichment_client_builder() -> ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
}
```

### Review before execute

```text
offline scan → service selection → review exact coordinates (no network)
             → explicit send → revalidate repository/revision → bounded fetch
```

## Verification Results

현재 작업에서 확인된 범위는 다음과 같다.

```text
Focused remote-enrichment Rust tests: 20 passed; 0 failed
Repo Manager Rust tests: 121 passed; 0 failed
Repo Manager Dependency Lens frontend panel tests: 13 passed; 0 failed
Repo Manager frontend tests: 87 passed; 0 failed
Repo Manager production build: passed
Repo Manager Linux cargo check and strict Clippy: passed
Dependency notices/policy regression tests: passed
Full frontend workspace tests: 1,270 passed; 0 failed
Full frontend workspace production build: passed
Full Rust workspace check, strict Clippy, format check and tests: passed
cargo-deny: passed (duplicate-version advisories only)
pnpm audit --audit-level moderate: no known vulnerabilities
```

위 결과는 Linux/WSL source evidence이며 최종 release evidence가 아니다. native Windows CI와
Windows 실기는 이 workthrough 작성 시점에 **pending**이며, v0.6.0 release 전 별도로 통과해야
한다. WSL의 MSVC cross-check는 Visual C++/NASM이 없는 host 환경에서 `aws-lc-sys` C 빌드 전에
중단되어 code evidence로 사용하지 않는다.

## Next Steps

- native Windows CI evidence를 수집하고, 실패 시 고정 오류·cache·preview 경계를 재검토한다.
- Windows 실기에서 OSV/deps.dev opt-in 흐름, 오프라인 분석 보존, proxy/redirect/timeout,
  cache reuse/stale fallback과 Workbench snapshot 비노출을 확인한다.
- v0.6.0 release preparation에서 앱 버전 bump와 최종 notices/release manifest evidence를
  함께 갱신한다.
