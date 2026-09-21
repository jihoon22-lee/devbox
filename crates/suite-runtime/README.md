# suite-runtime

Shared per-process Suite connection, installation/peer identity, transport and
read-only health observations. Four product hosts link this library explicitly.
The host supplies its product identity, domain handler and source list; command
admission, domain stores and mutation authority stay with that host. Merely linking
this crate does not approve an installation, launch a product or start a listener.

Extracted from the Control Center source shared by path inclusion. No new external
library, executable, service or renderer authority is introduced. Existing native
identity/transport tests move with the implementation; CI resolves all four hosts
through Cargo dependency edges instead of manually registered source paths.
