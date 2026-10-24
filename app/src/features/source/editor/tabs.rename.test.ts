import { expect, test } from "vitest";
import { emptyTabs, reduceTabs } from "./tabs";

test.each([false, true])("rename then reuse original path keeps independent identities (folder=%s)", (folder) => {
  const a = { root: "project:p", path: folder ? "src/a.ts" : "a.ts" };
  const b = { ...a, path: folder ? "dest/a.ts" : "b.ts" };
  let s = reduceTabs(emptyTabs(), { type: "open", key: a, pinned: true });
  const originalId = s.tabs[0]!.id;
  s = reduceTabs(s, { type: "move", key: a, group: 1, index: 0 });
  s = reduceTabs(s, {
    type: "rename",
    from: folder ? { ...a, path: "src" } : a,
    to: folder ? { ...b, path: "dest" } : b,
  });
  s = reduceTabs(s, { type: "open", key: a, pinned: true, group: 0 });
  expect(s.tabs).toHaveLength(2);
  expect(new Set(s.tabs.map((t) => t.id)).size).toBe(2);
  expect(s.tabs.find((t) => t.id === s.active[0])?.key).toEqual(a);
  expect(s.tabs.find((t) => t.id === s.active[1])?.key).toEqual(b);
  expect(s.tabs.find((t) => t.key.path === b.path)?.id).toBe(originalId);
  for (const key of [a, b]) {
    const id = s.tabs.find((t) => t.key.path === key.path)!.id;
    s = reduceTabs(s, { type: "activate", id });
    expect(s.active[s.group]).toBe(id);
    s = reduceTabs(s, { type: "move", key, group: 0, index: 0 });
    expect(s.tabs.filter((t) => t.group === 0)[0]).toMatchObject({ id, key });
  }
  s = reduceTabs(s, { type: "closed", key: a });
  expect(s.tabs.map((t) => t.key)).toEqual([b]);
  expect(s.active[0]).toBe(originalId);
  s = reduceTabs(s, { type: "closed", key: b });
  expect(s.tabs).toEqual([]);
  expect(s.active).toEqual([undefined, undefined]);
});
