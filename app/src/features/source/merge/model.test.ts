import { beforeEach, expect, test } from "vitest";
import type { ConflictPreview } from "../../../rpc/gen/rpc";
import { choose, mergeKey, useMergeDrafts } from "./model";

const target = { projectId: "p" },
  key = mergeKey(target, "file");
export function preview(text = "markers"): ConflictPreview {
  return {
    base: "base",
    ours: "ours",
    theirs: "theirs",
    merged: text,
    textSupported: true,
    labels: { base: "base", ours: "main", theirs: "side", rebase: false },
    token: { target, path: "file", worktreeHash: text, stageOids: ["a", "b", "c"], identity: "identity" },
  };
}
beforeEach(() => useMergeDrafts.setState({ entries: {} }));
test("resolution_text_is_preserved_after_stale", () => {
  const s = useMergeDrafts.getState();
  const first = s.begin(key);
  s.receive(key, first, preview());
  s.edit(key, "my resolution");
  s.fail(key, first, "changed");
  const reload = s.begin(key);
  s.receive(key, reload, preview("external current"));
  expect(useMergeDrafts.getState().entries[key]?.resolution).toBe("my resolution");
  expect(useMergeDrafts.getState().entries[key]?.preview?.merged).toBe("external current");
});
test("older preview responses cannot replace latest token or an edited draft", () => {
  const s = useMergeDrafts.getState();
  const first = s.begin(key);
  const second = s.begin(key);
  s.edit(key, "typed while loading");
  s.receive(key, second, preview("new"));
  s.receive(key, first, preview("old"));
  expect(useMergeDrafts.getState().entries[key]?.resolution).toBe("typed while loading");
  expect(useMergeDrafts.getState().entries[key]?.preview?.token.worktreeHash).toBe("new");
});
test("target keys isolate drafts and side choices include absent stages", () => {
  expect(mergeKey({ ...target, worktreeId: "agent" }, "file")).not.toBe(key);
  expect(choose(preview(), "both")).toBe("ours\ntheirs");
  expect(choose({ ...preview(), ours: null }, "ours")).toBeNull();
});
