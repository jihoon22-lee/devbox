---
name: devbox-migration-review
description: Review Devbox migration, data ownership, authority, and recovery changes against preserved legacy behavior. Use for importer, IPC authorization, secret ownership, or installer recovery work; not routine UI edits.
---

# Devbox migration review

Use [CONVENTIONS.md](../../../CONVENTIONS.md), the current relevant WP issue, and
its requirement/acceptance IDs as the contract. For v0.8, #541 is the plan, #542
defines acceptance, and #543–#551 own implementation. Evaluate only the affected
boundaries; don't turn every review into a suite-wide audit.

## Trace ownership and authority

- Identify the legacy source, destination namespace/store, writer, caller identity,
  schema/version, and compatibility behavior. Separate installer identity, data root,
  single-instance identity, and DPAPI binding decisions.
- Trace backend capability/authorization and IPC peer/session validation. Hidden
  routes/buttons do not enforce permissions. Check version mismatch, replay,
  request bounds, deadlines, cancellation, and stale results where relevant.
- Reuse existing `SecretReference`, platform Sealer, AppLink, and integration
  boundaries. Keep Runtime-owned processes separate from actions on external
  processes. Do not expose raw secrets through fixtures, logs, or public records.

## Check failure and recovery behavior

- Require consistent SQLite snapshots that account for WAL and writers; a live
  `.db` copy or an immutable reader that ignores WAL is not a migration backup.
- Preserve legacy logical data/schema and write to the v0.8 destination namespace.
  Distinguish user-owned settings/templates from derived indexes and temporary data.
- Examine dry-run plans, ID/conflict mapping, repeat imports, journal/resume,
  corrupt/future schemas, cancellation, and source/destination validation.
- For suite activation, trace stage/validate/quiesce/activate/health-check/commit
  and recovery. State what rollback preserves and what it cannot reverse, including
  newly created v0.8 data. Do not claim a transaction across executables, registry,
  databases, and external vaults without evidence.

## Report evidence

Prepare synthetic fixtures during implementation and run detailed migration/authority
acceptance when the complete PR bundle is implemented, following CONVENTIONS §5.
Before then, run only a minimal reproduction needed to resolve a concrete defect
or design uncertainty. Do not repeat passing checks without a related change or
new risk. Use synthetic fixtures and existing tests. Do not run production migration,
installation changes, or destructive experiments merely to complete a review.
For each finding, provide severity, file/location, concrete failure scenario,
violated contract, and the missing or failing check. Record uncertain assumptions
and unperformed Windows observations separately from confirmed findings.
Put the result in the existing PR/workthrough packet when one exists.
