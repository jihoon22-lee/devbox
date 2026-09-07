---
name: devbox-release
description: Prepare, verify, or publish an explicitly requested Devbox release using exact-main candidate evidence and installer acceptance. Follow the user's requested phase and version.
---

# Devbox release

Read [release policy](../../../docs/release-policy.md),
[release evidence](../../../docs/release-evidence.md), and CONVENTIONS §4/8.
Inspect the current release workflow, input validator, catalog, and manifest before
running a release operation. Historical evidence is not evidence for a new release.

- Establish the requested version, phase, source commit, and existing authorization.
  A preparation or review request does not authorize creating a tag or publishing.
  Reuse explicit authorization already present; ask only for an unresolved action
  after its candidate and evidence are concrete and reviewable.
- Confirm exact current main and successful CI. Use the existing Windows candidate
  workflow and its declared coverage, with assembly, packaged runtime, installer,
  and applicable migration acceptance. Record checks unavailable in this host.
- For v0.7 topology, preserve 15 apps/32 public assets and the existing shard
  contract. For v0.8, use the implemented, verified WP08 manifest; do not treat
  four products or the seven-asset goal as an already implemented release contract.
- Before tagging, verify candidate source, version, repository, workflow identity,
  expiry, and asset digests. Create the annotated stable tag on the same source
  only within the authorized publication scope.
- Promote the matching candidate without rebuilding. Missing, expired, mismatched,
  or failed evidence blocks promotion. Preserve the stable final verifier's
  `always()` and explicit preflight/draft-stage success conditions.
- After publication, verify fresh-download names/counts/digests and release flags.
  Update the release ledger and concise workthrough with commit/run/asset evidence.
  Complete branch/worktree cleanup for any related merged repository changes.
- Never create public RC/prerelease tags or releases without the user's explicit
  request. Follow the guarded workflow input path; don't invent a bypass.
