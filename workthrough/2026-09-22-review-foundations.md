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
