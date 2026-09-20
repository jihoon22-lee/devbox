# webhook-host

Native domain logic, product-host adapter, migration readers and regression fixtures
extracted from `webhook-lab`. This crate has no desktop entrypoint, app identifier,
standalone UI, installer or runtime executable fallback. Its existing Rust library
alias preserves internal call sites; its package is `devbox-webhook-host`.

The owning v0.8 product controls initialization, lifecycle and native admission.
Original source namespaces remain read-only migration inputs; engine reuse does
not grant cross-product authority. The retained fake LSP executable (editor only)
is a test fixture and is not a product or public asset.

[Historical feature documentation](https://github.com/jihoon22-lee/devbox/blob/005b942da8628ae19117505c903a127f41998192/apps/webhook-lab/README.md).
