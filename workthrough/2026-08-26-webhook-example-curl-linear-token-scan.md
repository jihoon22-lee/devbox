# Webhook Lab Example Curl Linear Token Scan Workthrough

- Date: 2026-08-26
- Branch: `feat/webhook-lab/example-curl-linear-token-scan`
- Base: `5b47b026e4f09eb9656182a3fa8186a0eef0b8cb` (`main`)
- Trigger: frontend CI on PR #415 timed out in the existing example-curl bounds fixture
- Status: implementation and focused local verification complete; CI pending

## Outcome

The example-curl credential detector now tokenizes bounded response metadata once and validates each
candidate with linear-time prefix, AWS-key and three-segment JWT checks. The previous unanchored JWT
regular expression retried its greedy segment branch at every character of a long ordinary header
value. Under concurrent frontend tests, the existing bounds fixture spent 8.8 seconds in that scan
and exceeded Vitest's five-second per-test deadline.

The replacement preserves the existing fail-closed behavior: known token prefixes may still appear
inside a delimited candidate, private-key markers are rejected, JWT segments remain bounded by the
existing header/body limits, and detected candidates are replaced completely with `[REDACTED]`.
There is no timeout increase and no relaxation of path, header, body, JSON or output bounds.

## Verification

- Existing `src/lib/exampleCurl.test.ts`: 9 passed.
- The formerly timing-out bounds test is included in that suite; the complete file used 13ms of
  test execution and 1.45 seconds wall-clock in the Linux-native dependency mirror.
- Webhook Lab frontend build and GitHub Actions remain the next gates.

## Files changed

- `apps/webhook-lab/src/lib/exampleCurl.ts`
- `workthrough/2026-08-26-webhook-example-curl-linear-token-scan.md`
