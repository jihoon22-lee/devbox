# API Playground MCP draft preservation

## Overview

This change makes MCP tool and prompt argument drafts independent for every selected item in the
current Protocol Lab connection. Editing a second tool or prompt no longer overwrites the draft for
the first one, while disconnecting or resetting the explorer still discards every draft in memory.

The work is frontend-only. It does not change MCP transport, native validation, persistence,
protocol schemas, dependencies, package versions or release assets.

## Context

The earlier argument-reset race fix removed effects that could overwrite input one commit after an
editor appeared. It replaced each argument object with a render-derived draft shaped like this:

```ts
{ name: string, values: Record<string, unknown> }
```

That object has only one value slot. Switching from tool A to tool B appeared to preserve A as long
as B remained untouched, because A was still the last stored draft. Once the user edited B, however,
the state replaced A completely. Returning to A recreated initial values and silently lost the
user's input. Prompt arguments had the same behavior. The existing switch tests did not expose the
gap because they never edited the second selection.

MCP list names are server-provided data, even though the frontend validates their shape and bounds.
Using `Map` gives exact string identity without interpreting names such as `__proto__` as object
properties. Draft lifetime remains tied to one connection so argument content is not persisted or
carried to another server.

## Changes made

### 1. Store drafts by exact MCP item name

File:

- `apps/api-playground/src/ProtocolLab.tsx`

Tool and prompt state now use separate read-only maps. A render first looks up the selected name and
falls back to schema-derived initial arguments only when that item has no draft. Updates clone the
map and replace only the selected entry, leaving all other drafts intact.

`resetExplorer` replaces both maps with empty maps. This path already runs before a new connection,
after disconnect, on a stale connection and during the relevant transport/profile resets, so no MCP
argument crosses the existing connection boundary.

### 2. Exercise the real overwrite sequence

File:

- `apps/api-playground/src/ProtocolLab.test.tsx`

The prior prompt and tool switch tests now edit both selections before switching back through each
one. They verify both independent values and assert that the currently selected draft is passed to
`prompts/get` or `tools/call`. This sequence deterministically fails with the old single-slot state.

A separate regression disconnects and reconnects after entering a prompt value, reloads the same
prompt name, and verifies that its input is empty. This protects the connection-local privacy
boundary independently of the preservation behavior.

### 3. Document the user and protocol contract

Files:

- `apps/api-playground/README.md`
- `docs/superpowers/specs/2026-08-30-api-playground-protocol-lab.md`
- `CHANGELOG.md`
- `workthrough/2026-09-03-api-playground-mcp-draft-preservation.md`

The README, MCP design specification and Unreleased changelog now state that drafts are isolated by
validated name for one connection and discarded on reset or disconnect. No dependency or generated
asset changed.

## Code examples

### Replace the single draft slot

```tsx
// Before: editing another item replaces the only stored draft.
const [toolDraft, setToolDraft] = useState({ name: "", values: {} });

// After: every exact selected name owns its connection-local value object.
const [toolDrafts, setToolDrafts] = useState<
  ReadonlyMap<string, Record<string, unknown>>
>(() => new Map());
```

### Update only the selected tool

```tsx
// apps/api-playground/src/ProtocolLab.tsx
onChange={(values) => setToolDrafts((current) => {
  const next = new Map(current);
  next.set(selectedToolName, values);
  return next;
})}
```

### Preserve the connection boundary

```tsx
// apps/api-playground/src/ProtocolLab.tsx
const resetExplorer = () => {
  // List, selection, result and timeline resets are omitted here.
  setToolDrafts(new Map());
  setPromptDrafts(new Map());
};
```

## Verification results

### Focused Protocol Lab regression

```text
pnpm --filter api-playground test -- ProtocolLab.test.tsx
1 file / 17 tests passed
Exit code: 0
```

Coverage now verifies:

- tool A and tool B retain distinct edited arguments through repeated switches;
- prompt A and prompt B retain distinct edited arguments through repeated switches;
- invocation receives the draft belonging to the exact current selection;
- disconnect and reconnect discard all connection-local argument drafts.

### Affected workspace verification

```text
pnpm verify:affected
scope resolver and runner regression tests passed
frontend_scope=apps
frontend_packages=apps/api-playground
rust_scope=none
dependency_scope=none
API Playground: 37 files / 278 tests passed
initial JS: 595425 bytes raw / 177065 bytes gzip
bundle budget: 685000 bytes raw / 205000 bytes gzip
Additional TypeScript packages: 0
Elapsed: 15.0 s
Exit code: 0
```

Only API Playground was built and tested. Cargo, dependency audits and the other 14 applications
were not started. Vite's pre-existing large-chunk and mixed static/dynamic Tauri event import
advisories remain informational; the measured initial bundle stays within the repository budget.

### Static repository contracts

```text
node .github/scripts/check-frontend-accessibility.mjs   PASS (15 static app contracts)
bash .github/scripts/check-catalog.sh                   PASS
git diff --check                                        PASS
```

These checks inspect source and catalog contracts without building every application. A Windows
packaged run is not required for this renderer-state change; native calls are covered at the mocked
boundary and the native MCP validation path is unchanged.

## Next steps

- Require this PR's affected-scope GitHub Actions checks to pass before merge.
- Continue the shared asynchronous AppLink/listener lifecycle audit as a separate rollback boundary.
