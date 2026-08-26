# Everything+ Result Context Menu Race Workthrough

- Date: 2026-08-26
- Branch: `feat/everything-plus/context-menu-race`
- Base: `643a529c31823b32c5d5a5614cf30180b35f79eb` (`main`)
- Trigger: frontend CI on PR #415 intermittently failed the existing Shift+F10 result-menu fixture
- Status: implementation and focused local verification complete; CI pending

## Outcome

Everything+ still closes an open result menu whenever the authoritative name/content result array
is replaced, preventing actions from targeting a stale row. That synchronization now runs in a
layout effect, before the replacement rows are painted and can receive keyboard input.

The previous passive effect could remain queued after Testing Library (or a fast user) observed a
fresh row. If Shift+F10 opened its menu in that interval, the old replacement cleanup ran afterward
and immediately closed the new menu. The failed CI DOM showed that the row had been selected by
`onBeforeOpen` while the menu itself had already disappeared, matching this race.

No context-menu action, result ownership rule, search behavior, persistence, or external operation
changes. The existing Shift+F10/focus-restoration fixture remains unchanged so it continues to cover
the user-visible contract instead of accommodating the race with a retry.

## Files changed

- `apps/everything-plus/src/App.tsx`
- `workthrough/2026-08-26-everything-context-menu-result-race.md`

## Verification

- Existing `src/App.test.tsx`: 11 passed in five consecutive runs (55 assertions total), including
  the unchanged Shift+F10/focus-restoration fixture each time.
- Everything+ TypeScript/Vite build passed.
- `git diff --check` passed.
