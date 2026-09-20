# B09 — Four-product source and release cutover

Refs #551, #541, #542. Accepted base: B08 main `005b942d` / PR #561;
final CI `35496405510`, installed completion `35495003866` and scoped predecessors.
B01–B08 are complete. B09 implementation is underway; no detailed B09 checks run.

## Planned implementation

- Keep 14 consumed domain/host libraries under named `crates/` engines; remove
  their standalone Tauri startup/build/config/resources and all 15 old frontend
  shells. Move the retained global-shortcut adapter into Control Center. Preserve
  engines, migration readers, test fixtures and explicit legacy aliases.
- Freeze the v0.7 catalog for migration/reference readers before publishing only
  four products. Update Cargo/pnpm graph, metadata, budgets, capabilities and notices.
- Convert candidate/assembly/runtime/installer/promotion/read-back to four product
  ZIPs + Suite setup + manifest + notices. Keep exact-main identity and same-byte
  stable promotion; no public RC, no candidate substitution or skipped gates.
- Synchronize parity/data acceptance, R01–R26/S01–S08 traces and current docs.
  Retain historical facts and separate modeled/native/packaged/manual evidence.

## Execution rules

Finish the entire implementation/fixtures/docs before detailed verification.
Intermediate commits stay local. One final full audit and required CI; repeat only
actual failed/affected scopes. Hosted disposable Windows/WSL only for tests that
change Docker, services or networking. B09 cannot claim physical IME/multi-monitor
execution from CDP events. No production data migration during fixtures.

## Mechanical native extraction

Move consumed Rust sources, existing fixtures and manifests to domain engine paths;
redirect relative dependencies/includes and source consumers. The following semantic
commit removes the standalone bootstrap/configuration and obsolete app directories.
The frozen `apps/legacy-v0.7-catalog.json` is migration/reference input, never the
public runtime/release product list. The fake LSP server remains test-only.

## Source and release cutover implementation

- Removed all 15 standalone shells after extracting their consumed engines; frozen legacy
  catalog remains read-only source discovery/provenance. Public catalog now exposes exactly
  four products. Retained Launcher UI tests and existing engine/domain fixtures.
- Removed standalone engine Tauri bootstrap/build/resources and renamed Cargo packages,
  preserving Rust library aliases. Lockfiles/notices follow the current dependency graph.
- Direct Suite routing remains the product path; retired executable launch is centrally
  rejected before spawn. Historical metadata readers/argv fixtures remain for migration.
- Updated 572 feature records and data mappings with accepted B02–B08 source evidence.
  Git/vault original files, Manager install provenance remain in place. Derived caches,
  managed runtimes and live processes are not falsely marked as copied user data.
- Two Windows shards build four products, including static Workspace helper and Suite
  bootstrap. Private LSP fixtures stay outside public assets. Pinned NSIS assembles one
  setup plus four ZIPs, manifest and notices; exact file closures/digests/source are checked.
- Reconstruct installer acceptance input from verified public bytes; test exact original
  private payload byte identity, including LF on Windows. Stable promotion has no rebuild
  fallback. Published acceptance downloads again and exercises all four full ZIPs.
- Candidate native domains run independently on disposable Windows, WSL2/Docker separately.
  Seven pinned baseline anchors are measured on the same VM as matching product workloads;
  unchanged B01 budgets cover startup/input/idle/warm/search/task/profile readback.
- Windows Cargo jobs restored to two after locking only the shared Tauri build-script
  staging copy. Dependency cache namespace and failure retention remain in place.
- Current documentation now describes four products and migration/recovery; old v0.7
  guides are preserved under docs/history/v0.7. R01–R26/S01–S08 mapping and unexecuted
  physical IME/monitor/reboot layer limits are explicit in docs/v0.8-acceptance.md.

## Verification state

No detailed B09 tests/build/Clippy/native checks have run during development. After all
implementation/fixtures/docs finish, perform one full local audit and required CI. Candidate
assembly/native/installer/WSL2 gates run only on exact current main after B09 source merge.
Final outcomes belong in #541/#542/#551 and Actions, without a result-only source PR.
