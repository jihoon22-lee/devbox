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

## Next steps

- Merge only after all six required source CI jobs pass.
- Rebuild an unpublished candidate from the new exact main commit.
- Require all 15 packaged runtime contracts and all 15 installer lifecycles to pass from fresh
  `windows-2025` runners before creating the annotated `v0.6.0` tag.
- Publish once, run the fresh public-download verifier, update the release ledger, and remove all
  dedicated worktrees and local/remote branches in the repository-required order.
