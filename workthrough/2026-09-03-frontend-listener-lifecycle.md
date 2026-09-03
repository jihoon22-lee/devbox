# Frontend native listener lifecycle hardening

## Overview

This change gives every affected frontend effect explicit ownership of its asynchronous Tauri
listener registration. Cold AppLink requests are pulled only after the live wakeup listener is
ready, disposed effects do not consume one-shot requests, late registrations are immediately
released, and independently registered listeners cannot hide each other's successful cleanup.

The boundary spans Developer Toolbox, Log Lens, Run Manager, Devbox Manager, WSL Desktop,
Knowledge Base, and Code Pad because all seven defects are instances of the same renderer/native
event-delivery contract. It is frontend-only: native commands, Rust crates, dependencies, package
versions, catalog entries, and release assets are unchanged.

## Context

Tauri's `listen` API returns `Promise<UnlistenFn>`. A React effect can therefore be cleaned up
before registration resolves, particularly during the development-only StrictMode sequence of
setup, cleanup, and setup. Treating the returned promise as if registration were synchronous
created three related failure modes:

1. A late registration could publish its cleanup function after the effect had already tried to
   release listeners, leaving a duplicate native listener active.
2. An AppLink registration rejection belonging to a disposed effect could still call
   `takePendingOpen`. Because the native slot is one-shot, the replacement effect then saw no
   request and the user-visible handoff disappeared.
3. `Promise.all` around independent listener registrations discarded every result when one
   registration rejected. A sibling listener that had succeeded could no longer be released.

Log Lens also pulled the cold slot before the listener promise resolved. A hot relaunch arriving in
that registration window could remain pending without another wakeup. Devbox Manager, Run Manager,
and Developer Toolbox had variants of the disposed-effect cold-pull problem. WSL Desktop,
Knowledge Base, and Code Pad exposed the generic late or partial registration leaks.

## Changes made

### 1. Make cold and hot AppLink delivery share one live effect

Files:

- `apps/developer-toolbox/src/App.tsx`
- `apps/log-lens/src/App.tsx`
- `apps/run-manager/src/App.tsx`
- `apps/devbox-manager/src/App.tsx`

Each effect records disposal before doing any asynchronous work. It establishes the live listener,
then performs exactly one cold-start pull. Registration failure falls back to the cold pull only
while that effect is still live. A registration that succeeds after cleanup invokes its stop
function immediately and never takes the pending slot.

Every pending-result and error continuation also checks ownership before changing UI state. Live
events remain wakeup signals for the one-shot slot in Developer Toolbox, Log Lens, and Run Manager;
Devbox Manager preserves its existing validated install-event payload behavior while using the same
registration and cold-start lifetime rules.

### 2. Release late and partially successful native listeners

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/knowledge-base/src/App.tsx`
- `apps/code-pad/src/App.tsx`

WSL Desktop now owns the terminal-output and terminal-closed stop functions separately. A terminal
handler ignores stale events, and any registration that finishes after unmount stops itself.

Knowledge Base applies the same ownership rule to its docs-changed watcher. A stale callback cannot
reload tree and tag metadata after the effect has been disposed.

Code Pad no longer combines LSP diagnostics and status registration with `Promise.all`. Each
listener independently retains or releases its own stop function, so rejection on one channel
cannot leak the other channel.

### 3. Add deterministic lifecycle regressions

Files:

- `apps/developer-toolbox/src/App.applink.test.tsx`
- `apps/log-lens/src/App.handoff.test.tsx`
- `apps/run-manager/src/App.test.tsx`
- `apps/devbox-manager/src/App.test.tsx`
- `apps/wsl-desktop/src/App.applink.test.tsx`
- `apps/knowledge-base/src/App.test.tsx`
- `apps/code-pad/src/App.test.tsx`

Deferred promises model the otherwise timing-dependent boundary. The tests unmount before listener
registration settles, reject live registration, invoke a retained stale callback, and make one of
two LSP registrations fail. They assert both sides of the contract: no one-shot request or stale UI
work belongs to a disposed effect, while every successfully created listener is stopped once.

### 4. Record the shared contract

Files:

- `docs/architecture.md`
- the seven affected app `README.md` files
- `CHANGELOG.md`
- `workthrough/2026-09-03-frontend-listener-lifecycle.md`

The architecture guide now treats asynchronous listener registration as a resource-acquisition
boundary and documents listener-first AppLink delivery, independent ownership, late cleanup, and
StrictMode behavior. App documentation and the Unreleased changelog state the relevant user-facing
reliability guarantees.

## Code examples

### Listener-first cold AppLink pull

```tsx
let disposed = false;
let unlisten: (() => void) | undefined;
let coldStartConsumed = false;

const consumeColdStart = () => {
  if (disposed || coldStartConsumed) return;
  coldStartConsumed = true;
  consumePendingOpen();
};

void onOpenRequest(consumePendingOpen)
  .then((stop) => {
    if (disposed) stop();
    else {
      unlisten = stop;
      consumeColdStart();
    }
  })
  .catch(() => consumeColdStart());

return () => {
  disposed = true;
  unlisten?.();
};
```

### Independent listener ownership

```tsx
void listen("lsp/diagnostics", acceptDiagnostics)
  .then((stop) => {
    if (disposed) stop();
    else stopDiagnostics = stop;
  })
  .catch(() => undefined);

void listen("lsp/status", acceptStatus)
  .then((stop) => {
    if (disposed) stop();
    else stopStatus = stop;
  })
  .catch(() => undefined);
```

Unlike `Promise.all`, rejection of the status registration does not discard the diagnostics stop
function.

## Verification results

### Focused lifecycle regressions

```text
Developer Toolbox App.applink.test.tsx: 1 file / 12 tests passed
Log Lens App.handoff.test.tsx:           1 file / 7 tests passed
Run Manager App.test.tsx:                1 file / 20 tests passed
Devbox Manager App.test.tsx:             1 file / 54 tests passed
Knowledge Base App.test.tsx:             1 file / 15 tests passed
WSL Desktop App.applink.test.tsx:         1 file / 9 tests passed
Code Pad App.test.tsx:                    1 file / 31 tests passed
```

These regressions exercise listener success, rejection, partial failure, late resolution, stale
callbacks, and unmount cleanup at the mocked native boundary.

### Affected workspace verification

```text
pnpm verify:affected
frontend_scope=apps
frontend_packages=apps/code-pad,apps/devbox-manager,apps/developer-toolbox,
                  apps/knowledge-base,apps/log-lens,apps/run-manager,apps/wsl-desktop
rust_scope=none
dependency_scope=none

Code Pad:           15 files / 132 tests passed
Devbox Manager:      2 files /  72 tests passed
Developer Toolbox:  33 files / 261 tests passed
Knowledge Base:     13 files /  90 tests passed
Log Lens:            5 files /  34 tests passed
Run Manager:         8 files /  62 tests passed
WSL Desktop:        27 files / 232 tests passed

Total: 103 files / 883 tests passed
Frontend builds: 7 passed
Bundle budgets: 7 passed
Additional TypeScript packages: 0
Exit code: 0
```

The graph-aware verifier built and tested only the seven applications whose listener lifecycle was
changed. Rust, Windows packaging, dependency audits, and the other eight applications were not
started. The existing large lazy-chunk advisories remain informational; every measured initial
bundle is within its repository budget.

### Static repository contracts

```text
node .github/scripts/check-frontend-accessibility.mjs   PASS (15 static app contracts)
bash .github/scripts/check-catalog.sh                   PASS
git diff --check                                        PASS
```

A Windows packaged run is not required for this renderer lifecycle change. Native registration and
one-shot delivery are covered with deterministic mocks, while the native implementations and
permissions are unchanged.

## Next steps

- Require this PR's affected-scope GitHub Actions checks to pass before merge.
- Keep new frontend native listeners aligned with the documented independent-ownership pattern.
