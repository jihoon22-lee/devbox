# Process cleanup (review stage 3)

- Prerequisite: #571 merged as 725b2c39. CI 35603507976 and native/hosted WSL2
  35603507963 passed on the first attempt.
- F6: one 750ms deadline shared across MCP cancel/grace/force/poll/stderr cleanup.
  Force escalation reserves half the remaining budget for reaping and authority checks.
- Use start_kill plus bounded wait; authority polling yields via Tokio. Unknown/nonempty
  authority is not success. Failed-spawn cleanup propagates unconfirmed termination.
- Drop/failed Windows assignment signal and release handles without synchronous polling.
- Regression coverage: unknown/nonempty authority with concurrent executor heartbeat,
  expired deadlines, forced escalation/reaping, duplicate cleanup, nonblocking Drop,
  unassigned child and existing owned descendant/stdio tests.
- Validation: `pnpm verify:affected` PASS (315s), check/Clippy/fmt plus API Studio
  42, HTTP engine 147 and shared-source Workspace 182 tests. Windows CI/native
  acceptance remain required; no repeated local run is planned.
