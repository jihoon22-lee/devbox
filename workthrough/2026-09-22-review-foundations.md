# Shared foundations (review stage 4)

- Prerequisite: #572 merged as abfed742; CI 35614204770 and native/hosted WSL2
  35614204652 PASS on the first attempt. Earlier stages: #570 and #571.
- Suite bus/scope/peer/health implementation moved to suite-runtime, an explicit
  Cargo dependency of all four products. Product admission and domain handlers stay
  in hosts. Remove obsolete shared-source CI edges; assert reverse Cargo consumers.
  No new external dependency/version, executable or renderer authority.
- Draft recovery uses stable native error codes through both transport adapters;
  UI text no longer controls regeneration. Other failures preserve the preview.
- Shared committed-write outcome carries durability warnings; Editor uses the same
  post-commit parent-sync contract without weakening revision/identity checks.
- Closed-store digest checks cancel/deadline per block, retaining all integrity passes.
  File enumeration bounds visited entries independently, including empty/ignored dirs.
- Separate focusable/tabbable helpers, CSS visibility, positive tabindex and radio groups.
  Actual Chromium Tab/Shift+Tab fixture is part of a11y tests, separate from jsdom.
- Fixtures: crate-consumer scope, typed draft recovery/non-recovery, injected post-commit
  warning, cancelled/expired digest, empty-directory budget, CSS/actual keyboard order.
- Validation: completed the full affected audit (resolver selected all). Frontend
  builds/types/bundle budgets and all Vitest suites PASS; actual Chromium CSS and
  Tab/Shift+Tab PASS. Rust workspace check/Clippy/fmt and all tests PASS.
- Corrections collected during validation: secrets reverse-dependency expectation
  includes suite-runtime; browser fixture starts at dialog focusFirst and uses the
  CDP Shift bit; one import was reordered. Only failed or not-yet-run checks were
  resumed under the same resource supervisor. No passed compiler/test scope repeated.
- New regression fixtures for digest cancellation/deadline, post-commit warning and
  directory work budget passed. Cargo versions unchanged; notices regenerated from lock.
- Remaining merge gate: final GitHub CI and Windows native/hosted WSL2 acceptance.
- First CI found stale Suite source/implementation paths in the parity inventory and
  a browser fixture cleanup race after successful keyboard assertions. Updated all
  three inventory paths, close Chromium through CDP and await owned process/pipes
  before bounded profile removal. Focused catalog contracts and browser/cleanup PASS.
- First CI dependency policy and Linux Rust PASS; Windows check/Clippy/tests completed
  without failures. Superseded native acceptance was cancelled early to avoid wasting
  a long run on a head requiring metadata/fixture corrections. Final head must rerun CI.

- Native acceptance caught a real extraction regression: library CARGO_PKG_VERSION
  (0.1.0) replaced the host version (0.8.1) in package capture, Describe and health.
  Hosts now explicitly pass their version to plugin/bootstrap capture; all shared
  paths use it. Add a wire-identity regression and a foundation contract rejecting
  library-version identity; register Suite-only changes for native acceptance.
  Revalidate the affected native crates/contracts, then final-head CI/native gates.
- Version correction validation: foundation/catalog contracts and affected native
  check/Clippy/fmt/tests PASS. Clippy's test-module placement finding was corrected
  before resuming unfinished checks. Native artifact evidence explicitly records
  `installation_version_mismatch` for the superseded extraction; its failed gates
  are not counted as PASS. Final corrected source still requires CI/native acceptance.
