# HTTP request lifetime (review stage 1)

- Scope: F1 cancellation registration/response ownership and bounded control admission;
  F5 one monotonic deadline across redirects, body preparation and response streaming.
- Register response ownership while holding cancellation routing, then publish the
  new generation. Failed registration leaves the previous generation current.
- Keep native authorization/replay checks; reserve eight independent HTTP cancel
  slots alongside 64 data operations. No unbounded bypass.
- Regression fixtures cover concurrent registration, supersession, early/stale/duplicate
  cancel, registration failure, saturated admission and delayed redirect headers/body.
- Existing request/redirect/cancellation tests remain part of affected Rust validation.
- Validation: pending final affected checks and GitHub Actions CI. No Windows GUI
  execution claimed; this stage changes native request logic and admission only.
