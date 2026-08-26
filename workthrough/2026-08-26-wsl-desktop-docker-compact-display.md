# WSL Desktop Docker Compact Display Workthrough

- Date: 2026-08-26
- Issue: #276 `feat(wsl-desktop): Docker compact 표시`
- Branch: `feat/wsl-desktop/docker-compact-display`
- Base: `b86b1a69725bf960fc040475b2bafef2b0324efc`
- Target: WSL Desktop 0.4.0 / v0.5.0 P1-09-13
- Status: implementation, direct review and local PR-wide gates complete; GitHub Actions pending

## Outcome

WSL Desktop의 Docker 영역은 더 이상 5열 가로 table을 사용하지 않는다. 각 컨테이너는 native
`details`/`summary` 행으로 표시되며 접힌 상태에서 다음 정보를 우선한다.

1. container name
2. 정규화된 lifecycle state
3. 좁게 축약한 published/target port mapping

행을 펼치면 Docker CLI가 반환한 full container ID, image, original status와 original ports를 확인하고
기존 start/stop/restart action을 실행할 수 있다. 이름과 port summary는 260px 패널에서 잘리지 않도록
ellipsis를 사용하고, exact name/ports는 title과 detail에 남는다.

backend는 가변 공백 table을 추측하지 않는다. Docker CLI에 ID/name/image/status/ports 다섯 필드만
명시적으로 요청하고 tab 구분 결과를 exact field count로 검증한다. 따라서 status에 port나 container
name이 섞이는 기존 parser 오류 가능성을 제거하고, malformed output은 빈 목록으로 오인하지 않고
고정 오류로 fail closed한다.

## Problem and Existing Boundary

기존 패널 폭은 360px이며 260px까지 줄어들 수 있지만 Docker 목록은 NAME, IMAGE, STATUS, PORTS,
ACTION 5열 table이었다. 각 cell은 `white-space: nowrap`이라 가로 폭을 반드시 초과했고, 중요한
name/state/port보다 image와 action 열이 같은 우선순위를 차지했다.

backend도 기본 `docker ps -a`의 공백 table을 heuristic으로 파싱했다. COMMAND와 CREATED는 공백을
포함하고 STATUS 길이도 가변인데, status 시작 token 뒤 최대 네 단어를 가져오고 colon/arrow token을
port로 추정했다. 이 방식은 status에 port 일부가 들어가거나 노출-only port를 잃을 수 있어 “원문을
detail에서 표시”한다는 계약을 만족할 수 없었다.

이 PR은 이미 설치된 Docker CLI/engine의 container snapshot과 기존 action만 다룬다. devbox가 Docker를
download, install 또는 configure하지 않으며 Docker Desktop 전체 기능을 복제하지 않는다.

## Scope

### Included

- 260px sidebar에서도 유지되는 container disclosure list
- name/state/port 우선 summary
- `Running`, `Paused`, `Restarting`, `Removing`, `Exited`, `Created`, `Dead`, `Unknown` 표시 분류
- host address를 제거한 `published→target/protocol` port summary
- IPv4/IPv6 duplicate mapping 제거
- 고유 mapping 최대 2개와 나머지 `+N` 표시
- Docker가 반환한 ID/image/status/ports exact detail
- empty ports의 명시적 `(empty)` detail과 `No ports` summary
- Created/Exited container의 Start, 나머지 기존 Stop/Restart 흐름
- missing Docker guidance와 container list 비표시
- malformed formatted output의 고정 오류
- keyboard로 열고 닫을 수 있는 native disclosure semantics와 focus-visible outline

### Excluded

- Docker CLI 또는 engine 자동 설치/download/update
- daemon start/stop, engine settings와 resource summary
- image build/pull/push, registry, volume, network, Compose와 Kubernetes UI
- COMMAND와 container environment 조회
- CPU, memory, disk, health detail와 logs
- Port Manager/Workbench snapshot producer 변경
- Log Lens container log adapter
- storage schema, history, profile 또는 snapshot persistence
- Docker action protocol 확대와 새 destructive action

## Design Decisions

### 1. Request structured fields from Docker instead of reparsing its table

The command is executed as exact argv:

```text
wsl.exe -d <distro> -- docker ps -a --no-trunc --format
  {{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}
```

`--no-trunc` keeps the detail evidence complete. The command does not pass through a shell and does not request
COMMAND, labels, mounts, environment or inspect output.

### 2. Fail the snapshot instead of guessing malformed rows

An empty command output is a valid empty container list. Every non-empty line must have exactly five tab-delimited
fields. ID, name, image and status must be non-empty; ports may be empty. A malformed line aborts the whole parse
with `컨테이너 목록 형식이 올바르지 않습니다.`. The error contains no source line, field, image, container name
or parser message.

The fixed parser error intentionally omits the Docker executable name so the frontend's existing installation-error
matching branch does not mistake a format contract failure for an installation problem.

### 3. Keep source fields immutable and derive summary-only values

The existing `ContainerInfo` wire shape remains:

```json
{
  "id": "full-container-id",
  "name": "api",
  "image": "registry.example/team/api:latest",
  "status": "Up 3 hours (healthy)",
  "ports": "0.0.0.0:8080->80/tcp, :::8080->80/tcp"
}
```

`dockerDisplayState` and `compactDockerPorts` return view-only values. They never mutate this DTO. Detail rendering
reads the original DTO properties directly.

### 4. Deduplicate address-family bindings by their useful mapping

Docker commonly renders the same published mapping once for IPv4 and once for IPv6:

```text
0.0.0.0:8080->80/tcp, :::8080->80/tcp
```

Both become `8080→80/tcp` in the summary. Bracketed IPv6 (`[::]:8080`) and Docker's compact IPv6 (`:::8080`)
use the same final-colon rule. Exposed-only ports such as `6379/tcp` remain unchanged. The full source string always
remains in Original ports.

### 5. Use native disclosure semantics

`details`/`summary` supplies Enter/Space keyboard toggling and a semantic expanded state without a new React state
machine. A custom chevron is `aria-hidden`; the actual name, state and port text remain the summary's accessible
name. `focus-visible` adds a clear outline and the expanded body wraps long untrusted text with
`overflow-wrap: anywhere`.

### 6. Do not add another Docker integration layer

This feature remains in `wsl-desktop` because it is the single consumer of the distro-scoped Docker list. No common
crate/package was extracted and no sidecar or adapter dependency was added. Workbench snapshot suggestions and Log
Lens log ingestion remain their separately identified PRs.

## Data Flow

```text
Refresh / selected distro
  -> docker_ps(distro)
  -> wsl.exe exact argv
  -> docker ps five-field --format output
  -> parse_docker_ps
       -> empty output => []
       -> exact five fields => ContainerInfo
       -> malformed required field/count => fixed error
  -> runtime-only frontend ContainerInfo[]
  -> summary: name + derived state + derived compact ports
  -> detail: exact ID + image + status + ports
  -> existing action callback by exact container ID
```

## Display Rules

### State

| Source status prefix/content | Summary | Running count |
|---|---|---:|
| `Up ...` | Running | yes |
| `Up ... (Paused)` or `Paused...` | Paused | no |
| `Restarting...` | Restarting | no |
| `Removal...` / `Removing...` | Removing | no |
| `Exited...` | Exited | no |
| `Created...` | Created | no |
| `Dead...` | Dead | no |
| empty/unrecognized | Unknown | no |

Paused is checked before Up so Docker's `Up ... (Paused)` form is not counted as running.

### Ports

| Source | Compact summary |
|---|---|
| `127.0.0.1:8080->80/tcp` | `8080→80/tcp` |
| `[::]:8080->80/tcp` | `8080→80/tcp` |
| `6379/tcp` | `6379/tcp` |
| empty | `No ports` |
| three unique mappings | first two plus `+1` |

## Backend Changes

- `commands/dashboard.rs`
  - adds the exact Docker format constant
  - calls `docker ps` with `--no-trunc --format`
  - maps the parser's fixed error without reflecting command output
- `core/parsers.rs`
  - replaces the heuristic default-table parser with exact tab fields
  - preserves empty ports and CRLF output
  - accepts an empty list
  - rejects malformed rows as one snapshot failure

The Tauri command name and `ContainerInfo` serialization contract did not change.

## Frontend Changes

- `lib/dockerDisplay.ts`
  - normalizes display state without replacing source status
  - compacts/deduplicates port mappings with a two-item bound
- `components/DistroPanel.tsx`
  - replaces the wide table with disclosure rows
  - keeps summary and original detail in separate semantic regions
  - retains existing action callback and busy-state contract
- `App.css`
  - adds a bounded two-row summary grid
  - ellipsizes summary-only values and wraps detail values
  - preserves keyboard focus visibility

No HTML string injection, raw inner HTML, clipboard write, file write, network request or storage write was added.

## Security and Privacy Review

| Threat | Control |
|---|---|
| COMMAND or environment exposes a credential | Docker format does not request either field |
| table ambiguity mixes values | exact tab format and five-field validation |
| malformed output looks like no containers | whole snapshot fails with a fixed error |
| parser error reflects untrusted container data | one constant Korean error only |
| summary overwrites source evidence | all derivation is view-only; detail reads DTO source fields |
| IPv6 address consumes narrow panel | host address is summary-only elided; exact ports remain in detail |
| long image/name/status forces horizontal overflow | summary ellipsis, detail `overflow-wrap: anywhere` |
| unsafe markup in Docker-controlled text | React text nodes only |
| accidental persistence | container state remains runtime React memory only |
| feature becomes Docker Desktop clone | engine/image/volume/registry/resource/log operations excluded |
| new external supply-chain surface | no dependency, sidecar, download or capability change |

## Tests

### Rust WSL Desktop

- exact two-row formatted parse
- source image/status/ports equality
- empty ports and CRLF preservation
- malformed field count, empty ID/image/status rejection with fixed non-reflective error
- valid empty output
- all existing WSL, terminal, workspace and multiplexer fixtures

```text
cargo test -p wsl-desktop --lib core::parsers::tests --jobs 1
8 passed; 0 failed

cargo test --workspace --jobs 1
all workspace unit, integration and doc tests passed

cargo clippy -p wsl-desktop --all-targets --jobs 1 -- -D warnings
passed
```

### Frontend WSL Desktop

- all Docker lifecycle state classifications including paused-before-running and removing
- IPv4/IPv6 duplicate mapping
- exposed-only and empty ports
- maximum two mappings plus remaining count
- 260px fixture summary includes name/state/ports but not image/original status
- exact ID/image/status/empty ports in detail
- Created container Start callback uses exact ID
- missing Docker guidance omits the container list
- all existing terminal, context-menu, storage, workspace and applink fixtures

```text
pnpm --filter wsl-desktop exec vitest run --maxWorkers=2
Test Files  15 passed (15)
Tests       117 passed (117)
```

The frontend test and build ran from an exact rsync copy under a Linux-native cache directory. This avoids repeated
jsdom/module initialization through the `/mnt/e` 9p mount. The source worktree remained the only edited source and
the temporary validation copy is removed after review.

## Build

```text
pnpm -r --workspace-concurrency=2 build
17 of 18 frontend/package workspace projects passed
```

WSL Desktop production assets:

| Asset | Size | gzip |
|---|---:|---:|
| JS | 643.24 kB | 184.27 kB |
| CSS | 16.80 kB | 3.95 kB |

The existing Vite warning for a WSL Desktop chunk over 500 kB remains. No package, Rust dependency, capability,
sidecar, runtime download or storage schema was added.

## Documentation

- app README documents compact summary, exact detail, runtime-only data and Docker CLI/engine boundary
- architecture maps formatted Docker fields to summary/detail
- UX design marks the compact panel row implemented and records the 260px fixture
- native-first plan records exact state/port rules, fail-closed parsing, exclusions and W1 evidence
- roadmap advances the next P1-09 item to #277 Code Pad Quick Open

## Files

- `apps/wsl-desktop/src-tauri/src/commands/dashboard.rs`
- `apps/wsl-desktop/src-tauri/src/core/parsers.rs`
- `apps/wsl-desktop/src/lib/dockerDisplay.ts`
- `apps/wsl-desktop/src/lib/dockerDisplay.test.ts`
- `apps/wsl-desktop/src/components/DistroPanel.tsx`
- `apps/wsl-desktop/src/components/DistroPanel.test.tsx`
- `apps/wsl-desktop/src/App.css`
- `apps/wsl-desktop/README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

## PR-wide Gates

- `cargo fmt --all -- --check`: passed
- `cargo clippy -p wsl-desktop --all-targets --jobs 1 -- -D warnings`: passed
- `cargo test --workspace --jobs 1`: passed
- `cargo check --workspace --jobs 1`: passed
- `pnpm --filter wsl-desktop exec vitest run --maxWorkers=2`: 117 passed
- `NODE_OPTIONS=--max-old-space-size=1024 pnpm -r --workspace-concurrency=2 build`: passed
- `pnpm install --frozen-lockfile --prefer-offline`: passed in Linux-native validation copy
- `pnpm audit --audit-level moderate`: no known vulnerabilities
- dependency notices and dependency policy regression tests: passed
- build-manifest notice tests: passed
- catalog consistency: passed
- `cargo deny --locked check`: advisories, bans, licenses and sources passed; configured duplicate warnings only
- GitHub Actions Linux, Windows, frontend, dependency and catalog gates: PR에서 확인 예정

## W1 Packaged Checkpoint

- Docker CLI/engine이 있는 selected distro에서 full ID/image/status/ports가 exact detail에 표시된다
- IPv4/IPv6가 함께 publish된 mapping은 summary에서 한 번만 보이고 detail에는 원문이 남는다
- 260px sidebar에서 이름·상태·port가 보이고 수평 table overflow가 생기지 않는다
- 긴 container name/image/status/ports가 detail panel 밖으로 밀어내지 않는다
- keyboard로 summary를 열고 닫으며 focus outline과 expanded semantics가 유지된다
- Created/Exited Start 및 running Stop/Restart가 exact container ID에 적용된다
- Docker CLI가 없으면 guidance가 나오고 container list가 보이지 않는다
- malformed format fixture는 빈 목록이나 설치 부재로 표시되지 않고 고정 오류가 나온다
- refresh 뒤 이전 runtime snapshot이 storage/profile/history에 남지 않는다

## Next

#277 Code Pad Quick Open is the next P1-09 feature. Its app-local implementation draft is being prepared in a
separate worktree, but no #277 code or documentation is part of this PR. LSP UX, preview distinction, Workbench
runtime suggestions, Docker resource summary and Log Lens remain their independently reviewed issues.
