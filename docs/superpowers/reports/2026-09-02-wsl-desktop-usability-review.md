# WSL Desktop usability reinforcement review

Date: 2026-09-02
Target: `apps/wsl-desktop` at `e7862de` (v0.6.0 stable source line)
Scope: perceived usability of the shipped terminal — what a daily WSL user meets
between opening the app and closing it. Read-only review. No shipped behavior,
dependency, version, catalog or release artifact changes accompany it.

## Method and baseline

Source read in full: `src/App.tsx`, `src/components/*`, `src/lib/*`,
`src-tauri/src/commands/terminal.rs`, `src-tauri/src/commands/multiplexer.rs`,
`src/App.css`, plus the owning design spec
[`2026-08-17-wsl-desktop-terminal-design.md`](../specs/2026-08-17-wsl-desktop-terminal-design.md).

```text
pnpm install --frozen-lockfile      PASS
pnpm --filter wsl-desktop test      PASS (18 files, 138 tests)
```

One finding (F1) was reproduced with a throwaway Vitest probe against the real
`App` component; the probe is quoted in "Reproducing F1" below and was removed
after the run. Every other finding is a direct source reading with a file/line
citation. Nothing here was verified on Windows: this review distinguishes
"source-confirmed" from "needs a Windows observation" and never claims runtime
proof it does not have.

## Boundaries this review does not propose relaxing

Several rough edges below exist because a safety rule produced them. The
recommendations keep every one of these:

- No shell string assembly. `wsl.exe` argv stays exact, `--cd` stays a separate
  argument, and no user text becomes a shell fragment.
- The multiplexer stays opt-in, detected read-only, never installed, and never
  leaks a resolved absolute executable path to the renderer, logs or profiles.
- Broadcast keeps its explicit target selection, the two-target minimum, the
  32-target bound, the multiline and dangerous-command confirmations, and the
  backend's own target validation.
- Log Lens handoff keeps its one-time envelope, TTL, and the rule that a
  `wslPath` never reaches localStorage, a profile, AppLink argv or the clipboard.
- Profile store validation stays fail-closed; a corrupt store is never
  overwritten with an empty one.
- No new network dependency, no runtime download, no telemetry.

## Findings

Priority is frequency-weighted: P1 is met in ordinary daily use, P2 is what the
app still lacks against a mainstream terminal, P3 is polish.

### P1-1 (F1). A routine snapshot refresh silently switches broadcast off and disables Docker actions

`refreshDashboard` sets the shared state to `refreshing` on entry
(`src/App.tsx:285`). `broadcastReady` requires `dashboardState === "fresh"`
(`src/App.tsx:1251`) and an effect force-clears the toggle whenever that goes
false (`src/App.tsx:1257`). A refresh runs on a `min(60s, max(5s, staleAfterMs))`
interval — 30 s with the shipped TTL (`src/App.tsx:377`) — and additionally on
every terminal start, every pane/tab close and every Docker action. So broadcast,
which the user had to arm deliberately (pick ≥2 panes, then tick the toggle),
turns itself off at least twice a minute and does not come back when the refresh
succeeds. The same gate disables the Docker start/stop/restart buttons
(`src/components/DistroPanel.tsx:51,213,223,231`), so those flicker dead for the
length of each collection.

Why the cost is not buying safety: the snapshot carries per-distro counts only —
`terminal_counts_by_distro` is explicitly built "without exposing session ids,
pane keys, cwd, title or command metadata"
(`src-tauri/src/commands/terminal.rs:29-52`). Target identity comes from
`panes`/`tabs`, which are driven by `terminal-closed` events, and the backend
independently rejects any broadcast naming a session it does not hold
(`src-tauri/src/commands/terminal.rs:515-524`). Gating live broadcast on "a
collection is currently in flight" therefore removes usability without adding
target-identity safety.

Recommendation: keep fail-closed on `error` and `stale` and on any change to the
target set; treat `refreshing` as ready while the last-good snapshot is still
inside its TTL. Same change for the Docker action gate. Everything in
"Boundaries" above stays.

### P1-2. Confirmation fatigue: 15 native `window.confirm`/`window.prompt` sites

`rg` counts 15 non-test call sites in this app — the highest in the monorepo
(next is webhook-lab at 10; seven apps have none). In a terminal, the ones that
recur are close-a-pane (`src/App.tsx:979`), close-a-tab (`:985`), open-a-link
(`src/components/TermPane.tsx:254`) and multiline paste
(`src/components/TermPane.tsx:208`). Closing a single pane always asks, from four
separate entry points (✕ button, `Ctrl+Shift+W`, context menu, palette). Native
dialogs also break theme, focus restoration and IME behavior, and cannot be
dismissed with the app's own `Esc` conventions.

Recommendation, in order of value: (a) replace the native dialogs with an in-app
confirm/prompt that restores focus to the exact pane, matching the existing
`ContextMenu`/`ActionPalette` accessibility approach; (b) make single-pane close
confirmation a persisted setting (default on) while multi-pane, tab and
close-others confirmations stay unconditional; (c) allow "don't ask again for
this host" for link opening, scoped to the session and never persisted. Keep the
start-command, profile-switch, profile-delete, Log Lens handoff, multiline-paste
and dangerous-command confirmations exactly as they are — those gate execution
or a cross-app handoff.

### P1-3. First run, and every run after closing the last pane, is an empty app

Restore runs once distros hydrate; with no saved layout it just marks the
workspace ready (`src/App.tsx:877-884`). `workspaceFromRuntime` returns `null`
with zero panes and `saveLastWorkspace(null)` removes the key
(`src/lib/workspace.ts:194,229-234`), so closing the last pane also clears the
layout. The app whose sole purpose is a terminal opens with no terminal and an
empty-state card.

Recommendation: when hydration succeeded, no layout was restored, and no applink
target is pending, open one terminal in the default distro. Make it a persisted
setting (default on) and skip it when the snapshot came back in `error` — this
must not start a shell on a failed hydration.

### P1-4. The tab bar never scrolls the active tab into view

`.tab-bar` is `overflow-x: auto` (`src/App.css:183-192`) and `TabBar` holds no
element ref and calls no `scrollIntoView`. `activateTab`/`stepTab`/`gotoTab`
(`src/App.tsx:998,1006,1014`) change state only. With enough tabs, `Ctrl+Tab` and
`Ctrl+Alt+N` move to a tab that stays off-screen, so the visible selection looks
wrong. The bar also has no middle-click close and no double-click rename — rename
exists only in the context menu (`src/lib/contextMenu.ts:64`).

Recommendation: scroll the active tab into view on every activation; add
middle-click close and double-click rename through the existing normalize/confirm
paths.

### P1-5. Broadcast targets are invisible in the terminal grid

When broadcast is on, the only indication is the toolbar badge
(`src/App.tsx:1434-1441`). `TermPane` renders `pane-focused` and nothing else
(`src/components/TermPane.tsx:533`), so at the place the user is actually typing
there is no marker distinguishing the up-to-32 panes that will receive the
keystroke from the ones that will not.

Recommendation: a per-pane target marker (border/badge plus an accessible name
suffix) whenever broadcast is armed. This reinforces the safety story rather than
relaxing it.

### P1-6. Session-keeping mode and side-panel state are not remembered

`multiplexer` initializes to `"native"` every launch (`src/App.tsx:103`) and
`panelOpen` to `true` (`src/App.tsx:91`); neither has a key in
`src/lib/storage.ts`. A tmux/zellij user re-selects the mode on every start, and
anyone who prefers the panel closed re-closes it on every start.

Recommendation: persist both. Re-validate the stored multiplexer against the
current detection result for the selected distro and fall back to `native`
silently, which is what the existing detection effect already does
(`src/App.tsx:240-262`).

### P1-7. A pane never says which mode it got, or that it reconnected

`StartedSession.resumed` is consumed only to suppress the start command
(`src/App.tsx:666,739`) and `Pane.multiplexer` is stored but rendered nowhere
(no reference in `src/components/*.tsx`). When the backend downgrades a requested
tmux/zellij launch to native, or when it re-attaches an existing `wsld-*`
session, the pane looks identical either way.

Recommendation: a pane-head badge for the effective mode and a one-shot
"reconnected" marker. This is the smallest change that makes the open
[#518](https://github.com/jihoon22-lee/devbox/issues/518) zellij/terminal
reconnect observation checkable by looking at the app instead of inferring it.

### P2-1. No drag pane resize, and no pane zoom

Layout is three presets — `grid`/`cols`/`rows` with equal fractions computed in
`PaneCanvas` (`src/components/PaneCanvas.tsx:78-92`). There is no splitter and no
maximize toggle (`rg` for `maximize|zoom|splitter|resizer` returns nothing in
`src/`). Drag resize is named in the design spec's usability table
(spec §4.3) and was explicitly excluded from #262's acceptance, so this is a
known deferral, not an oversight. In practice a build log pane and an editor pane
cannot be given different weights.

Recommendation: treat drag resize plus a "zoom active pane" toggle as one work
item. Both must go through the existing ack-after-commit resize path and respect
the `MIN_ROWS`/`MIN_COLS` floors (`src/components/TermPane.tsx:105-106`) and the
`.panes` CSS floor (`src/App.css:265-278`) that exist to stop pane collapse from
destroying shell output.

### P2-2. The DOM renderer is still in use

Loaded addons are fit, search, unicode11 and web-links
(`src/components/TermPane.tsx:325-334`; `package.json` has no
`@xterm/addon-webgl` and the lockfile has no webgl/canvas addon). The spec's
usability table names `@xterm/addon-webgl` with a canvas/DOM fallback (§4.3);
it too was excluded from #262. Heavy output and full-screen TUIs therefore run on
the slowest renderer xterm ships.

Recommendation: adopt the WebGL addon with an explicit fallback on context loss.
The current CSP needs no change (`src-tauri/tauri.conf.json:21`). This one must
be measured on Windows before and after — no throughput claim should be made from
a Linux dev box.

### P2-3. `Alt+Arrow` cycles a list instead of moving spatially

`focusPane` walks `tab.paneIds` modulo its length (`src/App.tsx:1055-1062`) and
`matchShortcut` maps Right/Down to `+1` and Left/Up to `-1`
(`src/lib/shortcuts.ts:21-24`). In a 2×2 grid, `Alt+Right` from the top-left pane
moves to the top-right, but `Alt+Down` moves there too, and from the top-right
`Alt+Right` wraps to the bottom-left. Windows Terminal, which these bindings
imitate, moves by geometry.

Recommendation: resolve direction against the rendered grid position (the same
`order`/`gridTemplate` math `PaneCanvas` already computes) and stop wrapping.

### P2-4. Scrollback search has no options and no whole-buffer highlight

`runSearch` passes only `decorations` and `incremental`
(`src/components/TermPane.tsx:517-529`). The addon supports case sensitivity,
whole word, regex and `decorations`-driven highlight-all; none is exposed, and
the panel offers only prev/next/close (`:557-585`).

Recommendation: add case/word/regex toggles and match-all highlighting, keeping
the 512-character query bound (`src/lib/terminalUx.ts:5`).

### P2-5. No clear-scrollback, scroll-to-bottom or select-all

None of the three exists in the palette, the context menu or the shortcut table.
After a large build, the only way to get a clean screen is the shell's own
`clear`, which does not drop the 10,000-line buffer.

Recommendation: add all three as palette entries and context-menu items on the
exact right-clicked pane, matching the existing capability-gated pattern in
`src/lib/contextMenu.ts`.

### P2-6. The command palette is thin, and its actions carry no shortcut hints

The palette holds five fixed actions plus one entry per profile
(`src/App.tsx:1295-1337`). Missing: new tab, close tab, rename tab, layout
switch, font size, toggle side panel, refresh snapshot, open a terminal in a
named distro, save current profile. `PaletteAction` has no shortcut field
(`src/components/ActionPalette.tsx:4-10`), so the palette cannot teach the
bindings it duplicates, and matching is a plain substring test (`:23-28`).
The spec's `{{param}}` snippet model with quote-by-default substitution and a
pre-execution preview (§4.4) is unimplemented.

Recommendation: complete the action set and render a shortcut hint column first;
treat the snippet model as a separate, larger item because it carries its own
quoting and preview rules.

### P2-7. There is no shortcut reference anywhere in the app

`matchShortcut` and `matchTerminalKey` define fifteen key families between them
(`src/lib/shortcuts.ts`, `src/lib/terminalUx.ts:27-47`). Five reach the user, as
`title` tooltips on the new-tab button, the palette button and the three font
controls (`src/components/TabBar.tsx:87`; `src/App.tsx:1415,1458,1464,1470`). The
rest — `Ctrl+Shift+D`, `Ctrl+Shift+W`, `Ctrl+Shift+F`, `Ctrl+Shift+C/V`,
`Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+Alt+1..9`, `Alt+Arrow`, and the
`Shift+F10`/Menu context-menu key — are undiscoverable without reading the
README.

Recommendation: one keyboard-shortcut panel, generated from the same tables the
matchers use so it cannot drift.

### P3. Polish

| # | Finding | Evidence |
|---|---|---|
| P3-1 | The toolbar holds ~18 interactive controls in one wrapping row; a narrow window pushes it to 2–3 lines and takes that height from the panes. The CSS comment already records this as the entry point to pane collapse. | `src/App.tsx:1341-1479`; `src/App.css:38-48` |
| P3-2 | Distro selection is duplicated three times: toolbar `select`, side-panel `select`, side-panel cards. | `src/App.tsx:1350-1361`; `src/components/DistroPanel.tsx:82-93,104-109` |
| P3-3 | The error banner has no dismiss control and clears only as a side effect of the next action; it also carries both `role="alert"` and `aria-live="assertive"`. | `src/App.tsx:1537` |
| P3-4 | A new tab does not inherit the active pane's cwd; a split does. | `src/App.tsx:900` vs `src/App.tsx:1162-1179` |
| P3-5 | Font family, cursor style, scrollback size (10,000) and the color theme are hard-coded; only font size is user-controlled. | `src/components/TermPane.tsx:82-87,303-308` |
| P3-6 | Recent paths are capped at 5 with a single pin slot. | `src/lib/storage.ts:10,38-43` |
| P3-7 | Tabs are `div`s with `tabIndex=0` and `aria-current`, without `role="tablist"`/`role="tab"` or arrow-key traversal inside the bar. The existing suite's a11y assertions pass, so this is a semantics improvement, not a reported violation. | `src/components/TabBar.tsx:26-32` |
| P3-8 | Terminal bell is unhandled — no visual or audible signal on `\a`. | no `onBell` in `src/components/TermPane.tsx` |

## Suggested packaging

Four cohesive candidates, each one user-visible boundary, ordered by
value-per-risk. None is a commitment: the roadmap has no open v0.7 milestone, so
these are candidates until the user prioritizes them.

1. **Snapshot gating and confirmation load** (F1, P1-2). Frontend-only, highest
   daily impact, no new dependency. Regression tests: broadcast survives an
   in-flight refresh, still fails closed on `error`/`stale`/target change; Docker
   actions stay enabled on a last-good snapshot inside TTL; the in-app dialog
   restores focus to the exact pane.
2. **Session continuity and startup** (P1-3, P1-6, P1-7). Persisted mode/panel
   state, first-run terminal, effective-mode and reconnect badges. Also the
   cheapest way to make #518's reconnect observation self-evident.
3. **Navigation and orientation** (P1-4, P1-5, P2-3, P2-7). Tab scroll-into-view,
   middle-click/double-click, broadcast target markers, spatial pane focus, the
   shortcut reference.
4. **Terminal surface depth** (P2-1, P2-2, P2-4, P2-5, P2-6). Drag resize and
   zoom, WebGL renderer with fallback, search options, buffer commands, palette
   completion. Largest and the only one needing a Windows performance
   measurement; the snippet model stays out of it.

## Reproducing F1

Save as `apps/wsl-desktop/src/App.probe.test.tsx`, reusing the `./api` and
`./components/TermPane` mocks from `App.contextMenu.test.tsx`, then run
`pnpm --filter wsl-desktop exec vitest run src/App.probe.test.tsx`. It passed on
`e7862de`, which is the failure being reported.

```tsx
// after arming broadcast on two panes and asserting toggle.checked === true:
let release: (() => void) | undefined;
snapshotMock.mockImplementationOnce(
  () => new Promise((resolve) => { release = () => resolve(snapshot()); }),
);
fireEvent.click(screen.getByRole("button", { name: "새로고침" }));

await waitFor(() => expect(toggle.checked).toBe(false));
release?.();
await waitFor(() => expect(toggle).toBeEnabled());
expect(toggle.checked).toBe(false); // stays off until re-armed by hand
```

## What this review did not cover

- No Windows or packaged-runtime observation; no ConPTY, WebView2 or installed
  binary behavior was exercised.
- No rendering throughput measurement (P2-2 needs one before and after).
- No change to the multiplexer detection contract, the Log Lens handoff
  lifecycle, the integration snapshot schema, or the profile store format.
- Docker engine management, distro installation and anything the app already
  declares out of scope stay out of scope.
