import { afterEach, beforeEach, expect, test } from "vitest";
import { memoryJournal } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { FakeDocumentsDaemon } from "../documents/test-daemon";
import { emptyTabs } from "./editor/tabs";
import { syncDocumentTabs, useSource } from "./store";

const a = { root: "project:p", path: "a.ts" },
  b = { root: "project:p", path: "b.ts" };
let stop: () => void, off: () => void;
beforeEach(() => {
  const daemon = new FakeDocumentsDaemon();
  daemon.write(a, "A");
  daemon.write(b, "B");
  stop = useDocuments.getState().connect(daemon, { journal: memoryJournal() });
  useSource.setState({ layout: emptyTabs(), recent: [], locations: {} });
  off = syncDocumentTabs();
});
afterEach(() => {
  off();
  stop();
});
test("dirty previews pin synchronously and opening another file preserves their buffer", async () => {
  await useSource.getState().open(a);
  await useDocuments.getState().settled();
  useDocuments.getState().edit(a, "draft");
  await useSource.getState().open(b);
  expect(useSource.getState().layout.tabs).toHaveLength(2);
  expect(useSource.getState().layout.tabs.find((t) => t.key.path === "a.ts")?.pinned).toBe(true);
  expect(useDocuments.getState().find(a)?.text).toBe("draft");
});
test("a durable close removes a tab but a missing choice does not discard it", async () => {
  await useSource.getState().open(a);
  await useDocuments.getState().settled();
  useDocuments.getState().edit(a, "draft");
  expect(await useSource.getState().close(a)).toBe(false);
  expect(useSource.getState().layout.tabs).toHaveLength(1);
  expect(await useSource.getState().close(a, "keep")).toBe(true);
  expect(useSource.getState().layout.tabs).toHaveLength(0);
});
