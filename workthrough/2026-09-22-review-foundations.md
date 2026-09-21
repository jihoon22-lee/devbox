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
- Validation pending: entire implementation/fixtures/docs before one full affected audit
  (CI resolver changed), followed by final CI and Windows native acceptance.
