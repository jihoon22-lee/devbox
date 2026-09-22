# Knowledge preview and lazy recovery — #574 stage 3

- Branch: `fix/devbox-knowledge/preview-recovery`; base `46a78acc` after #576 merged.
- Scope: R3/R4. RecoveryBoundary owns a stable lazy-attempt token outside Suspense.
  Retry creates a fresh token/resource and reinvokes the loader; other boundaries and
  the NoteSessionProvider retain their state. WeakMap storage does not retain retired attempts.
  Knowledge feature and startup lazy imports use this mechanism. Browser module-cache
  failures can still persist; no automatic reload or unsafe draft loss is introduced.
- NoteDocument publishes a monotonic sourceVersion on path/content publication, including
  identical reopenings. Save/inspect status updates keep it stable. Preview snapshots capture
  this version and their original path. Both response application and synchronous rendering
  enforce ownership, including A-B-A transitions and errors before passive cleanup.
- Regressions written: real NoteSessionProvider draft retained through rejected loader retry,
  repeated failure, unaffected sibling loader; actual Notes DOM old-link hiding during new
  failure, identical reopen, batched A-B-A, original relative path navigation. Existing
  reverse-completion, mode-exit, unmount and actual Knowledge render recovery tests remain.
- Implementation and documentation complete. The first affected check found optional-props
  inference in the lazy wrapper; preserving ComponentProps fixed it. Final `pnpm verify:affected`
  PASS: 227 feature + 27 Knowledge tests, types, build and bundle checks. Rust scope none.
  Pending: PR, final CI/native acceptance and merge. No detailed tests ran during individual
  feature development.
