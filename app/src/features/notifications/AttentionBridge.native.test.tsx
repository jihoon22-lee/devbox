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
test("run failure deduplicates the event before both native toast and notification center", async () => {
  mocks.attention.mockReset();
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <AttentionBridge />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  const sub = wire.sent.find((message) => message.topic === "notify.raised");
  const emit = async (id: string, rev: string) =>
    act(async () =>
      wire.deliver({
        type: "event",
        subId: sub?.id ?? "absent-run-subscription",
        rev,
        payload: { id, projectId: "프로젝트/한글", runId: "run-1", task: "검사/test", reason: "startLimit" },
      }),
    );
  await emit("event-1", "1");
  await emit("event-1", "2");
  expect(useNotifications.getState().items.filter((item) => item.target?.search?.runId === "run-1")).toHaveLength(1);
  expect(mocks.attention).toHaveBeenCalledTimes(1);
  expect(mocks.attention.mock.lastCall?.[0]).toMatchObject({
    id: "event-1",
    target: { to: "/runs", search: { projectId: "프로젝트/한글", runId: "run-1", task: "검사/test" } },
  });
  await emit("event-2", "3");
  expect(mocks.attention).toHaveBeenCalledTimes(2);
  expect(useNotifications.getState().items).toHaveLength(2);
  act(() => useNotifications.getState().clear());
  await emit("event-2", "4");
  expect(mocks.attention).toHaveBeenCalledTimes(2);
  expect(useNotifications.getState().items).toHaveLength(0);
});
test("preflight and trust failures retain the exact project and task without inventing a run", async () => {
  mocks.attention.mockReset();
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <AttentionBridge />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  const sub = wire.sent.find((message) => message.topic === "notify.raised")!;
  for (const [index, reason] of ["schedule_failed", "schedule_trust_required"].entries()) {
    await act(async () =>
      wire.deliver({
        type: "event",
        subId: sub.id,
        rev: String(index + 1),
        payload: {
          id: `null-${index}`,
          projectId: "p/한글",
          runId: null,
          task: "테스트/한글",
          reason,
        },
      }),
    );
    expect(useNotifications.getState().items[0]?.target).toEqual({
      to: "/runs",
      search: { projectId: "p/한글", task: "테스트/한글" },
    });
    expect(mocks.attention.mock.lastCall?.[0].target.search).not.toHaveProperty("runId");
  }
  expect(mocks.attention).toHaveBeenCalledTimes(2);
  view.rerender(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>{null}</AgentDataProvider>
    </RpcProvider>,
  );
  expect(wire.sent).toContainEqual({ type: "unsubscribe", subId: sub.id });
});
