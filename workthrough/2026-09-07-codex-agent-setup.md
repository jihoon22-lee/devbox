# Prepare the Codex environment for v0.8 work

## Change

The user requested completing the agent environment before milestone implementation.
This is one preparation PR referencing #541/#543; it does not implement or close B01.

- `AGENTS.md` now routes to relevant conventions and current milestone contracts.
  Its size falls from 6,790 to 3,962 bytes. Historical release evidence and the
  detailed release procedure remain verbatim in `docs/release-evidence.md` and
  `docs/release-policy.md`.
- `CONVENTIONS.md` resolves WSL validation and commit/CI ordering conflicts, makes
  the v0.8 B01–B09 grouping explicit, preserves existing validation gates, and
  defines concise workthrough/context handoff rules.
- `.agents/skills/` adds `devbox-change`, `devbox-migration-review`, and the
  explicit-only `devbox-release`. The release invocation policy lives in its
  `agents/openai.yaml`.
- `docs/codex-setup.md` records official sources, the user-selected host settings,
  effective-config checks, restart requirements, and recovery instructions.

## Personal host changes

Outside Git, `~/.codex/config.toml` now selects Astra/high, a 512,000-token context,
a 430,000-token automatic compaction threshold, and experimental context management.
All unrelated settings were preserved and the prior configuration was backed up.
The personal `workthrough` entrypoint, template, and existing helper documents were
shortened to one evidence-focused record per PR/task; the full prior skill was backed up.
Existing OpenAI Docs MCP and GitHub tooling were retained. No plugin cache was edited.

## Verification

- Four skill entrypoints pass the skill creator validator. Local Markdown/skill
  targets and release invocation YAML pass focused checks.
- Relocated release procedure and historical evidence match the parent commit verbatim.
- Codex CLI 0.153.4 app-server with strict config loading returns all five requested
  settings and discovers the three repository skills plus personal workthrough with
  zero discovery errors. Account eligibility was checked locally without recording
  account identifiers or credentials.
- Local `codex debug prompt-input` includes the two automatic skills and excludes
  the explicit-only release skill from the initial catalog; no model turn was submitted.
- `pnpm verify:affected`: full frontend/Rust scope selected because the new skill
  policy YAML is an unclassified workspace path. Passed (exit 0): frontend build,
  bundle checks, tests and typecheck; Rust check, clippy, fmt and tests.
  Observed 1617 frontend and 2110 Rust test passes (including doc-tests).
- GitHub CI is tracked by the preparation PR's final checks.

## Limits

New tasks on the configured host are required to use the settings. Effective config
and skill discovery were verified; long-running history transitions and the exact
interaction of the 430K threshold with the experiment were not exercised.
No Windows app execution, package candidate, migration, tag, or release was performed.
