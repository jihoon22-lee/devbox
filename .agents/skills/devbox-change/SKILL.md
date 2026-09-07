---
name: devbox-change
description: Implement or maintain Devbox changes with milestone-aware PR grouping, affected verification, and concise evidence. Use for Devbox implementation, fixes, refactoring, or repository tooling changes.
---

# Devbox change

Read the repository [AGENTS.md](../../../AGENTS.md) and relevant sections of
[CONVENTIONS.md](../../../CONVENTIONS.md). Resolve these links from this skill directory.

## Establish the change boundary

- Identify the requested user behavior, relevant WP/requirement IDs, prerequisites,
  and acceptance criteria. Read current GitHub issue bodies using the available
  GitHub connection or `gh`; use local specifications for versioned contracts.
- For v0.8, follow the B01–B09 grouping in CONVENTIONS §8. Keep implementation,
  importers, regression fixtures, and documentation for one bundle in one PR.
  Explicitly requested preparation work can stand alone without completing a WP.
- Check the existing worktree, branch, and uncommitted changes. Use a dedicated
  branch/worktree when needed; preserve user-owned changes. Reuse existing domain
  code and actual consumers before introducing shared abstractions.

## Implement and verify

- Read only the affected app/module guidance and contracts. Keep mechanical moves
  separate from semantic changes in commits, with a single owner for shared edits.
- Select focused checks for the changed behavior. Use WSL for frontend and portable
  Rust validation; distinguish Windows compile checks from actual Windows execution.
- Run `pnpm verify:affected` from the root after the final local changes. Honor `all`
  if the resolver selects it. Use `verify:all` for the cases in CONVENTIONS §5.
- Keep local verification within the shared resource budget and worktree lock in
  [verification operations](../../../docs/verification.md). One full run satisfies
  both all-scope affected verification and the explicit full audit; do not duplicate it.
- Review failures and fix those attributable to the change. Record unrelated or
  environment failures accurately; never substitute a skipped check for a pass.

## Leave reviewable evidence

- Update one `workthrough/YYYY-MM-DD-scope.md` per PR bundle with the problem,
  important decisions, affected paths, actual checks/results, and remaining limits.
  Link fixtures and CI runs instead of copying full diffs or successful logs.
- In the PR, include issue mapping, grouping rationale, behavior/data/authority
  changes, validation, and rollback limits when relevant. Close only fully satisfied
  issues; use `Refs` for partial work and never auto-close #541/#542 from an implementation PR.
- Follow existing authorization for GitHub mutations. When merge is in scope,
  wait for CI on the final PR changes and complete CONVENTIONS §8 cleanup.
- Before a context handoff, preserve scope, WP IDs, worktree/branch, decisions,
  verification, and the next action. Recheck live Git/CI state on resume.
