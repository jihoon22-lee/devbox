# ports-engine

Native domain logic, product-host adapter, migration readers and regression fixtures
extracted from `port-manager`. This crate has no desktop entrypoint, app identifier,
standalone UI, installer or runtime executable fallback. Its existing Rust library
alias preserves internal call sites; its package is `devbox-ports-engine`.

Workspace and its installed Agent control initialization, lifecycle and native admission.
Original source namespaces remain read-only migration inputs; engine reuse does
not grant cross-product authority. Listener actions revalidate the observed
endpoint and process identity; container rows produce a handoff rather than
terminating a process directly.

[Historical feature documentation](https://github.com/jihoon22-lee/devbox/blob/005b942da8628ae19117505c903a127f41998192/apps/port-manager/README.md).
