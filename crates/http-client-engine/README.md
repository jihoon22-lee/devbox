# http-client-engine

Native domain logic, product-host adapter, migration readers and regression fixtures
extracted from `api-playground`. This crate has no desktop entrypoint, app identifier,
standalone UI, installer or runtime executable fallback. Its existing Rust library
alias preserves internal call sites; its package is `devbox-http-client-engine`.

The owning v0.8 product controls initialization, lifecycle and native admission.
Original source namespaces remain read-only migration inputs; engine reuse does
not grant cross-product authority. The retained fake LSP executable (editor only)
is a test fixture and is not a product or public asset.

[Historical feature documentation](https://github.com/jihoon22-lee/devbox/blob/005b942da8628ae19117505c903a127f41998192/apps/api-playground/README.md).

MCP stdio cleanup uses one 750 ms monotonic deadline across cancellation notification,
stdin closure, graceful/forced process termination, authority polling and stderr joining.
Polling yields to Tokio; kill signaling is separate from reaping. Failed or unknown
termination remains a cleanup failure. Drop only signals owned processes and releases
handles; it does not wait or certify termination. Windows suspended-child assignment
failure uses kill-on-close followed by bounded root reaping.
