# Workthrough: Devbox Manager WinGet Configuration v3 package-only flow

**Date:** 2026-08-30
**Branch:** `feat/devbox-manager/dev-setup-apply`
**Issue:** [#483](https://github.com/jihoon22-lee/devbox/issues/483)

## Outcome

Devbox Manager can now import a WinGet Configuration v3 document, review a
strict package-only projection, export a newly generated canonical document,
and apply each changed package after explicit confirmation. External YAML is
never executed or passed directly to WinGet.

The existing capability audit remains read-only. The new mutating flow is
Windows-only and intentionally excludes arbitrary DSC resources, uninstall,
fuzzy matching, interactive installers, PATH/registry editing, distro startup,
and reboot automation. Native Windows CI and physical acceptance remain the
release evidence for process ownership, UAC, installer, and cleanup behavior.

## Security and trust model

- The Rust-native picker accepts `.winget`, `.yaml`, and `.yml`; no renderer
  dialog permission or selected filesystem path crosses the IPC boundary.
- Raw input is allocated inside `Zeroizing<Vec<u8>>` before the first read.
  Partial reads and later exits wipe that buffer; parser-owned allocations are
  not claimed to be wiped.
- Input is limited to 256 KiB, 4,096 lines, 8 KiB per line, 32-space
  indentation, and 16 resources. Lexical preflight rejects control characters,
  tabs, directives, document markers, aliases, anchors, tags, and merge keys.
- `#[serde(deny_unknown_fields)]` models accept only the exact WinGet v3 schema,
  optional `dscv3` marker, `Microsoft.WinGet/Package`, and the reviewed package
  properties. Package IDs and resource names are de-duplicated
  case-insensitively.
- `_exist: false`, fuzzy matching, interactive mode, contradictory desired
  state, dependencies, commands, and other unreviewed fields fail closed.
- `source: winget` validates only the fixed local source name. It does not prove
  that the registered source URL or repository is Microsoft-operated.
- Public DTOs contain no source path, raw YAML, native error text, executable
  path, installer path, environment value, or process output.

## Implementation

### Core configuration model

`apps/devbox-manager/src-tauri/src/core/dev_setup_configuration.rs` implements
bounded lexical preflight, typed normalization, the package desired-state
model, and deterministic manual rendering. Export and apply documents are
created from normalized data rather than serializing imported structures.

The canonical export contains deterministic `DevboxPackage01`,
`DevboxPackage02`, … names and only package ID, reviewed version/latest state,
`source: winget`, `matchOption: equals`, and `installMode: silent`. Imported
names, descriptions, dependencies, agreement flags, and security declarations
are shown when relevant but are not copied. Apply creates a separate canonical
one-package document with `acceptAgreements: true` only after confirmation.

### Native preview and guarded apply

`apps/devbox-manager/src-tauri/src/commands/dev_setup.rs` adds five commands:

- `import_dev_setup_configuration`
- `discard_dev_setup_configuration`
- `export_dev_setup_configuration`
- `apply_dev_setup_configuration`
- `cancel_dev_setup_apply`

Preview IDs and temporary names use 32 OS-CSPRNG bytes. Native preview storage
is limited to four entries with a five-minute TTL. Starting another import
revokes previous native previews before opening the picker; discard removes the
native token before renderer state is cleared; apply consumes the token before
the first process starts, so failed, cancelled, and rejected attempts cannot be
replayed.

Package observation invokes fixed direct arguments equivalent to:

```text
winget list --id <package-id> --exact --source winget --disable-interactivity
```

Unknown state always maps to `verify` and blocks apply. A process/source-level
probe failure marks the remaining packages unknown without repeating the same
failure. For changed packages, apply invokes:

```text
winget configure --file <guarded-canonical-file>
  --accept-configuration-agreements
  --disable-interactivity
  --suppress-initial-details
```

The temporary file is exclusively created under app cache, synced, and checked
for unexpected links. Preparation errors close and remove a newly created file;
normal guard drop closes the handle before best-effort deletion.

`commands/related_tools.rs` now provides the same guarded process boundary to
the existing Related Tools install and Dev Setup. On Windows it creates the
process suspended, assigns it to a Job Object with `KILL_ON_JOB_CLOSE`, then
resumes it. Timeout and cancellation terminate the tree and perform bounded
reaping; owner drop or crash relies on closing the Job Object as the fallback.
The shared single-flight guard prevents concurrent Manager mutations.

### Frontend review flow

`src/api.ts` and `src/types.ts` validate exact DTO keys, package/action
relationships, timestamps, digests, package ordering, apply result coherence,
and safe enum values. The renderer cache is cleared before native one-time
apply. Export content is reconstructed independently and, where Web Crypto is
available, SHA-256 is also recomputed before download.

`src/App.tsx` and `src/App.css` add the package review table, source-trust and
external-declaration warnings, canonical export, native discard, expiry timer,
three required acknowledgements, final confirmation, cancellation, and
per-package results. Labels distinguish complete, partial, and cancelled runs.
Browser mocks exercise UI flow only and are not presented as native evidence.

### Registration, dependencies, and documentation

- `src-tauri/src/lib.rs` registers native dialog support, bounded state, and the
  five commands; renderer capabilities intentionally do not add
  `dialog:allow-open`.
- Direct dependencies are `tauri-plugin-dialog`, `serde_yaml_ng 0.10.0`,
  `getrandom 0.4.3`, and `zeroize 1`. The lockfile adds the YAML parser graph.
- `docs/dependency-policy.md` records purpose, alternatives, exact lock
  provenance, licenses, limits, and residual risks.
- `THIRD_PARTY_NOTICES.md` was regenerated with the repository script and now
  includes `serde_yaml_ng`, `unsafe-libyaml`, and `ryu` plus the current
  `Cargo.lock` digest.
- The Manager README, architecture guide, and implementation specification
  document the user flow and trust boundary.

## Verification

The following checks passed in the dedicated worktree:

```text
cargo test -p devbox-manager                         PASS (142 tests)
cargo check -p devbox-manager                        PASS
cargo clippy -p devbox-manager --all-targets -- -D warnings
                                                       PASS
cargo fmt --all -- --check                           PASS
pnpm --filter devbox-manager test                    PASS (61 tests)
pnpm --filter devbox-manager build                   PASS
pnpm test                                             PASS
pnpm build                                            PASS
cargo check --workspace -j2                          PASS
cargo test --workspace -j2                           PASS on clean rerun
cargo deny --locked check                            PASS with existing warnings
pnpm audit --audit-level moderate                    PASS, no known vulnerabilities
dependency-policy / generated-notice check           PASS
build-manifest tests and catalog consistency check   PASS
git diff --check                                     PASS
```

The Manager production frontend bundle was 296.53 kB JavaScript (89.81 kB
gzip) and 19.27 kB CSS (3.93 kB gzip). Full workspace builds produced only the
repository’s known Vite chunk-size warnings.

The first full workspace Rust test run exposed an unrelated intermittent Repo
Manager Dependency Lens fixture failure: the stale-count assertion observed
`0` instead of `1` after a 5 ms filesystem timestamp delay. The exact isolated
test then passed, and a second full workspace run passed. This reliability work
is tracked separately in [#492](https://github.com/jihoon22-lee/devbox/issues/492#issuecomment-5465366872)
instead of being mixed into the Manager feature.

A Windows GNU cross-target attempt stopped in `aws-lc-sys` because the local
WSL environment has no `x86_64-w64-mingw32-gcc`. This is an environment
limitation and is not counted as Windows validation. GitHub Windows CI and the
physical Windows checklist must supply that evidence.

## Remaining acceptance

- Confirm the native picker and packaged resource permissions on Windows.
- Exercise WinGet/App Installer availability, local `winget` source behavior,
  UAC/reboot-risk messaging, success/failure/timeout/cancel cleanup, and the
  absence of path/process-output leakage.
- Record the packaged Manager artifact size at the release checkpoint; the
  dependency policy intentionally keeps this measurement pending until an
  actual package exists.
