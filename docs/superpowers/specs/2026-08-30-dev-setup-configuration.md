# Devbox Manager Dev Setup: WinGet Configuration v3 package-only

**Date:** 2026-08-30
**App:** `apps/devbox-manager`
**Status:** implemented; local/required CI and v0.6.0 #493 hosted W02 package gate passed;
organization-specific WinGet policy/source combinations remain environment-dependent

## Purpose and scope

Dev Setup can review a user-selected WinGet Configuration v3 document and,
after explicit confirmation, apply its package requirements on Windows. The
import is a data import, not an execution-plan import: external YAML is never
executed and is never passed to WinGet. The Manager accepts a small package-only model, produces a
fresh canonical document, and invokes WinGet once per changed package.

The existing capability audit remains a separate schema-v1 read-only flow. It
does not install Docker, start a distro, edit PATH or the registry, or reboot
Windows. This configuration flow also does not support arbitrary DSC resources,
uninstall operations, fuzzy matching, or interactive installation.

## User flow

1. **Import.** `import_dev_setup_configuration` opens a Rust-native file picker
   for `.winget`, `.yaml`, and `.yml`. The renderer has no dialog-open
   permission. The selected file is opened and bounded natively, then parsed
   into `PackageRequirement` values.
2. **Review.** The backend probes each package with the guarded WinGet boundary
   and returns only package IDs, desired/current state, action, safe flags, a
   digest, and an opaque five-minute preview ID. `unknown` state becomes
   `verify`; it blocks apply and never becomes an install suggestion.
3. **Canonical export.** `export_dev_setup_configuration` returns
   `devbox-packages.winget`, generated from the validated model. It is not the
   imported file and contains no imported resource metadata or security
   declaration.
4. **Apply.** The UI requires three checklist confirmations—reviewed
   package-only contents, package agreements, and administrator/UAC plus
   reboot risk—and then a final confirmation window. The command requires all
   three boolean confirmations as well.
5. **Cancel.** While apply is running, `cancel_dev_setup_apply` sets a
   cooperative cancellation flag. The current guarded WinGet process is
   terminated and later packages are reported as `skipped`.

## Import contract

The contract is intentionally narrower than the full DSC/WinGet configuration
surface.

| Boundary | Current implementation |
| --- | --- |
| Input size | Non-empty UTF-8 text, at most 256 KiB and 4,096 lines; each line is at most 8 KiB and indentation is at most 32 spaces. |
| YAML preflight | Rejects NUL/other control characters, tabs, document markers, directives, aliases, anchors, tags, merge keys, and multiple documents. |
| Document | Exact `$schema` URL in `core/dev_setup_configuration.rs`; optional `metadata.winget.processor` is accepted only as `dscv3` (scalar or `{ identifier: dscv3 }`); `resources` must be non-empty. Unknown root/resource/property fields fail closed. |
| Resource count | At most 16 resources. |
| Resource type | Exact `Microsoft.WinGet/Package` only. Custom resources, including command or registry resources, are rejected. |
| Package properties | `id` is bounded and strictly shaped; `source` must equal `winget`; `matchOption` is absent or `equals`; `installMode` is absent, `default`, or `silent`; `_exist: false` is rejected. |
| Desired state | Exactly one of present (no `version`/`useLatest`), latest (`useLatest: true`), or a bounded version string. |
| Metadata | Only bounded description and `current`/`elevated` security-context declarations are parsed. Duplicate direct/nested security context is rejected. Declarations are shown in review but are not copied automatically. |

`source: winget` is a fixed source-name check, not a proof that the locally
registered source is official or points to an expected URL. The Manager does
not fetch or validate source registration metadata. This is an explicit
residual trust risk for Windows acceptance.

The parser uses `serde_yaml_ng` only after lexical preflight and deserializes
into `#[serde(deny_unknown_fields)]` structs. It never exposes a generic YAML
value or executes YAML tags. Package IDs and bounded resource names are both
de-duplicated case-insensitively.

The native command creates the imported byte buffer inside
`zeroize::Zeroizing` before the first read, then borrows its validated UTF-8
view for parsing. Partial reads and every later exit therefore clear that raw
buffer on drop. This reduces residual plaintext memory at the import boundary;
it is not a claim that every parser-owned `String` or temporary allocation is
wiped.

## Review and planning contract

The native command calls `run_guarded_winget` with direct arguments:

```text
list --id <package-id> --exact --source winget --disable-interactivity
```

The list probe has a 30-second timeout. Exit code `0` means `present`, the
exact WinGet no-application code means `absent`, and every other failure,
timeout, or unavailable executable means `unknown`. For a package desired as
`latest` and already present, a second `list` with `--upgrade-available`
distinguishes `update-available` from `present`. After one process/source-level
failure, the remaining packages are marked `unknown` immediately instead of
repeating the same bounded probe for each item.

The resulting action is one of:

| Observation | Action |
| --- | --- |
| absent | `install` |
| latest + update available | `update` |
| present + desired version | `reconcile-version` |
| present + present/latest desired | `none` |
| unknown | `verify` |

`canApply` is true only when there is at least one system-changing action and
no `verify` action. The frontend validates this relationship, the fixed
package-ID order, safe timestamps/digest, and exact DTO keys before rendering.

## Canonical output

`render_configuration` always creates a new document with:

- the fixed DSC schema and `dscv3` processor metadata;
- deterministic `DevboxPackage01`, `DevboxPackage02`, … resource names;
- only the validated package ID, desired version/latest state,
  `source: winget`, `matchOption: equals`, and `installMode: silent`;
- a fixed Manager description.

The review/export rendering passes `accept_agreements = false`. Imported
`acceptAgreements`, `securityContext`, descriptions, names, dependencies,
commands, paths, and other metadata do not cross the normalization boundary.
The apply path renders a separate one-resource document per changed package
with `acceptAgreements: true`, but only after the Manager’s own confirmation.

## Apply and process safety

The native command validates the preview ID and all three booleans, rejects a
second active apply, removes the preview from the in-memory map before running
the first package, and holds the shared Related Tools single-flight guard.
Each changed package is written to a new app-cache temporary `.winget` file
with exclusive creation, link checks, and sync. Preparation failures explicitly
close and remove a newly created file; the completed guard closes its handle
before best-effort removal on drop.

WinGet is invoked with:

```text
configure --file <guarded-temp-file>
  --accept-configuration-agreements
  --disable-interactivity
  --suppress-initial-details
```

Each package apply has a five-minute timeout. On Windows, the shared guarded
process boundary resolves a reviewed executable, creates the process
suspended, assigns it to a Job Object with `KILL_ON_JOB_CLOSE`, and resumes it
only after assignment. Completion waits for both the root exit code and zero
active processes in the Job Object. Timeout and cancellation explicitly
terminate the process tree and perform a bounded reap. Owner drop or crash uses
Job Object close as the process-tree kill fallback; it does not claim the same
explicit bounded reap. Process output, executable paths, installer paths, and
environment values are discarded.

Results contain only package ID plus fixed statuses: `unchanged`, `applied`,
`failed`, `timed-out`, `cancelled`, or `skipped`. A successful all-applied or
unchanged run is `complete`; timeout/failure is `partial`; cancellation with a
cancelled/skipped package is `cancelled`.

## Preview, renderer, and privacy boundary

- Preview state is bounded to four stored entries and expires after five
  minutes. Apply consumes the selected token before execution, so a failed or
  cancelled run cannot be replayed with the same token. Export can read the
  reviewed canonical content until expiry or apply consumption.
- Starting a new import clears older native previews before the picker opens,
  including a later picker cancellation or parse failure. The discard action
  removes its native token before clearing the renderer review state.
- The public review/export/apply DTOs contain no selected source path, raw YAML,
  WinGet output, installer location, or native error text. Frontend API
  wrappers replace native errors with fixed safe messages and validate exact
  shapes before React renders them.
- Browser mocks are bounded screen-flow fixtures. They do not prove native
  file reads, WinGet availability, Windows process ownership, or package
  installation.

## Implementation map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/core/dev_setup_configuration.rs` | Bounds, lexical YAML preflight, typed allowlist, package model, canonical renderer, pure tests. |
| `src-tauri/src/commands/dev_setup.rs` | Native picker/read, preview state, WinGet probes, apply/cancel commands, guarded temp files, safe DTOs. |
| `src-tauri/src/commands/related_tools.rs` | Shared Related Tools lock and Windows Job Object/direct-process WinGet boundary. |
| `src-tauri/src/lib.rs` | Registers state, native dialog plugin, and five Dev Setup configuration commands. |
| `src/api.ts` and `src/types.ts` | Browser/native invoke wrappers, mock fixtures, exact DTO/action validation. |
| `src/App.tsx` | Review table, export/discard controls, three checklist confirmations, final confirmation, cancel, and result display. |
| `src-tauri/capabilities/default.json` | Keeps dialog permission out of the renderer; picker access is native-only. |

## Acceptance and open questions

Pure Rust/frontend tests cover parser rejection, normalization, unknown-state
blocking, three-confirmation sequencing, cancellation UI, safe error handling,
and sanitized export. Manager and full-workspace Rust/frontend tests, check,
strict clippy, formatting, build, dependency-policy/notices, audit, manifest,
catalog, and `cargo deny` gates passed locally. One unrelated intermittent Repo
Manager stale-file fixture failure was isolated and recorded in #492; its exact
test and a second full workspace run passed.

The later v0.6.0 #493 hosted W02/package gate passed the Windows package flow. A real native picker,
WinGet/App Installer availability, locally registered `winget` source behavior, UAC/reboot-risk
messaging, package apply timeout/cancel cleanup and path/process redaction remain dependent on the
installed organization's environment; hosted evidence does not certify every policy/source
combination. Package installers may change PATH, registry, or files and may request a reboot; the
Manager never automates those changes or a reboot.

Reference documentation: [create a v3 configuration](https://learn.microsoft.com/en-us/windows/package-manager/configuration/create-v3),
[`winget configure`](https://learn.microsoft.com/en-us/windows/package-manager/winget/configure),
and [configuration check](https://learn.microsoft.com/en-us/windows/package-manager/configuration/check).
