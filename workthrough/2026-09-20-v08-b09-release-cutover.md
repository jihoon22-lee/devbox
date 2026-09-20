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
