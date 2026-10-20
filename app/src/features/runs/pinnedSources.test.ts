import { createPinnedLogSourcesStore } from "./pinnedSources";

beforeEach(() => localStorage.removeItem("devbox.pinnedLogSources"));
test("pins persist by project and duplicate sources are removed", () => {
  const store = createPinnedLogSourcesStore();
  store.getState().set("p", [
    { kind: "run", runId: "r" },
    { kind: "run", runId: "r" },
  ]);
  store.getState().set("q", [{ kind: "file", path: "/q.log" }]);
  const restored = createPinnedLogSourcesStore();
  expect(restored.getState().byProject).toEqual({
    p: [{ kind: "run", runId: "r" }],
    q: [{ kind: "file", path: "/q.log" }],
  });
  restored.getState().clear("p");
  expect(createPinnedLogSourcesStore().getState().byProject).toEqual({ q: [{ kind: "file", path: "/q.log" }] });
});
test("invalid persisted entries are discarded and oversized lists are bounded", () => {
  localStorage.setItem(
    "devbox.pinnedLogSources",
    JSON.stringify({
      state: {
        byProject: {
          p: [
            { kind: "file", path: "" },
            { kind: "unknown", id: "x" },
            { kind: "run", runId: 5 },
          ],
          q: Array.from({ length: 12 }, (_, i) => ({ kind: "run", runId: String(i) })),
        },
      },
      version: 0,
    }),
  );
  const restored = createPinnedLogSourcesStore();
  expect(restored.getState().byProject.p).toBeUndefined();
  expect(restored.getState().byProject.q).toHaveLength(8);
});
test("storage failures preserve in-memory pins", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  const store = createPinnedLogSourcesStore();
  store.getState().set("p", [{ kind: "run", runId: "r" }]);
  expect(store.getState().byProject.p).toEqual([{ kind: "run", runId: "r" }]);
});
