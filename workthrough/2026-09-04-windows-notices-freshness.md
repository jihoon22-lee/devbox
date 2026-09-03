# Windows candidate notices freshness

## Overview

The second exact-main v0.7.0 candidate proved that all 15 packaged applications launch correctly,
but installer acceptance found that an unchanged-version Port Manager installer contained the
v0.6.0 third-party notice bytes. Publication remained blocked and no v0.7.0 tag or release was
created.

This correction makes generated notice bytes platform-stable and removes only the exact Tauri
bundle staging directory before each application package. It retains the shared Rust dependency
cache and avoids a broad Cargo clean, while ensuring an old NSIS resource tree cannot be reused by
a newly generated installer.

## Evidence and diagnosis

Candidate workflow `33773256130` ran against exact source
`062d891067aa15fae20a98b26a38df4a14c0fee6`.

- Package shards 01, 02 and 03 passed in 28m33s, 27m39s and 32m10s.
- Linux assembly passed in 19 seconds with 15 apps, 32 candidate files and 31 manifest-declared
  assets.
- Packaged runtime acceptance passed all 15 application contracts in 7m3s.
- Installer acceptance stopped at Port Manager and uploaded complete failure and cleanup evidence.
- The baseline install and uninstall both completed, the app was correctly classified as
  `same-version`, and registry/install/app-data/integration residue counts were all zero.
- The candidate fresh install exposed notice SHA-256
  `7338ce834f2cf5e012fa006d441191a0041e4e6b01f982f83dfd9bf9573cf519`, exactly matching the public
  v0.6.0 CRLF notice asset instead of the v0.7.0 candidate notice SHA-256
  `e964b2b711a8b80e793a230d89e20581fe7874dcf841fa9c9d839297440f84a4`.

The detailed Windows log shows Port Manager itself compiled in the candidate run, so cleaning the
application Cargo package would add work without addressing the stale payload. The remaining
reuse boundary is Tauri's `target/release/bundle` packaging tree. The generated notice also lacked
an explicit checkout end-of-line rule even though Linux assembly and Windows NSIS packaging bind
to its exact bytes.

## Changes

1. `.gitattributes` now fixes `THIRD_PARTY_NOTICES.md` to LF in every checkout.
2. `build-windows-packages.ps1` resolves the constant `target/release/bundle` path beneath the
   repository, rejects a file or any reparse point, removes that exact staging tree, verifies the
   removal converged, and does so immediately before each app build.
3. The package workflow contract asserts the LF rule, bounded cleanup, reparse-point protection and
   continued absence of a broad `cargo clean`.
4. Release conventions, architecture and the v0.7.0 execution plan record the byte-identity and
   bundle-freshness boundary.

## Safety and efficiency

The cleanup path is a constant repository descendant rather than workflow input. It is never a
workspace root, user directory or unresolved environment variable. Previous app outputs are moved
into candidate staging before the next reset. Rust dependency artifacts remain available, so the
three-shard performance improvement is preserved.

## Verification

The source-level checks for this correction passed:

```text
python3 .github/scripts/test-windows-package-candidate-config.py
python3 .github/scripts/test-windows-installer-acceptance-config.py
python3 .github/scripts/test-windows-packaged-smoke-config.py
git diff --check
pnpm verify:affected
```

Because `.gitattributes` is a repository-wide policy file, the affected resolver deliberately
selected the full workspace. With frontend workspace concurrency and Cargo build jobs both capped
at one, verification passed all 22 frontend/package builds, 15 bundle budgets, frontend tests, two
extra TypeScript checks, Cargo check, Clippy with warnings denied, formatting, workspace tests and
doc-tests. The dependency policy also confirmed that generated notices still match both lockfiles.
A live resource check during Rust linking showed about 12 GiB available memory and one active
`rustc` process.

PowerShell is not installed in the WSL environment, so the focused local contract validates the
script text and safety invariants while the required GitHub Actions Windows job remains the native
parser and execution gate.

After required PR CI passes and the correction is merged, a new exact-main Windows candidate must
run all three package shards, assembly, packaged runtime and installer acceptance. Only a fully
successful replacement candidate authorizes the annotated v0.7.0 tag.

## Publication outcome

PR #539 passed required CI `33779657145` and merged as
`3a23f49c85aa3c3d04b86f227e8aa184ef964085`. Replacement candidate `33782002859` passed all three
shards, exact assembly, packaged runtime 15/15 and installer lifecycle 15/15. Port Manager's candidate
fresh install carried the expected v0.7.0 notice digest
`e964b2b711a8b80e793a230d89e20581fe7874dcf841fa9c9d839297440f84a4`, and all installer residue
counters were zero. That evidence authorized the annotated v0.7.0 stable tag.
