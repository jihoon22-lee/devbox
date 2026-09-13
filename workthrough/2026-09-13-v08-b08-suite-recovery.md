# B08 — Control Center and recoverable suite delivery

Refs #550, #541, #542. B08 is one implementation/importer/installer/fixture PR.
Base is the in-progress B07 integration (f902759); rebase onto its accepted main
before final PR verification. B07 and B06 verification evidence is tracked in their
own workthroughs. No production installation or migration is authorized by a test.

## Implementation boundary

Reuse Manager's existing doctor, Data Inspector, redacted support bundle, Related
Tools and package-only Dev Setup. Keep its legacy batch installer separate from
new generation activation. Expose only four user products. Inventory must distinguish
unknown installer evidence from absence and identify exact Manager-owned portable
registrations; arbitrary uninstall strings and guessed executable paths are denied.

Compose the existing owner importers with consistent WAL-aware source acquisition,
explicit source/destination reviews and resumable receipts. Package stages preserve
the previous generation; quiesce, activation, health and commit are explicit phases.
A verified bootstrap helper owns Control Center replacement. Post-commit downgrade
and uninstall preserve new user data and external vault/repository edits.

The v0.8 asset goal is one suite setup, four product ZIPs, manifest and notices;
its actual manifest/assembly/installer contract must be implemented and verified
before B09 uses it. No public RC or unverified stable promotion.

## Verification policy

Commit checks are minimum syntax/types and diff/plan review. Detailed verification
runs once after all B08 implementation/importers/fixtures are assembled. Add only
missing native installer and fault-injection acceptance, retain unrelated passes.
All network/service-changing fixtures run on disposable hosted Windows VMs. Never
provision local Docker/WSL or mutate existing services, firewall rules or routes.


## Existing Manager tools as an actual second consumer

Manager UI/tests moved to control-center-features with legacy wrappers. Control
Center consumes its controlled tools modes without legacy catalog/install refresh
or install AppLink handling. Styles are scoped to the shared tools root; standalone
Manager retains its document reset. Existing native doctor/diagnostics/support,
Related Tools and reviewed package-only Dev Setup commands are embedded through a
local Control Center route allowlist. No legacy batch install or bootstrap runs.
Official URLs resolve against the native curated list without installation probes.
The new tool component/capability and catalog dependency edges are registered.
Dependency locks add only the internal consumers; unrelated transitive pnpm changes
from lockfile resolution were removed. No B08 detailed test/build/audit has run.
