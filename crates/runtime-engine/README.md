# runtime-engine

Native domain logic, product-host adapter, migration readers and regression fixtures
extracted from `run-manager`. This crate has no desktop entrypoint, app identifier,
standalone UI, installer or runtime executable fallback. Its existing Rust library
alias preserves internal call sites; its package is `devbox-runtime-engine`.

The owning v0.8 product controls initialization, lifecycle and native admission.
Original source namespaces remain read-only migration inputs; engine reuse does
not grant cross-product authority. The retained fake LSP executable (editor only)
is a test fixture and is not a product or public asset.

WSL jobs publish their supervisor identity before executing user code and wait
for the host's acknowledgement after marker/PID/group/session validation. Missing,
incorrect or timed-out acknowledgement prevents execution; task stdin remains
closed. This also preserves status and logs for immediately exiting commands.

[Historical feature documentation](https://github.com/jihoon22-lee/devbox/blob/005b942da8628ae19117505c903a127f41998192/apps/run-manager/README.md).

Natural WSL completion requires an explicit leader-and-group absence witness from
one bound query. Command failure or unexpected output never proves absence. The
Workspace adapter retains distro/executable identity for observation and cleanup;
launch still validates project and filesystem authority.
