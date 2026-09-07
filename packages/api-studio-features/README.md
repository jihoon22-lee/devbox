# API Studio features

Requests/Protocols, Webhooks and Transforms are shared by their legacy app
entry points and API Studio. Each export is loaded separately. Existing tests
move with their implementations; run `pnpm --filter @devbox/api-studio-features test`.

Legacy entry points retain their original native command transport. API Studio
sets a product transport before mounting any feature; native code validates the
session, component allowlist, request identity and caller. Browser previews keep
the existing mock behavior. CSS is scoped by the `api-feature-requests`,
`api-feature-webhooks` and `api-feature-transforms` host classes.

This extraction does not itself establish completed v0.8 migration or handoff
parity. Those are tracked in the B02 workthrough and source inventories.
