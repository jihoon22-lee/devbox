import { createCurrentProjectStore } from "./store";
test("selection survives reload, validates persisted type and tolerates blocked storage", () => {
  localStorage.clear();
  const first = createCurrentProjectStore();
  first.getState().set("owned-project");
  expect(createCurrentProjectStore().getState().id).toBe("owned-project");
  localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id: 3 }, version: 0 }));
  expect(createCurrentProjectStore().getState().id).toBeUndefined();
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw Error("blocked");
  });
  const blocked = createCurrentProjectStore();
  expect(() => blocked.getState().set("safe")).not.toThrow();
  expect(blocked.getState().id).toBe("safe");
});
