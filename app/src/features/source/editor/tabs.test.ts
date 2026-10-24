import { expect, test } from "vitest";
import { emptyTabs, reduceTabs } from "./tabs";

const a = { root: "project:p", path: "a.ts" };
const b = { root: "project:p", path: "b.ts" };
const c = { root: "project:q", path: "a.ts" };
const open = (key: typeof a) => ({ type: "open" as const, key });
test("a clean preview is replaced but editing pins it before opening another file", () => {
  let s = reduceTabs(emptyTabs(), open(a));
  s = reduceTabs(s, open(b));
  expect(s.tabs.map((t) => t.key)).toEqual([b]);
  s = reduceTabs(s, { type: "pin", key: b });
  s = reduceTabs(s, open(a));
  expect(s.tabs.map((t) => [t.key.path, t.pinned])).toEqual([
    ["b.ts", true],
    ["a.ts", false],
  ]);
});
test("split moves the single document tab and reopening activates its existing group", () => {
  let s = reduceTabs(emptyTabs(), open(a));
  s = reduceTabs(s, { type: "pin", key: a });
  s = reduceTabs(s, open(b));
  s = reduceTabs(s, { type: "move", key: a, group: 1, index: 0 });
  s = reduceTabs(s, open(a));
  expect(s.tabs.filter((t) => t.key.path === "a.ts")).toHaveLength(1);
  expect(s.group).toBe(1);
  expect(s.tabs.find((t) => t.key.path === "a.ts")?.group).toBe(1);
  s = reduceTabs(s, open(c));
  expect(s.tabs).toHaveLength(3);
});
test("close is only committed after the caller's durable choice; moved keys retain tab identity", () => {
  let s = reduceTabs(emptyTabs(), open(a));
  s = reduceTabs(s, { type: "pin", key: a });
  const id = s.tabs[0]?.id;
  const renamed = { ...a, path: "renamed.ts" };
  s = reduceTabs(s, { type: "rename", from: a, to: renamed });
  expect(s.tabs[0]).toMatchObject({ id, key: renamed, pinned: true });
  s = reduceTabs(s, { type: "closed", key: renamed });
  expect(s.tabs).toHaveLength(0);
});
