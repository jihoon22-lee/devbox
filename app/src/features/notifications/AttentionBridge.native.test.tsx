import { act, cleanup, render, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { AgentDataProvider } from "../agents/AgentData";
import { task } from "../agents/test-fixtures";
import { ProjectWire } from "../projects/test-wire";
import { AttentionBridge } from "./AttentionBridge";
import { useNotifications } from "./store";

const mocks = vi.hoisted(() => ({ attention: vi.fn(), badge: vi.fn() }));
vi.mock("./native", () => ({ nativeAttention: mocks.attention, nativeBadge: mocks.badge }));
afterEach(() => {
  cleanup();
  useNotifications.getState().clear();
  vi.restoreAllMocks();
});
test("attention uses current visible list detail and task counts, and hidden grid IDs never suppress toasts", async () => {
  mocks.attention.mockReset();
  mocks.badge.mockReset();
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const wire = new ProjectWire();
  let location = { pathname: "/agents", search: { id: "a", view: "grid" } };
  render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <AttentionBridge location={() => location} />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  await act(async () => wire.reply("agents.list", { rev: "1", items: [task("a", "waiting"), task("b", "closed")] }));
  await waitFor(() => expect(mocks.badge).toHaveBeenCalledWith(1, 1));
  const sub = wire.sent.find((message) => message.topic === "agents.attention")!;
  let revision = 1;
  const attention = async (id = "a") =>
    act(async () =>
      wire.deliver({
        type: "event",
        subId: sub.id,
        rev: String(++revision),
        payload: { id, state: "waiting", message: "입력 확인" },
      }),
    );
  await attention();
  expect(mocks.attention).toHaveBeenLastCalledWith({
    id: "a",
    title: "작업 a · 입력 대기",
    body: "입력 확인",
    visible: false,
  });
  location = { pathname: "/agents", search: { id: "a", view: "list" } };
  await attention();
  expect(mocks.attention.mock.lastCall?.[0].visible).toBe(true);
  await attention("missing");
  expect(mocks.attention.mock.lastCall?.[0].visible).toBe(false);
  location = { pathname: "/settings", search: { id: "a", view: "list" } };
  await attention();
  expect(mocks.attention.mock.lastCall?.[0].visible).toBe(false);
  location = { pathname: "/agents", search: { id: "a", view: "list" } };
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  await attention();
  expect(mocks.attention.mock.lastCall?.[0].visible).toBe(false);
  expect(mocks.attention).toHaveBeenCalledTimes(5);
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(1);
});
test("startup and unmount badge zeros never become a proven empty task snapshot", async () => {
  const { useNativeRuntime } = await import("../../shell/nativeRuntime");
  useNativeRuntime.setState({ openTasks: null, snapshotCurrent: false });
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <AttentionBridge />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  expect(useNativeRuntime.getState().openTasks).toBeNull();
  await act(async () => wire.reply("agents.list", { rev: "1", items: [task("retained", "running")] }));
  await waitFor(() => expect(useNativeRuntime.getState().openTasks).toBe(1));
  view.unmount();
  expect(useNativeRuntime.getState().openTasks).toBe(1);
  expect(useNativeRuntime.getState().snapshotCurrent).toBe(false);
});
