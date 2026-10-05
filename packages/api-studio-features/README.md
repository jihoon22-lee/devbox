# API Studio features

Requests/Protocols, Webhooks and Transforms provide API Studio's feature UI.
Each export is loaded separately. Run the affected tests with
`pnpm --filter @devbox/api-studio-features exec vitest run <file>`; complete
product verification follows the repository verification policy.

API Studio sets a product transport before mounting any feature. Native code
validates the session, component allowlist, request identity and caller. Browser
previews use explicit fixture behavior. The former standalone legacy entry points
are removed. CSS is scoped by the `api-feature-requests`, `api-feature-webhooks`
and `api-feature-transforms` host classes.

Current behavior and product evidence are described in the
[API Studio README](../../apps/devbox-api-studio/README.md) and the
[product readiness roadmap](../../docs/superpowers/plans/2026-10-03-product-readiness/00-roadmap.md).

Request and collection assertions run JavaScript regular-expression matching in
an isolated worker with a 500 ms limit per expression. Cancellation terminates
the worker; a timeout fails that assertion without blocking the renderer.
