# v0.8 review corrections

Base: main `d6208b37f199545965fa1a55b473f3c4e8add299`.
Branch: `fix/workspace/v08-review-corrections`; reuse the existing worktree.
Single PR explicitly requested by the user; logical commits separate the changes.
Refs B02–B08, R02/R07–R09/R12/R15–R22/R25. Original scope was implementation and verification.
The follow-up explicitly authorizes CI-gated merge, cleanup and v0.8.1 publication (see below).
No intermediate test/build/Clippy/CI execution.

## Scope

- API GraphQL redirect payload confidentiality; source-owned Webhook → Workspace Logs review.
- Workspace Git commit bound to reviewed native index + HEAD.
- Runtime owned cleanup, private marker delivery, bounded observation and production-order regressions.
- CI cross-boundary include detection, Windows job label, Cargo placeholders; watcher poison handling.

- Cross-review 2/3: document identity, edit revision, latest-open admission and one ordered save.
- Cross-review 4: native file identity/content revision, conditional save, conflict comparison,
  explicit reread/conditional overwrite, missing-file recreation after review.
- Cross-review 5: native window/tray/exit request gate, current one-time decision and
  save/discard/cancel UI; preserve close-to-tray and Activity collection consent.
- Original review 5: recover import intent recorded before active-store pointer publication.

## Decisions and limits

Native document revisions are independent of watcher notifications. Source bytes and file
identity are included. Atomic publication still serves a different purpose from the pre-write
conflict check; arbitrary external writers do not participate in a filesystem transaction.
Normal graceful exit is reviewed. Forced process termination, power loss and unsaved in-memory
text recovery are not claimed safe; no plaintext recovery copy is introduced implicitly.

## Progress

Implementation, regression fixtures and documentation complete. Local all-scope verification
completed through the default affected entry point and failure/unexecuted-stage continuation below.
Windows close/tray/WSL vault and Suite acceptance remain unexecuted.

## Implementation mapping

1. GraphQL: sticky cross-origin payload exclusion and final-URL redaction check; real loopback
   regression matrix for 301/302/303/307/308, same/other origin and ordinary/secret variables.
2–3. Notes: one ordered writer, document/edit generations and latest-open generation; delayed
   native replies cannot clean newer edits or replace a newer selection. React and model fixtures
   cover ordinary, incoming and conflict flows, plus duplicate save and switching during save.
4. Notes conflict: native content + physical identity + modification revision; every save and
   explicitly reviewed overwrite checks a fresh expected revision independently of watcher timing.
   Deletion/replacement and stale overwrite retain the draft. Atomic publication is not an external
   filesystem transaction; a writer racing after the final comparison is a remaining narrow limit.
5. Notes quit: window close, tray quit and graceful exit share one native request/decision gate.
   Save/discard/cancel and pending-save failure preserve the editor. Close-to-tray remains hiding;
   actual accepted exit still stops Activity. Forced termination/power loss have no new recovery guarantee.
6. Source commit: native index entries (mode, stage, blob, path), HEAD and repository identity hash;
   approval passes the revision and native mutation checks it again. External add/reset, same-path
   blob change and HEAD change regressions. This does not lock out arbitrary external Git writers
   or trusted hooks in the final compare-to-command interval.
7. Webhook Logs: bounded source memory projection, Suite role/installation admission, explicit
   destination review, revision-bound claim/ack and operation deduplication. No raw path/clipboard/
   legacy launch. React receiver + contract fixtures and actual four-product Windows fixture added.
   Two parity entries corrected to pending Windows integration; old producer evidence is not E2E evidence.
8. Original Runtime finding + added handoff: supervisor catches TERM before user code; fresh live
   marked member admission supports retired/zombie leader; marker uses stdin and log redaction;
   framed noisy probes, timeout fallback, reserved observation budget, slower polling and removed
   unguarded signal plan. Tests use owned groups with no extra pre-completion reaper. Production
   already has a concurrent wrapper wait owner, contrary to the handoff's blanket reaper claim.
9. Original import finding: Activating journal with base pointer can resume, cancel or roll back;
   source/staged/base checks remain required and newer activated data is preserved.
10. Additional concrete fixes: all Rust explicit cross-crate includes scanned with transitive
    consumer propagation and negative injection fixture; Windows CI job label corrected; Cargo
    scaffold comments removed; both watchers contain poisoned state without restarting DB writes.

## Additional findings kept separate

- Broad file splitting and cross-app source extraction are architectural follow-ups, not marked
  fixed by this PR. Extracting only Suite would still leave browser/process-tree include edges;
  the strengthened include guard protects the existing graph in the meantime. A crate extraction
  must include all real consumers and preserve Tauri cfg/features before deleting the manual map.
- No Tauri dependency upgrade or advisory exception extension is made. The 2026-11-30 expiry
  remains a release-maintenance deadline; reassess the upstream GTK/urlpattern chain on the next
  Tauri update. No package upgrade is necessary to repair the reported runtime/data bugs.

## Verification plan and environment boundary

One final `pnpm verify:affected` after this whole implementation, fixtures and documents are
complete. CI test/resolver and root manifest changes select all; do not repeat with verify:all.
The common resource wrapper owns CPU/workers/memory and the cross-worktree lock. If failures
occur, collect findings, finish corrections, then run only failed/unexecuted or affected stages.
No local Docker, services, firewall or shared network changes. Windows native close/tray,
real WSL cold start, packaging and Suite integration remain unexecuted unless a new exact-source
hosted result is recorded. No main merge, release or deletion of the active unmerged worktree.


## Local results (2026-09-21)

- `pnpm verify:affected` selected **all**. Metadata/resource/scope/resolver regressions passed,
  including the injected cross-crate include failure. The first run stopped on new Notes fixture
  type errors and an omitted watcher error union; these were corrected together.
- Continuation ran remaining frontend builds, all frontend tests, remaining TypeScript checks,
  whole-workspace Rust check and product-foundation metadata. The complete frontend run found
  two failures (Logs add-source wiring and a Git dialog expectation); affected tests/builds were
  corrected and rerun. Knowledge quit effect cleanup typing was corrected at the same time.
- All frontend builds/type checks and bundle/route ceilings passed. **1,893 frontend tests pass**
  after corrections (1,892 in the initial scope plus one added Logs request-consumption regression).
  Successful unrelated package tests were retained rather than rerun.
- Whole-workspace Rust `check` and final `clippy --all-targets -- -D warnings` passed; `fmt --check`
  passed. A test-only owned-child constructor omission was corrected before the full test compile.
- Whole-workspace Rust tests ran with `--no-fail-fast`: 2,668 passed, 3 failed, 3 ignored. The three
  failures were existing Source pipe fixtures missing the new required index revision. Fixtures
  now obtain it through native preview; the same staged-path blob replacement regression was
  added. All **14 Source pipe tests passed** on correction. Final distinct Rust total: **2,672
  passed, 3 ignored**, with no known remaining executed-test failures.
- Native loopback GraphQL matrix, cancellation and 307/308 protections passed. Notes delayed
  React/model cases, native external edit/delete/replacement, graceful quit decision/UI, Runtime
  retired leader/noisy probe/timeout/private marker, import activation intent, Git index/HEAD and
  Webhook peer/revision/replay/receiver regressions passed in the scopes above.
- Changed Windows fixture entrypoints have syntax checks; this is not execution evidence.
  The Windows suite fixture adds history and saved-fixture delivery through actual review/Logs,
  unreviewed/stale/replay rejection and producer preservation. Two parity entries remain pending
  that exact-source Windows run. Native close/tray, real WSL cold start, installers and packaging
  are **not run** here. Existing ignored tests: actual WSL process interoperability; installed
  native LSP fixture; isolated parent-death child entrypoint (the enclosing parent test runs).
- Every heavy command used the shared verification lock and 4 CPU / 2 Cargo jobs / 2 test workers /
  8 GiB memory cap. Largest measured cgroup peak: 6,444,978,176 bytes, below the cap. No local Docker,
  daemon, firewall, route, shared network or existing service changes.
- Local logs: `.git/devbox-evidence/review-corrections-2026-09-21/` (affected, remaining,
  final-stages, targets). Main/remote main still `d6208b37`; one reused worktree, one correction
  branch. No main merge/release. GitHub CI results are separate from these local passes.


## v0.8.1 release authorization and preparation

The user subsequently explicitly authorized CI-gated merge, owned branch/worktree/temp/process
cleanup and publication of v0.8.1. Keep all source preparation in PR #569. Four product Cargo,
Tauri and package manifests and the four local Cargo.lock entries now agree on 0.8.1; engine
versions and third-party dependencies are unchanged. Both packaged-smoke/installer configurations
also use 0.8.1 for the Suite and all four products. Product foundation validation compares
all three manifests instead of pinning 0.8.0, and Suite workflow fixtures read the product version.
Final CI must cover this source before merge. Candidate dispatch, acceptance, annotation and
promotion will use exact current main and the same immutable candidate bytes. Prior Windows
release evidence is not reused as proof for v0.8.1. No local Docker/service/network mutation.

Version preparation checks: locked Cargo metadata, catalog/product foundation, packaged-smoke
configuration and Suite release-contract checks passed. Compiler suites already passed locally;
the versioned final PR source will receive the required final CI and exact-main candidate gates.

Version-head CI 35547702918 found the generated notices still bound to the pre-version
Cargo.lock digest. The official generator updated only that digest; third-party inventory and
dependency versions did not change. Final-head CI must pass after this metadata correction.
