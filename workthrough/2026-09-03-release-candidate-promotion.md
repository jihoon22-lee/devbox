# Verified Windows candidate promotion

## Overview

Stable releases now promote the exact Windows package candidate that already passed packaged-runtime
and installer-lifecycle acceptance. The release workflow no longer recompiles the same 15 applications;
it resolves a trusted candidate, verifies its provenance and all 32 asset digests, creates a draft, and
retains the existing independent download verification before publication.

## Context

- The v0.6.0 candidate and release both used source commit
  `d2fa25a0a1f087459838449daded00c0b09764b4`.
- Candidate packaging took about 59 minutes and release packaging rebuilt the same applications for
  about 67 minutes. Draft creation and publication verification took about one minute.
- The candidate already records the intended tag, source commit, repository, workflow run, and SHA-256
  identity of every public asset, and its two downstream Windows acceptance jobs gate run success.

## Changes made

### Trusted candidate resolution

Files:

- `.github/scripts/resolve-release-candidate.py`
- `.github/scripts/test-resolve-release-candidate.py`

The resolver queries artifacts by the exact stable tag and commit identity. It accepts only a non-expired
artifact from the repository's `main` branch whose `Windows package candidate` workflow-dispatch run is
complete and successful. Forks, other workflows/events, mismatched commits, malformed digests, failed
runs, and expired artifacts fail closed.

```python
return f"windows-package-candidate-{tag}-{commit}"
```

Including the intended tag in the artifact name prevents two release intentions for one commit from
being confused. Candidate artifacts are retained for 14 days instead of seven.

### Provenance-bound asset verification

Files:

- `.github/scripts/verify-downloaded-release.py`
- `.github/scripts/test-verify-downloaded-release.py`
- `.github/workflows/windows-package-candidate.yml`

Candidate verification now requires the expected repository and workflow run as explicit inputs and
compares both with the signed-off metadata. The existing tag, commit, manifest, size, SHA-256, unique
name, and exact 15-app/32-file checks remain intact.

### Stable promotion and prerelease compatibility

Files:

- `.github/workflows/release.yml`
- `.github/scripts/test-release-candidate-promotion-config.py`
- `.github/scripts/test-windows-package-candidate-config.py`
- `.github/scripts/check-catalog.sh`
- `.github/workflows/ci.yml`

For a stable tag, the release workflow downloads the exact candidate from its successful cross-workflow
run, independently verifies its assets and provenance, and uploads those flat assets to a new draft. It
then downloads and verifies the complete draft before making it public, as before. A missing, expired, or
mismatched candidate fails before release creation.

The explicitly gated manual prerelease path retains its existing Windows build. This preserves the
documented prerelease escape hatch while the unpublished candidate workflow remains stable-only.

### Release procedure documentation

Files:

- `AGENTS.md`
- `CONVENTIONS.md`
- `docs/architecture.md`
- `docs/development.md`
- `docs/windows-guide.md`

The stable procedure is now explicit: merge the release source, run and pass an exact-main candidate,
then create an annotated tag at the same commit. A manual Release dispatch is a retry path for an already
existing annotated tag, not a replacement for the candidate gate.

No application source, dependency, lockfile, version, or public asset contract changed.

## Verification results

```text
python3 .github/scripts/test-resolve-release-candidate.py       5 tests PASS
python3 .github/scripts/test-verify-downloaded-release.py       PASS
python3 .github/scripts/test-build-candidate-metadata.py        PASS
python3 .github/scripts/test-windows-package-candidate-config.py PASS
python3 .github/scripts/test-release-candidate-promotion-config.py PASS
python3 .github/scripts/test-validate-release-input.py          10 tests PASS
bash .github/scripts/check-catalog.sh                           PASS
Ruby YAML parse                                                  PASS
actionlint                                                       PASS
ruff check / format --check                                     PASS
git diff --check                                                 PASS
```

`pnpm verify:affected` intentionally selected a full audit because the CI verification driver changed:

```text
15 application frontend builds and bundle budgets               PASS
frontend package/application tests                              PASS
additional TypeScript package checks                            PASS
Cargo workspace check / Clippy / fmt                            PASS
Cargo workspace tests and doc-tests                             PASS
existing WSL interoperability test                              1 ignored as documented
Exit code                                                        0
```

GitHub Actions CI remains the required merge gate.
