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

Owned WSL stop validates the exact NUL-delimited marker and current PID/group/session
and signals in one bound invocation. The TERM grace starts after signal delivery;
KILL revalidates independently. Only an explicit absence witness settles a raced exit.

같은 native owner에서 실행 중인 로그를 다시 열면 쓰기 측의 상태와 잠금을 공유한다.
읽기 측은 추가 쓰기와 rotation을 같은 상태에서 관찰하며, 쓰는 중인 파일을 복구하거나
이름을 바꾸지 않는다. 약한 참조는 종료된 stream을 붙잡지 않으며, 같은 경로의 폴더가
교체돼도 옛 상태를 재사용하지 않는다. 마지막 live handle이 해제된 뒤에는 기존처럼
디스크의 segment를 읽어 복구한다.
