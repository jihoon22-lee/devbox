import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { profiles, task } from "../features/agents/test-fixtures";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useLayout } from "./layout";
import { Shell } from "./Shell";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), label: "main" }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: mocks.label }) }));
vi.mock("../features/terminal/TerminalView", () => ({ TerminalView: () => <div /> }));
const intent = (sequence: number, id: string) => ({ sequence, to: "/agents", search: { id, view: "list" } });
beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.listen.mockReset();
  mocks.label = "main";
  mocks.invoke.mockResolvedValue(undefined);
  mocks.listen.mockResolvedValue(vi.fn());
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1280 });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
  useLayout.setState({ listCollapsed: false, bottomOpen: false });
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  window.history.replaceState({}, "", "/");
  vi.unstubAllGlobals();
});
async function setup() {
  const wire = new ProjectWire();
  const view = render(
    <StrictMode>
      <RpcProvider client={wire.client}>
        <TooltipProvider>
          <Shell />
        </TooltipProvider>
      </RpcProvider>
    </StrictMode>,
  );
  await waitFor(() => expect(wire.requests("agents.list").length).toBeGreaterThan(0));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: projects });
    wire.reply("agents.list", { rev: "1", items: [task("a1", "waiting"), task("b2", "idle")] });
    wire.reply("agents.profiles", { items: profiles });
  });
  return { wire, view };
}
test("main Shell consumes cold intent after mounting and second-instance activation opens list detail from grid", async () => {
  window.history.replaceState({}, "", "/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  const listeners = new Set<(event: { payload: unknown }) => void>();
  mocks.listen.mockImplementation(async (_name, callback) => {
    listeners.add(callback);
    return () => listeners.delete(callback);
  });
  mocks.invoke.mockImplementation((name) =>
    Promise.resolve(name === "pending_navigation" ? intent(1, "a1") : undefined),
  );
  const { wire, view } = await setup();
  await waitFor(() => expect(window.location.search).toContain("id=a1"));
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("acknowledge_navigation", { sequence: 1 }));
  expect(window.location.pathname).toBe("/agents");
  expect(window.location.search).toContain("view=list");
  expect(window.location.search).toContain("gateway=");
  expect(listeners.size).toBe(1);
  const subscriptions = wire.sent.filter((message) => message.topic === "agents.attention");
  fireEvent.click(screen.getByRole("button", { name: "격자" }));
  await waitFor(() => expect(window.location.search).toContain("view=grid"));
  await act(async () => listeners.forEach((callback) => callback({ payload: intent(2, "b2") })));
  await waitFor(() => expect(window.location.search).toContain("id=b2"));
  expect(window.location.search).toContain("view=list");
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(subscriptions.length);
  view.unmount();
  expect(listeners.size).toBe(0);
});
test("popout identity does not subscribe to navigation or invoke native counts on an ordinary section route", async () => {
  mocks.label = "popout-agent-a1";
  window.history.replaceState({}, "", "/agents?renderer=dom");
  const { wire } = await setup();
  expect(mocks.listen).not.toHaveBeenCalled();
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(0);
});
test("global bottom terminal brings a background open panel forward and focuses the existing session, while foreground open panel hides", async () => {
  const { view } = await setup();
  const { useNativeRuntime } = await import("./nativeRuntime");
  act(() => useLayout.setState({ bottomOpen: true }));
  act(() => useNativeRuntime.getState().push({ kind: "bottom-terminal", wasFocused: false }));
  await waitFor(() => expect(useNativeRuntime.getState().bottomFocus).toBeGreaterThan(0));
  expect(useLayout.getState().bottomOpen).toBe(true);
  expect(mocks.invoke).not.toHaveBeenCalledWith("hide_main");
  act(() => useNativeRuntime.getState().push({ kind: "bottom-terminal", wasFocused: true }));
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("hide_main"));
  expect(useLayout.getState().bottomOpen).toBe(true);
  view.unmount();
});
test("global palette preserves route search and queues while an existing modal is open", async () => {
  window.history.replaceState({}, "", "/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  const { view } = await setup();
  const { useNativeRuntime } = await import("./nativeRuntime");
  const blocker = document.createElement("div");
  blocker.setAttribute("role", "dialog");
  document.body.append(blocker);
  act(() => useNativeRuntime.getState().push({ kind: "palette" }));
  expect(screen.queryByRole("combobox", { name: "명령 검색" })).not.toBeInTheDocument();
  await act(async () => blocker.remove());
  expect(await screen.findByRole("combobox", { name: "명령 검색" })).toBeVisible();
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  view.unmount();
});
