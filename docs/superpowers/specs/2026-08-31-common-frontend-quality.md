# Common frontend quality contract

Date: 2026-08-31  
Issue: #491  
Targets: all 15 desktop frontends, `packages/a11y`, frontend CI

## Goal

All shipped Devbox frontends expose the same minimum keyboard, focus,
high-contrast, reduced-motion, language, and initial-bundle guarantees without
flattening each application's layout or making Devbox Launcher opaque. The
contract is local-only and introduces no telemetry, remote service, browser
fetch, or persisted user data.

## Shared boundary

`packages/a11y` owns only behavior that is identical in multiple applications:

- IME composition detection, including the WebView/Windows key-code fallback;
- Enter/Space activation recognition;
- bounded dialog focus enumeration, initial focus, Tab wrapping, Escape, and
  safe focus restoration;
- common `:focus-visible`, forced-colors, and reduced-motion CSS;
- a jsdom axe helper that reports structural violations while leaving color
  contrast to physical rendering acceptance.

Application-specific shortcuts, labels, modal actions, layouts, and domain
semantics remain in each app. The shared stylesheet sets no page background.
Devbox Launcher must retain transparent `:root` and `body` backgrounds.

## Language policy

Every application document declares `ko-KR`. User actions, state, guidance,
empty states, and recoverable errors use Korean. Stable technical identifiers
and values stay unchanged: product names, protocol names, commands, file
formats, Git refs, HTTP methods, API fields, and persisted enum values are not
translated. This avoids changing interchange contracts merely to localize the
display layer.

## Keyboard and focus contract

- An Enter shortcut does nothing while a Korean IME composition is active.
- A focusable row or card with pointer activation also supports Enter and
  Space, unless it implements a documented composite-widget keyboard model.
- A modal receives initial focus, keeps Tab/Shift+Tab inside, closes with Escape
  when safe, and restores focus to its connected opener.
- Focus is visible for keyboard users in normal and Windows forced-colors modes.
- Reduced-motion preference shortens non-essential animation and transition.

The first targeted adoption covers the gaps found in the v0.5.1 audit: WSL
pane creation, Life Log project entry, Everything+ root entry, Code Pad path
and global shortcuts, Repo Manager commit shortcut, clickable collection/rule/
profile/tab/card surfaces, and Code Pad/Knowledge Base/WSL dialog surfaces.

## Automated gate

Each release app must:

1. depend on `@devbox/tokens` and `@devbox/a11y` through the workspace;
2. import token CSS before accessibility CSS;
3. declare `lang="ko-KR"`;
4. emit a Vite manifest;
5. run at least one axe smoke against a rendered initial application shell.

The repository checker derives the complete app set from the release catalog
and fails closed if a frontend directory or any integration point is missing.
The package tests cover IME, activation, focus traversal/restoration, and axe
reporting. App tests cover the real rendered shell with that app's existing
native boundary mocks.

`axe-core` color contrast is disabled only under jsdom because it has no layout
or canvas. Focus visibility and high contrast are enforced structurally by the
shared stylesheet and remain a physical Windows acceptance item in #493.
The exact `axe-core 4.13.0` package is a test-only dependency: production apps
do not import the testing export, the package is absent from their initial
manifest graphs, and its reviewed MPL-2.0 boundary is recorded in the
dependency policy rather than shipped in runtime notices.

## Initial bundle budgets

Every release frontend emits `.vite/manifest.json`. CI resolves the initial
module and its static import graph, excluding dynamic imports, and checks raw
and deterministic gzip totals against a checked-in per-app budget. Baselines
are measured after the common accessibility dependency and localization land;
budgets use modest rounded headroom so ordinary hash variation passes while an
unreviewed eager dependency fails.

Missing output, missing/invalid manifest, path escape, duplicate entry,
unconfigured release app, or either size overrun fails the gate. Lazy chunks
remain visible in the report but do not count toward initial bytes.

## Acceptance

- all 15 app tests and builds pass offline from the frozen lockfile;
- the shared package and both repository checker test suites pass;
- all 15 generated manifests are present and all bundle budgets pass;
- Korean IME Enter, keyboard activation, modal wrap/Escape/restore, focus CSS,
  and Launcher transparency have automated regression coverage;
- physical Windows screen-reader/high-contrast behavior remains explicitly
  pending in #493 until exercised on the packaged artifacts.
