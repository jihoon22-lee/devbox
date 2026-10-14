import { useToasts } from "../../ui/Toast";
import { useNotifications } from "./store";

beforeEach(() => useNotifications.getState().clear());
afterEach(() => useToasts.getState().items.forEach((item) => useToasts.getState().dismiss(item.id)));
test("replacement moves the same agent first and resets unread", () => {
  const n = useNotifications.getState();
  n.push({ key: "agent:a", id: "a", kind: "agent", title: "입력 대기", body: "needs Bash" });
  n.push({ key: "agent:b", kind: "agent", title: "다른 작업", body: "" });
  n.markAllRead();
  n.push({ key: "agent:a", kind: "agent", title: "다음 지시 대기", body: "done" });
  expect(useNotifications.getState().items.map((x) => x.key)).toEqual(["agent:a", "agent:b"]);
  expect(useNotifications.getState().unread).toBe(1);
  n.markRead("agent:a");
  expect(useNotifications.getState().unread).toBe(0);
});
test("retains the latest 100 items and derives unread", () => {
  for (let i = 0; i < 120; i++)
    useNotifications.getState().push({ key: `e${i}`, kind: "error", title: `${i}`, body: "" });
  expect(useNotifications.getState().items).toHaveLength(100);
  expect(useNotifications.getState().items[0]?.key).toBe("e119");
  expect(useNotifications.getState().unread).toBe(100);
  useNotifications.getState().clear();
  expect(useNotifications.getState().unread).toBe(0);
});
test("hydration validates items, bounds history and refuses unsafe targets", async () => {
  const valid = { key: "a", kind: "agent", title: "작업", body: "", atMs: 1, read: false };
  localStorage.setItem(
    "devbox.notifications",
    JSON.stringify({
      state: {
        unread: 999,
        items: [
          { ...valid, target: { to: "https://evil.test", search: { id: "a" } } },
          { ...valid, key: "b", target: { to: "/agents", search: { id: "b", gateway: "ws://evil.test" } } },
          { ...valid },
          { ...valid, key: "bad", atMs: "now" },
          { ...valid, key: "bad2", read: "yes" },
        ],
      },
      version: 0,
    }),
  );
  await useNotifications.persist.rehydrate();
  const state = useNotifications.getState();
  expect(state.items.map((x) => x.key)).toEqual(["a", "b"]);
  expect(state.items[0]?.target).toBeUndefined();
  expect(state.items[1]?.target).toEqual({ to: "/agents", search: { id: "b" } });
  expect(state.unread).toBe(2);
  expect(typeof state.push).toBe("function");
});
test("denied storage keeps memory usable; danger toast persists after expiry", async () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("denied");
  });
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("denied");
  });
  await useNotifications.persist.rehydrate();
  vi.useFakeTimers();
  try {
    useToasts.getState().push({ tone: "danger", title: "실패", body: "이유" });
    useToasts.getState().push({ tone: "warn", title: "경고" });
    vi.advanceTimersByTime(5000);
    expect(useToasts.getState().items).toHaveLength(0);
    expect(useNotifications.getState().items).toHaveLength(1);
    expect(useNotifications.getState().items[0]).toMatchObject({ kind: "error", title: "실패", body: "이유" });
  } finally {
    vi.useRealTimers();
  }
});

test("a stale popout write preserves main notices and storage events refresh memory", () => {
  const n = useNotifications.getState();
  n.push({ key: "old", kind: "error", title: "old", body: "" });
  const stale = useNotifications.getState().items;
  n.push({ key: "main", kind: "agent", title: "main", body: "" });
  const latest = localStorage.getItem("devbox.notifications")!;
  useNotifications.setState({ items: stale, unread: 1 });
  localStorage.setItem("devbox.notifications", latest);
  n.push({ key: "popout", kind: "error", title: "popout", body: "" });
  expect(useNotifications.getState().items.map((item) => item.key)).toEqual(["popout", "main", "old"]);
  localStorage.setItem(
    "devbox.notifications",
    JSON.stringify({
      state: { items: [{ key: "other", kind: "agent", title: "other", body: "", atMs: 1, read: true }] },
      version: 0,
    }),
  );
  window.dispatchEvent(new StorageEvent("storage", { key: "devbox.notifications", storageArea: localStorage }));
  expect(useNotifications.getState().items.map((item) => item.key)).toEqual(["other"]);
  expect(useNotifications.getState().unread).toBe(0);
});

test("denied writes with readable old storage preserve successive memory notices", () => {
  const n = useNotifications.getState();
  n.push({ key: "old", kind: "error", title: "old", body: "" });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  n.push({ key: "one", kind: "error", title: "one", body: "" });
  n.push({ key: "two", kind: "error", title: "two", body: "" });
  window.dispatchEvent(new StorageEvent("storage", { key: "devbox.notifications", storageArea: localStorage }));
  expect(useNotifications.getState().items.map((item) => item.key)).toEqual(["two", "one", "old"]);
});
