# Bound local verification before milestone implementation

## Change

Refs #541/#543. This preparation PR addresses verification load on the shared WSL
host; it does not implement or close B01.

- Both local verify commands share one cross-worktree lock and a resource supervisor:
  one package, two Vitest workers, two Cargo jobs, two Rust test threads, four CPUs,
  and nice +10. Supported systemd user scopes enforce memory high/max 6/8GiB,
  swap max 1GiB and CPU quota 400%. Unsupported hosts retain worker/affinity/priority
  controls with an explicit message; required mode fails before compilation.
- Cancellation forwards signals to descendants; errors remain failures. Each run
  records elapsed time, status, budget, sampled RSS/swap and available cgroup metrics
  under the Git common directory. CI retains its independent-runner concurrency.
- Only the known release skill invocation YAML avoids compiler scope. Local/CI
  validation enforces its explicit-only policy; unknown YAML/scripts still select all.
- AGENTS, conventions and the change skill link to `docs/verification.md`. The
  Codex setup guide reflects the user's successful new-session configuration check;
  no remote restart is needed on this host.

## Verification

- Resource/metadata regressions cover inherited limits, overrides, invalid settings,
  unavailable/required cgroups, failure status, cancellation with a grandchild, and
  exclusion/lock release across linked worktrees. Scope and runner regressions pass.
- A real systemd probe confirmed memory high/max and CPU quota controller values.
  A 64MiB high/max allocation fixture hit exactly 67,108,864 bytes cgroup memory
  peak and exited nonzero with a recorded result. An initial 32/48MiB fixture
  throttled until its 20-second harness timeout; its dedicated scope was terminated.
  No fixture processes remain. This does not establish an upper bound on run time
  under memory pressure.
- The updated change skill passes `quick_validate.py`; `git diff --check` passes.
- `DEVBOX_VERIFY_CGROUP=required pnpm verify:affected` passed with frontend/Rust
  scope `all`: frontend build/bundle/test/typecheck and Rust check/clippy/fmt/test.
  It recorded 1617 frontend and 2110 Rust test passes, including doc-tests.
  Elapsed 655.018s; cgroup memory peak 6,444,863,488 bytes
  (6.00GiB); sampled group RSS peak
  3,900,977,152 bytes; sampled swap peak
  140,091,392 bytes. Memory enforcement was active, exit 0.
  Final metadata/resource regressions and the systemd collection probe also passed.
- Final PR CI results remain attached to the PR checks; merge requires their success.

## Limits

Sampled RSS sums can double-count shared pages and miss brief peaks; use the cgroup
peak when available. No comparable unbounded baseline was run, so elapsed-time or
memory improvement percentages are not claimed. Direct package/Cargo commands and
Windows packaging are outside the local wrapper. No app execution or release was performed.
