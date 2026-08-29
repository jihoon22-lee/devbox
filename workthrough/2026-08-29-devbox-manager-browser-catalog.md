# Devbox Manager browser catalog sync

## Overview

Updated the Devbox Manager browser fallback to match the current stable `v0.5.0`
release catalog. The browser screen now has the same 14 managed targets as the
native Manager flow: it excludes Devbox Manager itself while including Devbox
Launcher and Log Lens, and uses the release manifest's exact target versions and
portable/installer asset metadata.

Tracking issue: #475.

## Context

The previous `MOCK_MANIFEST` still described `v0.4.0-rc3`, used pre-release app
versions and placeholder asset metadata, included the Manager app, and omitted
Log Lens. The repository catalog remains a 15-app catalog; the Manager UI filters
that catalog to manager-visible, non-self-managed apps. Historical two-app
fixtures in `App.test.tsx` remain unchanged because they exercise update and
action states with intentionally older versions.

## Changes Made

### Stable browser release manifest

- File: `apps/devbox-manager/src/api.ts`
- Changed the browser fallback release tag to `v0.5.0` and synchronized its
  generated timestamp with the published stable manifest.
- Replaced old placeholder entries with the 14 managed app entries and exact
  `v0.5.0` target versions, portable names, installer names, sizes, and SHA-256
  digests.
- Removed `devbox-manager` from the browser manifest and added `log-lens`.

### Frontend regression coverage

- File: `apps/devbox-manager/src/api.test.ts`
- Added a browser fallback test covering the 15-app source catalog, 14-app
  managed manifest, self exclusion, Launcher/Log Lens presence, stable release
  tag/timestamp, target versions, and asset naming contract.
- File: `apps/devbox-manager/src/App.test.tsx`
- Added a stable-catalog UI test covering the 14 rendered rows, absence of a
  Manager row, Launcher/Log Lens visibility, representative target versions,
  and the `Latest: v0.5.0` label.
- Preserved the existing historical two-app manifest fixture used by action
  state tests.

### Documentation

- File: `apps/devbox-manager/README.md`
- Documented that browser development mode uses the stable manifest metadata and
  displays 14 managed targets from the 15-app catalog.

## Code Examples

```typescript
// apps/devbox-manager/src/api.ts
const MOCK_MANIFEST: ReleaseManifest = {
  schemaVersion: 1,
  releaseTag: "v0.5.0",
  generatedAt: "2026-08-28T23:45:52Z",
  apps: [
    // 14 manager-visible, non-self-managed stable targets,
    // including devbox-launcher and log-lens.
  ],
};
```

```typescript
// apps/devbox-manager/src/App.test.tsx
expect(screen.getAllByRole("row")).toHaveLength(15); // header + 14 apps
expect(screen.queryByRole("row", { name: /Devbox Manager/ })).toBeNull();
expect(screen.getByText("Latest: v0.5.0")).toBeTruthy();
```

## Verification Results

### Focused frontend tests

```text
pnpm test  (apps/devbox-manager)
Test Files  2 passed (2)
Tests       39 passed (39)
Exit code: 0
```

### Frontend build

```text
pnpm build  (apps/devbox-manager)
tsc && vite build: successful
42 modules transformed
Exit code: 0
```

`git diff --check` also passed. No commit, push, PR, or GitHub mutation was
performed; this worktree is intentionally left dirty for review.
