import { autoCollapse, useLayout } from "./layout";

beforeEach(() => {
  localStorage.clear();
  useLayout.setState({ listWidth: 260, listCollapsed: false, bottomOpen: false, bottomHeight: 260 });
});
test("breakpoints and persisted toggles", () => {
  expect(autoCollapse(1100, 760)).toEqual({ list: false, bottom: false });
  expect(autoCollapse(1099, 759)).toEqual({ list: true, bottom: true });
  useLayout.getState().toggleBottom();
  useLayout.getState().toggleList();
  expect(JSON.parse(localStorage.getItem("devbox.layout")!).state).toMatchObject({
    bottomOpen: true,
    listCollapsed: true,
  });
});
test("rehydration accepts only finite clamped primitives and preserves actions", async () => {
  localStorage.setItem(
    "devbox.layout",
    JSON.stringify({
      state: { listWidth: 900, bottomHeight: -10, listCollapsed: "false", bottomOpen: true, toggleList: null },
      version: 0,
    }),
  );
  await useLayout.persist.rehydrate();
  expect(useLayout.getState()).toMatchObject({
    listWidth: 480,
    bottomHeight: 120,
    listCollapsed: false,
    bottomOpen: true,
  });
  expect(typeof useLayout.getState().toggleList).toBe("function");
  useLayout.getState().setListWidth(Number.NaN);
  useLayout.getState().setBottomHeight(Infinity);
  expect(useLayout.getState()).toMatchObject({ listWidth: 480, bottomHeight: 120 });
});
test("unavailable storage cannot prevent layout changes", async () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw Error("unavailable");
  });
  expect(() => useLayout.getState().toggleBottom()).not.toThrow();
  expect(useLayout.getState().bottomOpen).toBe(true);
});
