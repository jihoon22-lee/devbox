# data-migration

Shared SQLite online-backup snapshot and destination-owned import plan/journal
from the Control Center foundation, reused by API Studio's importer. The domain
importer owns source selection, schema normalization and activation of non-SQLite
stores. A destination SQLite transaction does not make browser storage, native
files and installer state atomic together.

Run `cargo test -p data-migration` from the repository root.
