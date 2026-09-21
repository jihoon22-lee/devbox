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
- Validation: pending whole-branch affected and CI checks. Windows behavior must pass CI.
