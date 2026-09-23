# Issue #574 stage 5 — Knowledge metadata scans

- R7: replace unbounded recursive directory collection with streaming traversal,
  entry/directory/depth/JSON-size/time budgets and cooperative cancellation.
  Incomplete scans return typed errors, never authoritative partial trees.
- Snapshot configured root and a scan ticket under the DB mutex; acquire filesystem
  identity and traverse outside that mutex on a blocking worker. Revalidate identity,
  root and ticket before publishing. Root changes invalidate tickets even across A–B–A.
  Supersession cancels older scans; two outstanding workers maximum, including stalled I/O.
- R8: one active metadata pair and one trailing refresh; latest intent gates both
  success and error. Wait for both tree/tag operations on failure. Lifecycle stop
  invalidates old work without reviving it on StrictMode restart. Preserve last good pair.
- Tree/tag consistency is eventual (filesystem vs derived SQLite index), not a single
  atomic filesystem/database snapshot. Cooperative budgets do not preempt kernel I/O.
- Regressions cover empty-directory/entry/depth/size budgets, mid-scan cancellation and
  deadline, identity/generation changes, actual note save with the DB available during
  scan, burst coalescing, stale success/error, lifecycle and trailing refresh, DOM
  retention after an incomplete pair. Existing native Windows/WSL acceptance retained.
- Validation: `pnpm verify:affected` passed (260 frontend tests, 194 Rust tests,
  build/typecheck/check/Clippy/fmt and scope/resource contracts). Log:
  `/tmp/devbox-574-stage5-verify.log`. Implementation/tests/docs were complete before
  this single combined run. Final CI and native acceptance remain required before
  merge. No local service/VM changes.
