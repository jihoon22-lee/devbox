# Windows hosted runtime acceptance for v0.6.0

## Overview

The exact-main v0.6.0 candidate built and installed correctly, but its all-app packaged runtime job
could not attach to WebView2 on the current elevated `windows-2025` image. WebView2 Runtime 150 and
newer intentionally ignore environment, command-line, and HKCU browser-argument overrides for a
high-integrity host. The same hosted desktop also reports a valid Tauri main window while refusing
external minimize/hide transitions. This change makes both host boundaries explicit without changing
any application capability or weakening local and self-hosted Windows acceptance.

## Context and root cause

- Candidate run `33352901854` built exact main
  `82e47c9c5af4cd0b50107a3161d1682cf269e856`, passed the strict 15 apps / 32 public files /
  31 manifest-declared assets / mismatch 0 contract, and passed all 15 installer lifecycles.
- All 15 portable runtime probes failed before renderer attachment because the WebView2 CDP endpoint
  never appeared. The runner image was `windows-2025-vs2026` `20260824.214.3` with WebView2 Runtime
  `151.0.4129.101` already installed; installing the candidate did not change that runtime.
- Tauri/wry tracks the elevated Runtime 150 behavior in
  [wry #1782](https://github.com/tauri-apps/wry/issues/1782). Microsoft documents HKLM policy or
  API-level `AdditionalBrowserArguments` as the supported elevated-host paths in
  [WebView2Feedback #5645](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5645).
- An image-scoped HKLM policy restored CDP, renderer, and IPC immediately. Seven external window
  techniques, including `ShowWindow`, `SetWindowPos`, system messages, and UI Automation, could not
  minimize or hide the hosted main window. The same exact candidate had already passed a physical
  Windows Port Manager minimize-to-second-launch restore check.

## Changes made

### Transactional elevated WebView2 CDP policy

File: `.github/scripts/windows-packaged-smoke.mjs`

- Detect whether the Windows acceptance process is elevated.
- Keep the process-local `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` path for ordinary Windows runs.
- For an elevated run, inspect the per-image HKLM WebView2 policy before mutation and fail on any
  existing value instead of overwriting it.
- Mark mutation ownership before the single non-retried PowerShell operation, then exact-read the
  installed value. An ambiguous timeout therefore still reaches cleanup.
- In `finally`, remove only the exact value if ownership is unchanged, remove a newly created key only
  when it remains empty, verify that the owned value is absent, and block release on any cleanup
  failure.
- Record `host.elevated`, the selected CDP override, policy restoration, the last bounded HTTP
  observation, and listener owners in evidence.

### Hosted single-instance contract

Files:

- `.github/scripts/windows-packaged-smoke.mjs`
- `.github/workflows/windows-package-candidate.yml`
- `.github/scripts/test-windows-package-candidate-config.py`

The normal Windows and self-hosted contract is unchanged: minimize or begin hidden, launch the same
binary again, and require the original owned window to become visible and non-minimized. The hosted
variant is enabled only when all of these are true:

```text
GITHUB_ACTIONS=true
RUNNER_ENVIRONMENT=github-hosted
RUNNER_OS=Windows
acceptance process is elevated
```

If that host refuses the external minimize request while the owned primary remains visible and
non-minimized, the gate still requires the second process to exit successfully, the first process to
remain healthy, exactly one executable image to remain, the renderer to remain focused and error-free,
and both processes to emit zero output. Apps configured to begin hidden still require a direct show
transition after the second launch. The workflow independently requires exactly 15 accounted window
contracts and, for an elevated run, 15 restored policy contracts.

This layered result is not a second physical-test exclusion. The exact candidate's physical Windows
minimize/restore evidence and the 15-app common `show`/`unminimize`/`set_focus` source contract remain
authoritative for the native transition; the hosted runner remains a blocking all-app process,
renderer, IPC, and cleanup gate. Only the user-approved installed WSL Desktop zellij/terminal
reconnection check remains deferred.

## Verification results

### Local static contracts

```text
node --check windows-packaged-smoke.mjs                         PASS
node windows-packaged-smoke.mjs --self-test                    PASS
test-windows-package-candidate-config.py                       PASS
test-windows-packaged-smoke-config.py                          PASS
test-windows-installer-acceptance-config.py                    PASS
Ruby YAML parse                                                PASS
check-catalog.sh                                               PASS
git diff --check                                               PASS
```

### Fresh hosted diagnostic

Run `33369379014` reused the exact candidate artifact and tested Port Manager both before and after
candidate installer provisioning:

```text
WebView2 Runtime                    151.0.4129.101 before and after
packaged status                     PASS / PASS
CDP override                        hklm-policy
renderer and native IPC             PASS
second-instance process contract    PASS
window contract                     hosted-visible-primary
stdout / stderr                     0 bytes / 0 bytes
remaining image/descendant count    0 / 0
app-data transaction residue        0
CDP policy restored                 true / true
runtime root removed                true / true
```

## Exact-main candidate follow-up

Candidate run `33371506798` rebuilt exact main
`fc7a49cd4df828f6917990019d0e1dc8d4f7618b`. Its unpublished artifact
`9752181424` (`sha256:fcb309cbd05d9bc26a2be997caf904d2bb50a0111b08f7bbc4113897c427f4b0`)
independently verified all 15 apps, 32 downloaded assets, and 31 manifest-declared assets with zero
missing, undeclared, digest, or size mismatches. Installer evidence `9752273014`
(`sha256:a4147e13b21af13c69e449bf701bbaded74a60e6c03ffa7af61275527af0a5b2`)
passed all six install/update/uninstall phases for all 15 apps with zero registry, install-directory,
marker, app-data, or integration residue.

The packaged runtime evidence `9752424801`
(`sha256:7cf5e36859e6ef753049454770bfb3dc347db52992f7e1ca24e20d67be453fa7`)
failed closed at 13/15 while preserving complete cleanup and policy restoration for every attempted
app. Both failures were isolated after the common renderer, native IPC, ten-second process, and
cleanup checks:

- Devbox Launcher intentionally starts with its Tauri window hidden. Windows
  `Process.MainWindowHandle` selected the visible internal WebView2
  `com.devbox.devboxlauncher-siw` window instead of the hidden titled `Devbox Launcher` window, so
  the harness rejected a healthy process before exercising the second-launch restoration contract.
- WSL Desktop correctly returned the bounded unavailable-runtime result on a hosted machine without
  WSL, but its opportunistic background snapshot writer printed the expected error to GUI stderr.
  The exact 88-byte output had SHA-256
  `2d6c922ac70b50920d5fa3924488201ad5b7476894c5cb2d4179b0a105dff93b`.

### Follow-up corrections

- The Windows helper now enumerates top-level windows owned by the packaged PID and selects only the
  window whose title exactly matches the app contract. First-instance health, minimize/hide, and
  second-instance restoration therefore inspect the Tauri app window instead of an implementation
  detail chosen by `Process.MainWindowHandle`.
- A pure first-instance contract requires a responding live process plus an exact titled native
  window. Visible apps must be visible and non-minimized; an app may begin hidden only when its
  packaged config explicitly allows that state and the exact titled window is actually hidden.
  Self-tests cover visible and hidden success plus wrong-title, unapproved-hidden, minimized,
  unresponsive, and exited failures.
- WSL Desktop's periodic snapshot writer now treats collection as opportunistic: it preserves the
  last-good snapshot and retries without writing expected environment failures to stderr. Explicit
  dashboard refresh still returns the same bounded user-safe error through IPC.

### Exact-artifact Launcher diagnostic

Run `33380179211` reused the exact failed candidate instead of rebuilding it and applied only the
revised acceptance helper. Artifact `9753458118`
(`sha256:6c1d6418fdfe158205ed415afbb4877dbc5a0840e22fb93452ef914f95bf310c`)
recorded:

```text
packaged status                     PASS
first process                       responding for ten seconds
exact titled native window          present, hidden, non-minimized
first displacement mode             initially-hidden
second process                      exit 0
primary after second launch         visible, non-minimized, exact title
window contract                     direct-restoration
remaining executable images         0 after cleanup
stdout / stderr                     0 bytes / 0 bytes
CDP policy / app data restored      true / true
runtime root removed                true
```

### Follow-up local verification

```text
node --check windows-packaged-smoke.mjs                         PASS
node windows-packaged-smoke.mjs --self-test                    PASS
test-windows-package-candidate-config.py                       PASS
test-windows-packaged-smoke-config.py                          PASS
test-windows-installer-acceptance-config.py                    PASS
check-catalog.sh                                               PASS
cargo fmt --check (wsl-desktop)                                PASS
cargo test -p wsl-desktop --lib -j1                            PASS — 102 / 102
cargo check -p wsl-desktop --all-targets -j1                   PASS
cargo clippy -p wsl-desktop --all-targets -j1 -- -D warnings   PASS
pnpm -r --workspace-concurrency=1 build                         PASS — 22 / 22
git diff --check                                               PASS
```

## Next steps

- Merge this follow-up only after all six required source CI jobs pass.
- Rebuild one unpublished candidate from the new exact main commit; do not run it for ordinary
  intermediate merges.
- Require all 15 packaged runtime contracts and all 15 installer lifecycles to pass from fresh
  `windows-2025` runners before creating the annotated `v0.6.0` tag.
- Publish once, run the fresh public-download verifier, update the release ledger, and remove all
  dedicated worktrees and local/remote branches in the repository-required order.
