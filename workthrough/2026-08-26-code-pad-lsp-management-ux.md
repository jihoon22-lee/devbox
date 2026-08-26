# Code Pad LSP Management UX Workthrough

- Date: 2026-08-26
- Issue: #278 `feat(code-pad): LSP 관리 UX`
- Branch: `feat/code-pad/lsp-management-ux`
- Base: `6fcc7ab4306afa24fb349f479609a0e6b031eddc`
- Target: Code Pad 0.4.0 / v0.5.0 P1-09-15
- Status: implementation, direct review and local PR-wide gates complete; GitHub Actions pending

## Outcome

Code Pad의 언어 서버 설정 dialog가 server lifecycle 상태, retry/backoff, 관리형 runtime cache와
최근 log를 한 화면에서 설명한다. crashed/degraded 상태와 열린 restart circuit은 `다시 시도`로
명시적으로 복구하고, 시작 중인 session은 중복 start를 허용하지 않으면서 별도 `중지` action으로
진행 중인 start reservation을 취소할 수 있다.

최근 log는 새 storage가 아니라 `LspManager` process memory에만 저장한다. 언어별 최대 200개 entry를
보존하며 server stderr는 native에서 bounded line으로 조립한 다음 path, URL, credential/token pattern을
redaction한다. raw config/protocol/install error와 installer validation reason도 Tauri command 경계에서
고정된 안전 메시지로 바꾼다.

Dialog는 header/footer를 고정하고 그 사이 body 하나만 scroll한다. 기존 status와 installer의 별도
`overflow-y`를 제거해 nested scroll을 없앴고, viewport 안에서 최대 900px 높이를 사용한다.

## Scope

### Included

- configured language별 stopped/starting/ready/degraded/crashed 상태
- restart failure count, 남은 backoff와 auto-restart circuit 상태
- crashed/degraded/circuit-open 상태의 explicit `다시 시도`
- manual start가 완료되기를 기다리지 않는 explicit start cancellation
- 관리형 server ref와 검증된 install index를 결합한 cache 상태
- lifecycle code/message와 bounded sanitized stderr recent log
- log entry/drop byte/truncation 표시
- status/log 2초 polling과 generation 기반 stale response suppression
- native command 단계의 config/protocol/installer 오류 sanitization
- corrupt managed index의 safe recovery-required signal과 explicit recovery 유지
- single-scroll panel과 충분한 viewport-bounded height
- Rust parser/store/error-boundary fixture와 React state/log/retry/cache/race fixture
- README, roadmap, native-first plan과 workthrough 갱신

### Excluded

- 새 LSP protocol feature, server capability 또는 document sync 변경
- managed runtime catalog, download, digest, extraction 또는 install transaction 변경
- 자동 server/runtime 설치 또는 자동 recovery
- raw stderr/raw error 보기와 export
- log persistence, telemetry 또는 external log tool handoff
- 새 network request, sidecar, capability 또는 storage schema
- editor/preview visual distinction (#279)

## Existing Boundary

기존 Code Pad는 다음 기반을 이미 제공했다.

- `LspManager`가 언어별 session과 restart tracker를 process memory에서 소유
- 세 번의 최근 failure 뒤 auto-restart circuit을 열고 1s/2s backoff 적용
- `LanguageServerStatus`에 restart attempt/failure/delay/circuit field 제공
- `LspProcess`가 stderr를 stdout JSON-RPC transport와 분리하고 bounded ring에 저장
- 관리형 installer가 exact manifest/version/platform index와 destination integrity를 검증
- frontend가 server status와 installer catalog/status를 조회
- corrupt config/index는 자동 덮어쓰기 없이 explicit recovery 필요

그러나 frontend는 retry field와 cache validation을 설명하지 않았고 stderr recent log command도 없었다.
Status/installer 각각이 scroll owner가 되어 작은 viewport에서 nested scroll이 생겼다. 관리 command와
installer error 문자열은 raw detail을 reject value로 전달할 수 있었고, polling response가 겹치면 늦은
응답이 최신 state를 덮을 수 있었다.

## Design Decisions

### 1. Keep logs runtime-only and bounded

`LspLogStore`는 최대 64개 language bucket과 language별 `VecDeque` 최대 200개만 유지한다. 오래된 entry를 제거할 때
`droppedEntries`를 증가시켜 UI가 생략 사실을 숨기지 않는다. Entry sequence는 JS number precision에
의존하지 않도록 decimal string으로 직렬화한다.

Log snapshot에는 다음만 포함한다.

```text
languageId
entries[{ sequence, level, code, message }]
droppedEntries
droppedStderrBytes
stderrTruncated
```

Timestamp, workspace path, executable path, argv, raw status reason과 raw stderr buffer는 포함하지 않는다.
Store는 serialize/persist하지 않으므로 앱을 닫으면 사라진다.

### 2. Sanitize stderr before it enters the log store

`StderrLineSanitizer`는 broadcast chunk를 newline 기준으로 조립한다. Raw line은 최대 8 KiB까지만
buffer하고 이를 넘으면 partial content 대신 고정 메시지 하나로 교체한다. 표시 message도 최대
2,048 Unicode character로 제한한다.

Sanitization은 다음 순서로 수행한다.

1. CR/tab과 control character 정규화
2. path/URL separator가 있는 line은 quoted path의 공백 뒤 suffix도 남지 않도록 전체를 고정 메시지로 교체
3. authorization, cookie, password, credential, secret, token, API key marker redaction
4. OpenAI/GitHub/JWT 형태의 known token redaction
5. display message length bound

Process stderr task가 raw ring에서 교체한 byte와 truncation flag를 별도 누적하고, broadcast receiver
lag도 고정된 warning entry로 기록한다. 이 수치는 정제 로그 누락량과 구분해 표시하며, raw chunk는
Tauri command에 전달하지 않는다.

### 3. Record lifecycle with fixed codes and messages

Start, ready, start failure, stop, manual retry, scheduled auto-restart, circuit open과 successful auto-restart는
manager가 고정 code/message로 기록한다. Internal `RestartTracker.reason`은 기존 restart 판단에만 쓰고
log DTO에는 넣지 않는다.

`record_restart_failure`는 manager state lock을 해제한 뒤 log lock을 잡는다. 서로 다른 mutex의 lock
order가 엇갈리지 않도록 state mutation과 log append를 한 critical section에 섞지 않았다.

### 4. Sanitize at the native command boundary

Frontend가 raw error를 받은 뒤 숨기는 것만으로는 IPC boundary를 보호하지 못한다. 따라서 management
command는 다음처럼 native에서 고정 메시지로 변환한다.

- config load/save: safe config message
- manager config/protocol error: generic language-server operation message
- installer network/archive/path/metadata error: generic managed-server operation message
- install status `reason`: fixed validation-failed message
- corrupt installed index: path/detail 없는 recovery-required signal
- third-party initialize `serverInfo` label: status/event DTO에서 제거하고 reviewed config/runtime identity 사용

Frontend recovery button은 더 이상 English raw error substring을 찾지 않고 exact safe recovery signal만
소비한다. Config DTO의 `error` field도 손상 여부만 보존하고 parser detail은 교체한다.

### 5. Reuse existing retry state and action

새 retry engine을 만들지 않았다. Status DTO에 이미 있던 `restartFailures`, `restartDelayMs`와
`autoRestartDisabled`를 표시하고, explicit recovery는 기존 `restart_language_server` command를 한 번
호출한다. 이 command는 restart tracker/circuit을 지우고 현재 session을 안전하게 종료한 뒤 기존
start pipeline을 사용한다.

Manual start가 오래 걸릴 때는 global operation busy 상태와 별개인 cancel action이 native `stop`을 호출한다.
Native manager는 in-progress reservation token을 제거하고 늦게 완성된 child를 publish하지 않은 채 종료한다.
취소 의도로 발생한 start reject만 frontend에서 흡수하고 다른 start failure는 기존 safe 오류로 유지한다.

### 6. Derive cache state without exposing install paths

Configured `managed` server의 manifest ID/version을 `ManagedInstallStatus`의 exact key와 비교한다.

```text
installed + exact reviewed catalog/status -> 검증된 캐시 사용 가능 · runtime kind · executable name
needs_reinstall              -> 캐시 검증 실패 · 재설치 필요
not_installed                -> 캐시 없음 · 설치 필요
matching catalog/status 없음 -> 검토된 catalog에 없음
```

Process-owned canonical destination과 executable absolute path는 기존 installer private index에 남고 새
frontend DTO에 추가되지 않는다. Runtime 표시는 reviewed metadata의 kind/executable name만 사용한다.

### 7. Reject stale polling responses

Status와 log는 한 generation에서 `Promise.all`로 조회한다. 각 refresh가 monotonically increasing
generation을 받고, 현재 active generation과 일치할 때만 두 state를 함께 commit한다. Dialog cleanup은
active flag를 내리고 generation을 증가시켜 unmount 뒤 resolution도 무시한다.

Action 직후 refresh와 2초 interval이 겹쳐도 오래된 status/log pair가 새 result를 덮지 않는다.

### 8. Use one scroll owner

Panel은 다음 layout contract를 사용한다.

```text
fixed header
  scrollable .lsp-panel-body
    config
    status cards
    log disclosures
    managed installer
fixed wrapping footer
```

`.lsp-status-section`과 `.lsp-installer-section`의 `overflow-y`를 제거했다. Panel은
`min(900px, calc(100vh - 32px))` 높이와 `min(900px, calc(100vw - 32px))` 너비를 사용해 작은
viewport에서도 backdrop 밖으로 빠지지 않는다.

## File Changes

### Native

- `apps/code-pad/src-tauri/src/lsp/logs.rs`
  - bounded in-memory log store
  - stderr line assembler/redactor
  - path/URL/credential/oversize/drop fixtures
- `apps/code-pad/src-tauri/src/lsp/manager.rs`
  - log store ownership
  - lifecycle/retry log recording
  - stderr event monitor
  - log snapshot query
- `apps/code-pad/src-tauri/src/commands/lsp.rs`
  - log command
  - management/config public error boundary
- `apps/code-pad/src-tauri/src/commands/installer.rs`
  - installer status/error public boundary and safe recovery signal
- `apps/code-pad/src-tauri/src/lsp/mod.rs`, `src-tauri/src/lib.rs`
  - module export and Tauri command registration

### Frontend

- `apps/code-pad/src/types.ts`, `src/api.ts`
  - log DTO and invoke wrapper
- `apps/code-pad/src/components/LspControlPanel.tsx`
  - status/log generation polling
  - retry/cache/log UI
  - safe management errors and single body scroll
- `apps/code-pad/src/components/ManagedInstallerPanel.tsx`
  - fixed validation explanation
  - safe recovery signal consumption
  - raw operation error suppression
- `apps/code-pad/src/App.css`
  - panel sizing/single scroll
  - cache/retry/log presentation
- `apps/code-pad/src/components/LspControlPanel.test.tsx`, `src/App.test.tsx`
  - API mock and UI/race/error regression

### Documentation

- `apps/code-pad/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- this workthrough

## Failure and Security Fixtures

Rust fixtures cover:

- path, URL, Authorization/Bearer, password and known token redaction
- split stderr chunks, invalid control bytes and final partial line
- oversized raw line replacement
- 200-entry ring eviction and dropped count
- 64-language bucket upper bound
- retry circuit lifecycle codes without raw failure reason
- config/protocol error path/credential suppression
- corrupt config DTO detail replacement
- installer network URL/credential suppression
- exact safe corrupt-index recovery signal
- install validation reason replacement

React fixtures cover:

- crashed/degraded explicit retry and exact one restart command
- restart failure count, rounded delay and circuit-open explanation
- verified managed cache state
- bounded log entry and truncation/drop warning
- pending manual start 중에도 stop command가 실행되는 cancellation race
- old status/log response ignored after a newer action refresh
- corrupt config detail absent from the DOM
- managed validation reason absent from the DOM
- safe corrupt-index signal retaining explicit recovery
- existing install/uninstall/config/save behavior

## Validation

Completed while implementing:

```text
cargo fmt --all -- --check
cargo test -p code-pad --lib lsp::
  104 passed
cargo test -p code-pad --lib commands::
  29 passed
pnpm --dir apps/code-pad test -- --maxWorkers=2
  14 files, 112 tests passed
pnpm --dir apps/code-pad build
  TypeScript and Vite production build passed
cargo test --workspace
  all workspace unit/integration/doc tests passed
cargo check --workspace
  passed
cargo clippy --workspace --all-targets -- -D warnings
  passed
pnpm --workspace-concurrency=2 -r build
  all 17 frontend/package projects passed
pnpm --workspace-concurrency=2 -r test
  all 17 frontend/package projects passed
check-catalog + dependency-policy regression scripts
  passed
pnpm audit --audit-level moderate
  no known vulnerabilities
cargo deny --locked check
  advisories, bans, licenses and sources passed; allowlisted duplicate warnings only
```

The frontend checks ran from an exact Linux-native mirror with the offline pnpm store. This avoids `/mnt/e` 9p
latency while keeping build concurrency at two workers. The exact temporary mirror was removed after final
PR-wide validation.

Remaining before merge:

- all GitHub Actions checks

Tracked release checkpoint after the remaining P1 merges:

- Windows W1 packaged checkpoint evidence (WSL cannot execute the packaged Tauri runtime)

## Manual W1 Checkpoint

On Windows packaged Code Pad, verify:

- ready/stopped/starting/crashed/degraded status transitions
- server crash shows failure count and 1s/2s retry countdown
- third recent failure opens the circuit and explicit `다시 시도` recovers
- installed/not-installed/needs-reinstall/orphaned managed cache states
- stderr path, URL and credential sentinels are absent from log UI
- oversized stderr shows a fixed omission entry; raw ring replacement and sanitized-log eviction are described separately
- narrow viewport has one body scrollbar; status and installer do not create nested scrollbars
- header/footer remain reachable and footer wraps without covering content
- panel close/reopen clears runtime-only logs only when the application process restarts
- no console window, extra process, automatic download or external state change is introduced

## Follow-up

#279 `feat(code-pad): editor·preview 구분` remains the next independent P1-09 feature. It owns editor focus and
preview visual distinction and must not expand this PR's LSP manager or log boundary.
