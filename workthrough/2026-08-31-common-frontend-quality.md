# Common frontend quality, localization, and accessibility contract (W10 PR A)

**Date:** 2026-08-31  
**Issue:** [#491](https://github.com/jihoon22-lee/devbox/issues/491)  
**Branch:** `feat/workspace/frontend-quality`

## Overview

W10 PR A extends the frontend quality baseline to all 15 release applications. It
adds the shared `@devbox/a11y` package, applies the existing semantic tokens and
accessibility stylesheet consistently, localizes user-facing Korean text, and
adds fail-closed manifest/bundle and accessibility contract gates to CI.

The work is presentation- and verification-focused. Product names, protocol and
command names, API fields, file formats, Git refs, persisted enum values, and
native error categories remain stable. Known native messages are mapped to fixed
Korean display messages at the UI boundary; unknown messages are reduced to safe
generic errors rather than echoed. No telemetry, network service, new persisted
data, RC, tag, or release was created.

Physical Windows screen-reader, high-contrast, and packaged-rendering acceptance
remains explicitly tracked in [#493](https://github.com/jihoon22-lee/devbox/issues/493).

## Context and decisions

The v0.5.1 audit found repeated gaps across otherwise independent frontends:
pointer-activated rows were not uniformly usable from the keyboard, Enter could
submit while Korean IME composition was active, dialogs differed in focus
handling, and localization or bundle checks could silently omit a new release
app. A shared package owns only behavior that is identical across applications;
labels, shortcuts, layouts, and domain semantics remain app-owned.

The resulting minimum contract is:

- `ko-KR` on every application document and Korean user-facing actions, state,
  guidance, empty states, and recoverable errors;
- Enter/Space activation for pointer-activated rows/cards/tabs where the surface
  is not a composite widget;
- no Enter action during IME composition, including the WebView/Windows
  `keyCode === 229` fallback;
- dialog initial focus, Tab/Shift+Tab wrapping, safe Escape close, and focus
  restoration to a still-connected opener;
- visible `:focus-visible`, Windows `forced-colors`, and reduced-motion support;
- a Vite manifest and an axe smoke test for the rendered initial shell of every
  release app.

## Changes made

### 1. Shared accessibility package

Added the following package files:

- `packages/a11y/package.json`, `tsconfig.json`, `vitest.config.ts`, and `README.md`;
- `packages/a11y/src/index.ts` and `src/index.test.ts`;
- `packages/a11y/src/testing.ts` and `src/testing.test.ts`;
- `packages/a11y/styles.css`.

`@devbox/a11y` now provides:

- `isImeComposing`, which checks browser and React-native composition flags and
  the Windows/WebView 229 key-code fallback;
- `isKeyboardActivation`, which accepts Enter and Space only outside composition;
- bounded focusable-element enumeration that excludes hidden, disabled,
  `aria-hidden`, and inert descendants;
- `focusFirst`, `trapDialogKeyDown`, and `restoreFocus`, covering default focus,
  Tab wrapping, safe Escape handling, and connected-opener restoration;
- `@devbox/a11y/testing` with `findA11yViolations` and
  `assertNoA11yViolations`.

The shared stylesheet adds keyboard-visible focus rings, forced-colors focus
rules, and reduced-motion rules. It deliberately sets no page background, so
Devbox Launcher keeps its transparent `:root` and `body` window composition.

The axe helper disables only `color-contrast` under jsdom because jsdom has no
layout or canvas. Structural violations still fail the test; actual contrast,
screen-reader, and high-contrast rendering remain the physical Windows gate.

### 2. All 15 release frontends

Every release app now has the following integration points:

- `@devbox/tokens: workspace:*` and `@devbox/a11y: workspace:*` dependencies;
- `src/App.css` imports `@devbox/tokens/tokens.css` before
  `@devbox/a11y/styles.css`;
- `index.html` declares `lang="ko-KR"`;
- `vite.config.ts` enables `build.manifest: true`;
- an existing rendered-shell test calls
  `assertNoA11yViolations` with that app’s native boundary mocks. Devbox
  Launcher also keeps a browser search-alias regression test so the localized
  display text does not break the English alias users may already type.

The release set is derived from `apps/catalog.json` and consists of:

`api-playground`, `code-pad`, `devbox-launcher`, `devbox-manager`,
`developer-toolbox`, `everything-plus`, `knowledge-base`, `life-log`,
`log-lens`, `port-manager`, `repo-manager`, `run-manager`, `webhook-lab`,
`workbench`, and `wsl-desktop`.

The affected app roots received the common metadata/CSS/Vite/test changes in
`apps/<app>/{package.json,index.html,src/App.css,vite.config.ts}`. Their
app-specific `src/` changes translate user-facing copy and add regression
coverage without translating stable technical values.

### 3. IME-safe activation and focus behavior

The shared helper is used at the audited interaction points:

| Flow | Surface and behavior |
|---|---|
| Keyboard activation | API Playground collections; Devbox Manager app cards; Repo Manager repository cards; Run Manager service/job cards; Webhook Lab rule/history rows; Workbench profile rows; WSL Desktop tabs. Pointer activation remains unchanged, while Enter/Space activates only the row itself and never steals a nested control’s event. |
| IME-safe Enter | WSL Desktop pane creation; Life Log project entry; Everything+ root entry; Code Pad path/global shortcuts; Knowledge Base search/template actions; Repo Manager Ctrl/Cmd+Enter commit; API Playground and WSL Desktop command handlers. |
| Shared dialog focus | Code Pad close-document, encoding, LSP, and managed-installer dialogs; Knowledge Base rename flow; WSL Desktop Action Palette. Each focuses the first eligible control, wraps Tab, handles safe Escape, and restores the opener if connected. |
| Equivalent app-owned dialogs | Knowledge Base quick capture/template surfaces, Developer Toolbox handoff, Workbench wizard/template dialogs, and Launcher stale/preview dialogs retain their specialized transaction/busy behavior while preserving initial focus, trap, IME-safe Escape, and restore semantics. |

Representative shared activation contract:

```ts
export function isKeyboardActivation(event: CompositionAwareKeyboardEvent): boolean {
  return !isImeComposing(event) && (event.key === "Enter" || event.key === " ");
}
```

### 4. Korean UI and stable contract preservation

Localization was applied across all 15 apps, including labels, buttons,
tooltips, statuses, empty states, dialog copy, and recoverable errors. Technical
values remain as values: `Devbox Launcher`, `WSL`, `Docker`, `Git`, HTTP methods,
paths, protocol kinds, snapshot names, API keys/fields, and persisted IDs are not
rewritten for display.

The root regressions called out during review were fixed explicitly:

- **Devbox Manager** — the known native English
  `Related Tools는 Windows에서만 사용할 수 있습니다.` message maps to Korean
  display text; unknown native failures still use the generic safe error.
- **Developer Toolbox** — native text-handoff errors are mapped to the existing
  fixed Korean UI errors, with unknown failures reduced to
  `텍스트 전달을 처리하지 못했습니다` rather than exposing raw native text.
  The stable native Knowledge draft failure is likewise mapped to the Korean
  `Knowledge 초안` display contract without changing its handoff payload.
- **Code Pad and Run Manager** — raw native/OS/LSP/log failures no longer flow
  into localized banners. Known bounded categories map to actionable Korean
  text; unknown messages use fixed generic copy and do not echo paths or native
  details.
- **Repo Manager** — `routeOpenRequest` rejects empty, oversized, or NUL paths
  with fixed safe route text and never echoes an inbound path; the top-level
  error mapper allows only the small safe-message set.
- **WSL Desktop** — distro and Docker status keys are classified to Korean
  labels while the original Docker status remains available as a value/detail,
  preserving the native status contract.
- **Workbench and Devbox Launcher** — native-facing labels/errors such as
  `Workspace 시작`, `내가 시작한 작업 중지`, and `클립보드 미리보기` are localized;
  launcher IDs and `clipboard-preview/v1` behavior remain unchanged.

### 5. Fail-closed accessibility contract checker

Added:

- `.github/scripts/check-frontend-accessibility.mjs`;
- `.github/scripts/test-check-frontend-accessibility.mjs`.

The checker reads the release catalog, requires the frontend app directories to
match it exactly, and fails closed for missing or malformed integration points:
shared stylesheet features (`:focus-visible`, reduced motion, forced colors), a
page background in shared accessibility CSS, workspace dependencies, CSS import
order, `lang=ko-KR`, Vite manifest configuration, an invoked axe smoke import,
and the Launcher transparency exception. Commented-out markers do not satisfy
the contract. The fixture suite covers the passing and fail-closed paths.

CI runs the fixture suite in the dependency-policy job and runs the real
15-app checker in the catalog job. This keeps a newly added release app from
silently bypassing the shared quality contract.

### 6. Manifest-based initial bundle checker and budgets

`.github/scripts/check-frontend-bundles.mjs` now resolves each app’s
`dist/index.html` module entries through `.vite/manifest.json`, follows only the
static `imports` graph, and sums raw bytes plus deterministic gzip bytes
(`level: 9`, `mtime: 0`). Dynamic imports remain visible in the report as lazy
chunks but are excluded from the initial budget.

The checker fails closed for a missing `apps/catalog.json`, missing/invalid
config, missing output or index,
missing/invalid manifest records, duplicate entries/outputs, missing static
imports, non-canonical per-app output paths, path traversal or symlink escape,
release-catalog coverage mismatch,
selected-app coverage mismatch, and raw or gzip budget overrun. Fixture coverage
is in `.github/scripts/test-check-frontend-bundles.mjs`; the CI frontend job
runs the scoped checker after each frontend build.

The checked-in per-app initial budgets are:

| App | Raw initial budget | Gzip initial budget |
|---|---:|---:|
| API Playground | 685,000 bytes | 205,000 bytes |
| Code Pad | 1,225,000 bytes | 415,000 bytes |
| Devbox Launcher | 265,000 bytes | 85,000 bytes |
| Devbox Manager | 345,000 bytes | 105,000 bytes |
| Developer Toolbox | 610,000 bytes | 205,000 bytes |
| Everything+ | 270,000 bytes | 85,000 bytes |
| Knowledge Base | 970,000 bytes | 325,000 bytes |
| Life Log | 300,000 bytes | 95,000 bytes |
| Log Lens | 285,000 bytes | 90,000 bytes |
| Port Manager | 265,000 bytes | 85,000 bytes |
| Repo Manager | 320,000 bytes | 100,000 bytes |
| Run Manager | 385,000 bytes | 115,000 bytes |
| Webhook Lab | 415,000 bytes | 130,000 bytes |
| Workbench | 325,000 bytes | 100,000 bytes |
| WSL Desktop | 755,000 bytes | 220,000 bytes |

These baselines are intentionally rounded with modest headroom after the common
tokens/accessibility dependency and localization landed. The real 15-app
manifest checks passed against these per-app budgets.

### 7. Dependency, notices, and documentation updates

- `axe-core` is pinned exactly at **4.13.0**, is a dev-only dependency of
  `@devbox/a11y`, and is used only for jsdom structural regression tests. No
  production app imports it, it is absent from production initial manifest
  graphs, and it is not shipped in runtime notices or installers.
- The reviewed license is **MPL-2.0**. The exact package/source/integrity and
  test-only boundary are recorded as a package/version/integrity-bound approval
  in `docs/dependency-policy.md`; MPL-2.0 is not enabled as a repository-wide
  pnpm allowlist entry. The lockfile and `THIRD_PARTY_NOTICES.md` were
  regenerated. The package footprint recorded for review is 3,113,323 logical
  bytes / 3,174,400 allocated bytes.
- `.github/dependency-policy.json`, `pnpm-lock.yaml`, and
  `THIRD_PARTY_NOTICES.md` carry the policy/lock metadata; CI runs dependency
  policy, notice, catalog, and audit checks.
- `docs/superpowers/specs/2026-08-31-common-frontend-quality.md`,
  `docs/architecture.md`, `docs/development.md`, and `docs/roadmap.md` record
  the shared boundary and the physical-acceptance split.

## Verification results

Evidence captured for W10 PR A:

```text
pnpm build                         PASS — all frontend workspace builds
frontend test suite                PASS — 1,481 / 1,481 (Launcher: 14)
frontend typecheck                 PASS — all apps/packages
frontend accessibility fixtures    PASS
frontend accessibility checker     PASS — real 15-app catalog coverage
frontend bundle fixtures           PASS
frontend bundle checks             PASS — real 15-app manifest/budget checks
cargo test --workspace             PASS — earlier workspace evidence
cargo check --workspace            PASS
cargo fmt --all -- --check         PASS
devbox-launcher Rust tests         PASS — 29 / 29 after native string edits
catalog consistency                PASS
dependency policy                  PASS
third-party notice/lock check      PASS
pnpm audit                         PASS — no known vulnerabilities
```

The fixture suites exercised both passing and fail-closed paths, including
missing `apps/catalog.json`, missing config, missing manifest/output, static-import accounting,
lazy-chunk exclusion, path escape, duplicate entries, budget overruns, missing
dependencies, wrong language, CSS order, missing axe smoke, and forbidden shared
background styles.

## Pending physical acceptance

Windows packaged screen-reader and high-contrast behavior, actual contrast
rendering, keyboard-only observation across packaged artifacts, and Launcher
transparent-window rendering remain #493 acceptance work. This workthrough does
not claim those physical checks and does not create an RC, release tag, or
release artifact.
