# Life Log 일간·주간 (+ 기존 월간) 로컬 digest 구현

## Overview

Issue #306의 P2 Life Log 로컬 요약을 구현했다. 기존 `life-log/export/v1`가 소유한
날짜 경계, privacy, bounded DB/Git 조회, integration snapshot provenance를 새 기능에서
복제하지 않고 재사용해, 일간·주간과 기존 월간 화면에서 같은 입력이 같은 수치와 Markdown을 만드는
`life-log/digest/v1` 응답을 추가했다.

digest는 session과 요청 범위의 Git만 수치에 포함한다. Run Manager와 Knowledge Base는
현재 range-keyed history를 발행하지 않으므로 최신 snapshot의 provenance만 표시한다. cloud/local
LLM, network fetch, 개인 활동 원문의 외부 전송, 자동 저장은 구현하지 않았다.

## Context

- 일간/주간/월간 기존 화면은 기간 차트만 제공하고, source ownership과 aggregation rule을 한눈에
  설명하는 artifact가 없었다.
- 고정 `24 * 60 * 60 * 1000` 연산은 DST 전환일의 civil-day를 왜곡할 수 있었다. 날짜별
  `dayBoundaries`를 frontend가 만들고 native가 그대로 소비하는 기존 export 계약을 digest에도
  적용했다.
- native와 browser를 같은 데이터처럼 표시하면 브라우저에서 local DB/Git/snapshot이 있는
  것처럼 오해할 수 있으므로 browser fallback은 0-valued preview와
  `browser_preview_only` source 상태를 명시한다.
- save/copy/load가 비동기이므로 stale response, unmount, double action, native 내부 오류의
  raw 반향을 막아야 했다.

## Changes Made

### 1. Native digest domain and shared range validation

Files:

- `apps/life-log/src-tauri/src/core/digest.rs`
- `apps/life-log/src-tauri/src/core/export.rs`
- `apps/life-log/src-tauri/src/core/mod.rs`

`core::digest`에 다음 wire type을 추가했다.

- `DigestInput`: civil-date inclusive `startDate`/`endDate`, exclusive epoch `dayEnd`,
  timezone, authoritative local-civil-day boundaries, `day`/`week`/`month` period, nullable exact
  sanitized app filter.
- `DigestDocument`: fixed schema/version, range/filter/rules/headline, filtered session summary,
  daily rows, deterministic app totals, Git rows, four-source metadata.
- `DigestResponse`: native/browser origin, document, deterministic Markdown.

검증 순서는 다음과 같다.

1. period-specific day count를 먼저 확인한다(day 1, week 7, month 28~31).
2. day는 start/end가 같은 하나의 local civil day인지, week는 월요일 시작인지, month는 첫날이
   `-01`이고 같은 `YYYY-MM`인지 확인한다.
3. app filter는 empty/control/credential-shaped marker 및 256-byte 초과를 거부한다.
4. `export::validate_range_input`으로 실제 날짜 유효성, timezone, exclusive range, 연속
   boundary, DST에서 가능한 23/24/25시간 폭, 366일 상한을 export와 동일하게 검증한다.
5. 그 뒤에만 DB bounded query와 Git/snapshot 준비를 수행한다.

export의 내부 `ValidatedRange`를 외부로 노출하지 않고도 다른 local producer가 동일한
검증을 호출할 수 있도록 다음 좁은 helper를 추가했다.

```rust
pub fn validate_range_input(input: &ExportInput) -> Result<(), String> {
    validate_range(input).map(|_| ())
}
```

`prepare`는 DB mutex를 보유한 동안 privacy가 적용된 최대 50,000 session과 snapshot metadata를
준비하고, `build_response`는 mutex를 놓은 뒤 기존 bounded Git collector를 한 번 호출한다.
주·월 화면의 chart도 이 digest 결과에서 파생하므로 같은 기간에 `get_range`를 추가 호출해
DB/Git 작업을 중복하지 않는다.

### 2. Deterministic aggregation and provenance

Files:

- `apps/life-log/src-tauri/src/core/digest.rs`
- `apps/life-log/src-tauri/src/commands/digest.rs`

- export producer가 privacy/redaction을 적용한 session에 exact app filter를 적용한다.
- session은 저장된 duration을 보존하고 시작 timestamp가 속한 supplied boundary에 귀속한다.
- app totals는 duration 내림차순, 동률이면 UTF-8 byte 순으로 정렬하며 unique app은 2,048개로
  제한한다.
- daily row에는 date/boundary/PC usage/session count/Git commits/top app/empty 여부를 넣고,
  summary에는 usage/session/active day/total day/average/top app/Git total을 넣는다.
- app filter는 Git 결과에는 영향을 주지 않는다. Git은 requested range의 read-only count로
  유지되어 UI에서 “앱 필터 결과”와 “기간 Git 활동”을 혼동하지 않게 했다.
- Git project는 `parse_safe_project_path` 결과와 identity dedupe/order를 다시 확인하고,
  project error에는 count 0과 fixed error code만 허용한다.
- source 순서는 `life-log`, `git`, `run-manager`, `knowledge-base`로 고정한다.
- Run/Knowledge source는 latest-snapshot-out-of-range 상태와 schema/snapshot/producer
  version, generatedAt, freshness, Knowledge named view만 provenance으로 전달한다. note ID,
  path, title, body, raw environment 값은 digest DTO에 포함하지 않는다.
- snapshot source의 producer version/UTC timestamp/view/error code 및 life-log source version을
  저장 직전 다시 확인하고, 다른 source의 error code가 섞이지 않도록 fail-closed한다.

response validation은 generated DTO를 그대로 믿지 않는다.

- input/range를 export shared validator로 재검증한다.
- headline/rules를 summary와 fixed rule template에서 다시 계산한다.
- daily/app/Git 합계, active day, top app, project identity order, source 관계를 확인한다.
- known semver/UTC timestamp, known snapshot view/error set만 통과시킨다.
- Markdown이 현재 document의 renderer 결과와 byte-identical한지 확인한다.

```rust
let response = DigestResponse {
    origin: DigestOrigin::Native,
    document,
    markdown,
};
validate_response(&response)
    .then_some(response)
    .ok_or_else(|| "digest 결과를 검증하지 못했습니다".into())
```

### 3. Tauri commands and explicit save boundary

Files:

- `apps/life-log/src-tauri/src/commands/digest.rs`
- `apps/life-log/src-tauri/src/commands/mod.rs`
- `apps/life-log/src-tauri/src/lib.rs`

`get_digest`는 DB/Git/snapshot을 읽어 bounded response만 반환하며 파일/clipboard/history/
telemetry/network side effect가 없다. `save_digest`는 response를 다시 검증한 뒤 Windows
native Markdown save dialog에서 사용자가 확정한 path에만 `devbox_filesystem::atomic_write`를
호출한다.

- cancel은 `{ saved: false }`로 끝나고 파일을 만들지 않는다.
- absolute path, parent directory, `.md` extension, control character, existing target
  symlink/non-file를 확인한다.
- serializer/renderer/OS/Git/path 오류는 고정 안내로 collapse하며 raw stderr/path/credential을
  반환하지 않는다.
- non-Windows에서는 native 저장을 성공한 것처럼 가장하지 않고 fixed unsupported error를
  반환한다.

### 4. Frontend API and browser parity

Files:

- `apps/life-log/src/api.ts`
- `apps/life-log/src/api.export.test.ts`

TypeScript DTO와 `validateDigestInput`을 추가해 native invoke와 browser fallback 앞에서 exact
object keys, date/boundary, integer, byte, period, filter bounds를 공통 확인한다. 긴 문자열은
UTF-8 encode 전에 code-unit 상한을 먼저 확인해 malformed caller가 불필요한 큰 allocation을
유발하지 않도록 했다.

브라우저에서는 native DB/Git/snapshot을 읽지 않고 다음을 반환한다.

- `origin: "browser-preview"`
- four fixed sources 모두 `available: false`, `scope: "browser-preview-only"`,
  `errorCode: "browser_preview_only"`
- zero-valued summary/daily/app totals
- native data가 없음을 명시한 fixed headline/Markdown

browser Markdown/JSON/CSV와 native 응답의 source/range/empty 구조를 맞추되, native 성공이나
실제 local data를 가짜로 표시하지 않는다. explicit download에서만 browser Markdown을 사용하며
자동 persistence는 없다.

### 5. Daily/weekly (+ existing monthly) UI, stale handling, and usability

Files:

- `apps/life-log/src/App.tsx`
- `apps/life-log/src/App.css`
- `apps/life-log/src/App.test.ts`

- `buildDigestInput`이 정확히 한 local civil day, 월요일 시작 주간, 달력 월말(윤년 포함)을
  local civil-day boundary로 만든다.
- daily/weekly (+ existing monthly) panel에 summary cards, daily rows, empty state, exact app filter, timezone/range
  status, source/rule disclosure를 제공한다.
- app filter 변경, period/date 전환, refresh마다 `loadRequestRef`를 증가시켜 오래된 응답이
  state를 덮지 못하게 한다. 새 load가 시작되면 이전 digest를 먼저 버려 stale Markdown
  copy/save를 막는다.
- copy는 현재 Markdown만 explicit clipboard action에서 한 번 기록한다.
- native save 또는 browser download는 명시적 버튼 action에서만 실행하고,
  `digestBusyRef`/`contextActionBusy`로 duplicate action을 차단한다.
- action은 action token과 load token을 함께 기억해 날짜/필터가 바뀐 뒤 완료된 작업의 notice/
  error가 새 화면에 나타나지 않게 한다. unmount 시 load/action token과 busy ref를 무효화한다.
- 버튼은 `type="button"`, loading/busy disable, labels, live range status, `aria-busy`를
  사용한다. 기존 export modal의 keyboard/IME/focus 계약은 유지한다.
- native source details는 schema/snapshot/producer version, generatedAt, freshness, known view
  만 안전하게 표시하며 unknown source/scope는 fixed label로 축약한다.
- 주·월 chart는 digest daily result에서 파생해 native Git/DB query를 중복하지 않는다.

```tsx
const action = beginDigestAction();
if (!response || action === null) return;
try {
  await navigator.clipboard.writeText(response.markdown);
  if (isCurrentDigestAction(action)) setNotice("현재 digest를 클립보드에 복사했습니다.");
} catch {
  if (isCurrentDigestAction(action)) setError("digest를 클립보드에 복사하지 못했습니다.");
} finally {
  finishDigestAction(action);
}
```

### 6. Documentation and fixtures

Files:

- `apps/life-log/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `workthrough/2026-08-27-life-log-daily-weekly-digest.md`

README, roadmap, native-first plan에 wire contract, date/DST semantics, source ownership,
privacy/error policy, bounds, browser/native distinction, explicit copy/save, stale/a11y behavior,
비범위(LLM/cloud/network/#307 handoff)를 모두 반영했다.

Rust fixture는 다음을 고정한다.

- day/week/month shape와 invalid date/range rejection
- app filter credential/control rejection 및 non-reflective fixed error
- deterministic duration/tie ordering/top app
- privacy 이후 exact app filtering
- empty response와 native Markdown no-title output
- changed Markdown/range boundary rejection
- untrusted snapshot view rejection

Frontend fixture는 다음을 고정한다.

- local daily one-boundary builder and weekly seven-boundary builder
- Monday-week and leap-year monthly 29-boundary builders
- browser source unavailability/empty response와 deterministic daily/weekly/monthly preview
- malformed boundary/timezone 및 credential-like filter의 fixed error/non-reflection
- existing export date boundary/DST-compatible week regression

## Verification Results

### Rust

전용 Linux target과 두 개의 build job을 사용했다.

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue306
CARGO_INCREMENTAL=0
-j2
```

- `cargo fmt --manifest-path apps/life-log/src-tauri/Cargo.toml -- --check` — passed
- `cargo test --manifest-path apps/life-log/src-tauri/Cargo.toml core::digest -j2` — **7 passed**
- `cargo test --manifest-path apps/life-log/src-tauri/Cargo.toml -j2` — **76 passed, 0 failed**
- `cargo check --manifest-path apps/life-log/src-tauri/Cargo.toml -j2` — passed
- `cargo clippy --manifest-path apps/life-log/src-tauri/Cargo.toml --all-targets -- -D warnings` — passed

### Frontend

`pnpm install --frozen-lockfile`을 전용 worktree에서 실행했으며 lockfile은 변하지 않았다.

- `pnpm --filter life-log test` — **4 files, 41 tests passed**
- `pnpm --filter life-log build` — passed (`tsc` + Vite)
- `pnpm -r --workspace-concurrency=1 test` — all workspace frontend test projects passed
- `pnpm build` — all workspace frontend builds passed
- `bash .github/scripts/run-frontend-scope.sh typecheck all` — passed

기본 병렬 `pnpm test`는 `devbox-manager` Vitest worker 시작 timeout으로 중단되었지만,
동일한 전체 suite를 concurrency 1로 재실행해 모두 통과했다. life-log 테스트의 browser
download fixture에서 jsdom의 navigation 미구현 경고만 발생했으며 실패는 없었다.

### Dependency and diff checks

- `pnpm audit --audit-level moderate` — no known vulnerabilities
- `.github/scripts/check-dependencies.py check` — passed; notices match lockfiles
- `.github/scripts/test-check-dependencies.py` — passed
- `.github/scripts/test-build-manifest.py` — passed
- `cargo deny check` — advisories, bans, licenses, sources all passed (existing duplicate-crate warnings)
- `git diff --check` — passed
- target size: `du -sh /home/jihoon/.cache/targets/devbox-issue306` — **4.4G**

Windows packaged W2 smoke는 WSL에서 실행할 수 없으므로 남아 있다. 실제 native DB/Git,
Windows save dialog/atomic write, packaged UI에서 daily/weekly (+ existing monthly) source disclosure와
day summary/timeline/export 회귀를 확인하려면 Windows 환경의 `pnpm tauri dev` 또는
`pnpm tauri build` checkpoint가 필요하다.

## Next Steps

- PR 전 Windows W2 packaged smoke와 GitHub Actions 전체 gate를 실행한다.
- Run Manager/Knowledge가 동일 기간의 range-keyed history snapshot을 제공하게 되면,
  현재 provenance-only source를 별도 계약 검토 후에만 digest summary로 확장한다.
- #307의 Knowledge handoff/저장과 자동 AI/cloud summary는 이 기능의 후속 범위로 유지한다.
