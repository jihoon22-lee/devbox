import { nativeAttention, nativeBadge, subscribeNativeNavigation } from "./native";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), label: "main" }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: mocks.label }) }));
const intent = (sequence: number, id = "a1") => ({ sequence, to: "/agents", search: { id, view: "list" } });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
async function tick() {
  for (let index = 0; index < 8; index++) await Promise.resolve();
}
beforeEach(() => {
  mocks.label = "main";
  mocks.invoke.mockReset();
  mocks.listen.mockReset();
  mocks.invoke.mockResolvedValue(undefined);
  document.title = "devbox";
});
afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});
function native() {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
}
test("browser uses the title fallback without IPC", async () => {
  nativeBadge(2, 5);
  await nativeAttention({ id: "a1", title: "입력 대기", body: "확인", visible: true });
  const stop = subscribeNativeNavigation(vi.fn());
  stop();
  expect(document.title).toBe("(2) devbox");
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(mocks.listen).not.toHaveBeenCalled();
});
test("only native main host invokes typed attention and both task counts", async () => {
  native();
  const attention = { id: "a1", title: "입력 대기", body: "확인", visible: false };
  await nativeAttention(attention);
  nativeBadge(2, 5);
  await tick();
  expect(mocks.invoke).toHaveBeenCalledWith("notify_attention", { a: attention });
  expect(mocks.invoke).toHaveBeenCalledWith("set_badge", { attention: 2, open: 5 });
  mocks.label = "popout-agent-a1";
  mocks.invoke.mockClear();
  await nativeAttention(attention);
  nativeBadge(3, 6);
  const stop = subscribeNativeNavigation(vi.fn());
  stop();
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(mocks.listen).not.toHaveBeenCalled();
});
test("cold snapshot is read after listener registration and new event wins over stale snapshot", async () => {
  native();
  const snapshot = deferred<unknown>();
  const release = vi.fn();
  let deliver!: (event: { payload: unknown }) => void;
  mocks.listen.mockImplementation(async (_name, callback) => {
    deliver = callback;
    return release;
  });
  mocks.invoke.mockImplementation((name) => (name === "pending_navigation" ? snapshot.promise : Promise.resolve()));
  const navigate = vi.fn().mockResolvedValue(undefined);
  const stop = subscribeNativeNavigation(navigate);
  await tick();
  expect(mocks.listen).toHaveBeenCalledWith("devbox://navigate", expect.any(Function));
  expect(mocks.invoke).toHaveBeenCalledWith("pending_navigation");
  deliver({ payload: intent(2, "b2") });
  await tick();
  snapshot.resolve(intent(1));
  await tick();
  deliver({ payload: intent(2, "b2") });
  deliver({ payload: { ...intent(3), to: "/settings" } });
  await tick();
  expect(navigate).toHaveBeenCalledExactlyOnceWith({ to: "/agents", search: { id: "b2", view: "list" } });
  expect(mocks.invoke).toHaveBeenCalledWith("acknowledge_navigation", { sequence: 2 });
  stop();
  expect(release).toHaveBeenCalledTimes(1);
});
test("cold intent is acknowledged only after navigation completes", async () => {
  native();
  const navigation = deferred<void>();
  mocks.listen.mockResolvedValue(vi.fn());
  mocks.invoke.mockImplementation((name) => Promise.resolve(name === "pending_navigation" ? intent(1) : undefined));
  const navigate = vi.fn(() => navigation.promise);
  const stop = subscribeNativeNavigation(navigate);
  await tick();
  expect(navigate).toHaveBeenCalledOnce();
  expect(mocks.invoke).not.toHaveBeenCalledWith("acknowledge_navigation", expect.anything());
  navigation.resolve();
  await tick();
  expect(mocks.invoke).toHaveBeenCalledWith("acknowledge_navigation", { sequence: 1 });
  stop();
});
test("StrictMode abandoned registration cleans late listener and retains unconsumed intent", async () => {
  native();
  const registration = deferred<() => void>();
  const release = vi.fn();
  const navigate = vi.fn();
  let deliver!: (event: { payload: unknown }) => void;
  mocks.listen.mockImplementation((_name, callback) => {
    deliver = callback;
    return registration.promise;
  });
  const stop = subscribeNativeNavigation(navigate);
  stop();
  deliver({ payload: intent(1) });
  registration.resolve(release);
  await tick();
  expect(release).toHaveBeenCalledOnce();
  expect(navigate).not.toHaveBeenCalled();
  expect(mocks.invoke).not.toHaveBeenCalled();
  mocks.listen.mockResolvedValue(release);
  mocks.invoke.mockImplementation((name) => Promise.resolve(name === "pending_navigation" ? intent(1) : undefined));
  const nextStop = subscribeNativeNavigation(navigate);
  await tick();
  expect(navigate).toHaveBeenCalledOnce();
  nextStop();
});
