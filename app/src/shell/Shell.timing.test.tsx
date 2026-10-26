import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { profiles } from "../features/agents/test-fixtures";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useNativeRuntime } from "./nativeRuntime";
import { Shell } from "./Shell";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), label: "main" }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: mocks.label }) }));
vi.mock("../features/terminal/TerminalView", () => ({ TerminalView: () => <div /> }));
beforeEach(() => {
  mocks.invoke.mockReset().mockResolvedValue(undefined);
  mocks.label = "main";
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  useNativeRuntime.setState({ intents: [] });
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
const frames = () =>
  act(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name);
async function mount(reply = true) {
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
  if (reply) await respond(wire);
  return { wire, view };
}
async function respond(wire: ProjectWire) {
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: projects });
    wire.reply("agents.list", { rev: "1", items: [] });
    wire.reply("agents.profiles", { items: profiles });
  });
}
test("popouts never acknowledge first data paint before main does", async () => {
  mocks.label = "popout-agent-a1";
  await mount();
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeVisible();
  await frames();
  expect(calls("first_data_painted")).toHaveLength(0);
});
test("first data paint is acknowledged once after the agent list renders, not when the transport is ready", async () => {
  const first = await mount(false);
  expect(first.wire.client.state).toBe("ready");
  await frames();
  expect(calls("first_data_painted")).toHaveLength(0);
  await respond(first.wire);
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeVisible();
  await waitFor(() => expect(calls("first_data_painted")).toEqual([["first_data_painted"]]));
  first.view.unmount();
  await mount();
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeVisible();
  await frames();
  expect(calls("first_data_painted")).toHaveLength(1);
});
test("a traced palette is acknowledged after paint once, with explicit consumed and deferred outcomes", async () => {
  await mount();
  act(() => useNativeRuntime.getState().push({ kind: "palette", traceId: 7 }));
  expect(await screen.findByRole("combobox", { name: "명령 검색" })).toBeVisible();
  await waitFor(() =>
    expect(calls("palette_painted")).toEqual([
      ["palette_painted", { traceId: 7, outcome: "painted", deferred: false }],
    ]),
  );
  act(() => useNativeRuntime.getState().push({ kind: "palette", traceId: 8 }));
  await waitFor(() => expect(calls("palette_painted")).toHaveLength(2));
  expect(calls("palette_painted")[1]).toEqual([
    "palette_painted",
    { traceId: 8, outcome: "alreadyOpen", deferred: false },
  ]);
  expect(useNativeRuntime.getState().intents).toHaveLength(0);
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("combobox", { name: "명령 검색" })).not.toBeInTheDocument());
  act(() => useNativeRuntime.getState().push({ kind: "palette" }));
  expect(await screen.findByRole("combobox", { name: "명령 검색" })).toBeVisible();
  await frames();
  expect(calls("palette_painted")).toHaveLength(2);
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("combobox", { name: "명령 검색" })).not.toBeInTheDocument());
  const blocker = document.createElement("div");
  blocker.setAttribute("role", "dialog");
  document.body.append(blocker);
  act(() => useNativeRuntime.getState().push({ kind: "palette", traceId: 9 }));
  await frames();
  expect(calls("palette_painted")).toHaveLength(2);
  await act(async () => blocker.remove());
  expect(await screen.findByRole("combobox", { name: "명령 검색" })).toBeVisible();
  await waitFor(() => expect(calls("palette_painted")).toHaveLength(3));
  expect(calls("palette_painted")[2]).toEqual(["palette_painted", { traceId: 9, outcome: "painted", deferred: true }]);
});
