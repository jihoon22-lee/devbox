import { act, cleanup, fireEvent, render, renderHook, waitFor } from "@testing-library/react";
import { ProjectWire } from "../features/projects/test-wire";
import { useSystemMetrics } from "../features/runs/systemMetrics";
import { useRpcQuery } from "../rpc/query";
import { RpcProvider } from "../rpc/RpcProvider";
import { pageVisible, usePageVisible } from "./visibility";

type Shown = { payload: { label: string; shown: boolean } };
const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), label: "main" }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: native.label }) }));

let handlers: ((event: Shown) => void)[] = [];
const report = (label: string, shown: boolean) => {
  for (const handler of [...handlers]) handler({ payload: { label, shown } });
};
/** A host whose window starts as `initial`; `false` covers a sign-in start straight to the tray. */
function host(initial: boolean | Promise<boolean>, label = "main") {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  native.label = label;
  native.invoke.mockImplementation(async (command: string) => (command === "window_shown" ? initial : undefined));
  native.listen.mockImplementation(async (name: string, handler: (event: Shown) => void) => {
    if (name !== "devbox://window-shown") return () => {};
    handlers.push(handler);
    return () => {
      handlers = handlers.filter((entry) => entry !== handler);
    };
  });
}
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  native.invoke.mockReset();
  native.listen.mockReset();
  handlers = [];
  vi.useRealTimers();
});

test("without a host the page's own visibility decides and nothing native is asked", async () => {
  let state: DocumentVisibilityState = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => state);
  const view = renderHook(() => usePageVisible());
  expect(view.result.current).toBe(true);
  await act(async () => {
    state = "hidden";
    fireEvent(document, new Event("visibilitychange"));
  });
  expect(view.result.current).toBe(false);
  expect(native.invoke).not.toHaveBeenCalled();
  expect(native.listen).not.toHaveBeenCalled();
});

test("a host that never answers or lacks the command leaves the page visible", async () => {
  host(true);
  native.invoke.mockRejectedValue(new Error("unknown command"));
  const view = renderHook(() => usePageVisible());
  await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("window_shown"));
  expect(view.result.current).toBe(true);
  view.unmount();
  native.invoke.mockResolvedValue(undefined);
  native.listen.mockRejectedValue(new Error("no listener"));
  const next = renderHook(() => usePageVisible());
  await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(2));
  expect(next.result.current).toBe(true);
});

test("the host hides and shows this window only, by its own label", async () => {
  host(true, "popout-agent-a1");
  const view = renderHook(() => usePageVisible());
  await waitFor(() => expect(handlers).toHaveLength(1));
  await act(async () => report("main", false));
  expect(view.result.current).toBe(true);
  await act(async () => report("popout-agent-a1", false));
  expect(view.result.current).toBe(false);
  expect(pageVisible()).toBe(false);
  await act(async () => report("main", true));
  expect(view.result.current).toBe(false);
  await act(async () => report("popout-agent-a1", true));
  expect(view.result.current).toBe(true);
  view.unmount();
  expect(handlers).toHaveLength(0);
  // Nothing is left from a host that is no longer listened to.
  expect(pageVisible()).toBe(true);
});

test("a window that starts hidden is hidden before any event, and a later event outranks a late answer", async () => {
  host(false);
  const view = renderHook(() => usePageVisible());
  await waitFor(() => expect(view.result.current).toBe(false));
  await act(async () => report("main", true));
  expect(view.result.current).toBe(true);
  view.unmount();

  let answer = (_shown: boolean) => {};
  host(new Promise<boolean>((resolve) => (answer = resolve)));
  const late = renderHook(() => usePageVisible());
  await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("window_shown"));
  await act(async () => report("main", false));
  await act(async () => answer(true));
  expect(late.result.current).toBe(false);
});

test("focusing the window asks the host again", async () => {
  host(true);
  const view = renderHook(() => usePageVisible());
  await waitFor(() => expect(handlers).toHaveLength(1));
  await act(async () => report("main", false));
  expect(view.result.current).toBe(false);
  await act(async () => fireEvent.focus(window));
  await waitFor(() => expect(view.result.current).toBe(true));
});

function Metrics() {
  useSystemMetrics();
  return null;
}
test("status metrics stop while the host reports the window hidden and refetch at once when shown", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  host(true);
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Metrics />
    </RpcProvider>,
  );
  const methods = ["observe.system", "observe.ports"];
  await waitFor(() => {
    for (const method of methods) expect(wire.requests(method)).toHaveLength(1);
  });
  await act(async () => {
    wire.reply("observe.system", { memoryTotalBytes: 2, memoryAvailableBytes: 1 });
    wire.reply("observe.ports", []);
  });
  await waitFor(() => expect(handlers).toHaveLength(1));
  await act(async () => report("main", false));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  for (const method of methods) expect(wire.requests(method)).toHaveLength(1);
  await act(async () => report("main", true));
  // No timer advance: the refetch does not wait for the next interval.
  await waitFor(() => {
    for (const method of methods) expect(wire.requests(method)).toHaveLength(2);
  });
  await act(async () => {
    wire.reply("observe.system", { memoryTotalBytes: 2, memoryAvailableBytes: 1 });
    wire.reply("observe.ports", []);
    await vi.advanceTimersByTimeAsync(3100);
  });
  for (const method of methods) expect(wire.requests(method)).toHaveLength(3);
});

function Plain() {
  useRpcQuery("runtime.containers", {});
  return null;
}
test("a query without an interval does not watch the window at all", async () => {
  host(true);
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Plain />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.containers")).toHaveLength(1));
  expect(native.listen).not.toHaveBeenCalled();
  expect(native.invoke).not.toHaveBeenCalled();
});

function Polls({ background }: { background?: boolean }) {
  useRpcQuery("runtime.containers", {}, { refetchInterval: 5000, refetchIntervalInBackground: background });
  return null;
}
test("any interval query pauses while hidden and catches up when shown, unless it polls in the background", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  host(true);
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <Polls />
    </RpcProvider>,
  );
  const sent = () => wire.requests("runtime.containers").length;
  await waitFor(() => expect(sent()).toBe(1));
  await act(async () => wire.reply("runtime.containers", { items: [] }));
  await waitFor(() => expect(handlers).toHaveLength(1));
  await act(async () => report("main", false));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(sent()).toBe(1);
  await act(async () => report("main", true));
  await waitFor(() => expect(sent()).toBe(2));
  await act(async () => wire.reply("runtime.containers", { items: [] }));

  view.rerender(
    <RpcProvider client={wire.client}>
      <Polls background />
    </RpcProvider>,
  );
  await act(async () => report("main", false));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5100);
  });
  expect(sent()).toBe(3);
});
