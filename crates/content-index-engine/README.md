# content-index-engine

Native domain logic, product-host adapter, migration readers and regression fixtures
extracted from `everything-plus`. This crate has no desktop entrypoint, app identifier,
standalone UI, installer or runtime executable fallback. Its existing Rust library
alias preserves internal call sites; its package is `devbox-content-index-engine`.

The owning v0.9 product controls initialization, lifecycle and native admission.
Original source namespaces remain read-only migration inputs; engine reuse does
not grant cross-product authority.

[Historical feature documentation](https://github.com/jihoon22-lee/devbox/blob/005b942da8628ae19117505c903a127f41998192/apps/everything-plus/README.md).

인덱싱 batch의 파일 행·본문 정리·완료 commit은 하나의 SQLite transaction으로 처리한다. 중간 DB 오류와 commit 실패는 transaction을 rollback하여 같은 connection에서 다음 재시도를 진행할 수 있다.
