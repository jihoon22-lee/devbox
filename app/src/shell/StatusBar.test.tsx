import { focusManager } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useCurrentProject } from "../features/projects/store";
import { ProjectWire } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { StatusBar } from "./StatusBar";

const navigate = vi.hoisted(() => vi.fn());
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));
const methods = ["runtime.summary", "observe.ports", "observe.system"];
const gib = 1024 ** 3;
function replyStats(wire: ProjectWire, rev = "1") {
  wire.reply("runtime.summary", { rev, active: 205 });
  wire.reply(
    "observe.ports",
    [5173, 8080, 5173].map((port) => ({ port })),
  );
  wire.reply("observe.system", { memoryTotalBytes: 20 * gib, memoryAvailableBytes: 13.9 * gib });
}
afterEach(() => {
  useCurrentProject.setState({ id: undefined });
  focusManager.setFocused(undefined);
  vi.useRealTimers();
  vi.restoreAllMocks();
});
test("status queries start only visible and all stop polling while hidden or unmounted", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let visible = false;
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => (visible ? "visible" : "hidden"));
  focusManager.setFocused(false);
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <StatusBar />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  for (const method of methods) expect(wire.requests(method)).toHaveLength(0);
  await act(async () => {
    visible = true;
    fireEvent(document, new Event("visibilitychange"));
  });
  await waitFor(() => {
    for (const method of methods) expect(wire.requests(method)).toHaveLength(1);
  });
  await act(async () => replyStats(wire));
  expect(await screen.findByRole("button", { name: "실행 205" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "포트 5173, 8080" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "WSL 6.1/20GB" })).toBeInTheDocument();
  expect(wire.requests("runtime.list")).toHaveLength(0);
  expect(wire.requests("runtime.runs")).toHaveLength(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3100);
  });
  expect(wire.requests("runtime.summary")).toHaveLength(1);
  for (const method of methods.slice(1)) expect(wire.requests(method)).toHaveLength(2);
  const subscription = wire.sent.find(
    (message) => message.type === "subscribe" && message.topic === "runtime.changed",
  )!;
  await act(async () =>
    wire.deliver({ type: "event", subId: subscription.id, rev: "2", payload: { rev: "2", projectId: "p" } }),
  );
  await waitFor(() => expect(wire.requests("runtime.summary")).toHaveLength(2));
  await act(async () => replyStats(wire, "2"));
  await act(async () => {
    visible = false;
    fireEvent(document, new Event("visibilitychange"));
  });
  await act(async () => {
    wire.deliver({ type: "event", subId: subscription.id, rev: "3", payload: { rev: "3", projectId: "p" } });
    await vi.advanceTimersByTimeAsync(10000);
  });
  for (const method of methods) expect(wire.requests(method)).toHaveLength(2);
  await act(async () => {
    visible = true;
    fireEvent(document, new Event("visibilitychange"));
  });
  await waitFor(() => {
    for (const method of methods) expect(wire.requests(method)).toHaveLength(3);
  });
  await act(async () => replyStats(wire, "3"));
  view.rerender(
    <RpcProvider client={wire.client}>
      <div />
    </RpcProvider>,
  );
  await act(async () => {
    visible = true;
    fireEvent(document, new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(10000);
  });
  for (const method of methods) expect(wire.requests(method)).toHaveLength(3);
});

test("status buttons navigate to existing run and observation views", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <StatusBar />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.summary")).toHaveLength(1));
  await act(async () => replyStats(wire));
  fireEvent.click(await screen.findByRole("button", { name: "실행 205" }));
  expect(navigate).toHaveBeenLastCalledWith({ to: "/runs" });
  fireEvent.click(screen.getByRole("button", { name: "포트 5173, 8080" }));
  expect(navigate).toHaveBeenLastCalledWith({ to: "/runs", search: { view: "ports" } });
  fireEvent.click(screen.getByRole("button", { name: "WSL 6.1/20GB" }));
  expect(navigate).toHaveBeenLastCalledWith({ to: "/runs", search: { view: "system" } });
});

test("failed status queries do not masquerade as empty or zero values", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <StatusBar />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.summary")).toHaveLength(1));
  await act(async () => {
    for (const method of methods) wire.fail(method);
  });
  expect(await screen.findByRole("button", { name: "실행 조회 실패" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "포트 조회 실패" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "WSL 조회 실패" })).toBeInTheDocument();
});

test("Git chip shows current branch counts and isolates a late former-project status", async () => {
  useCurrentProject.setState({ id: "p" });
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <StatusBar />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  const old = wire.request("git.status").id;
  act(() => useCurrentProject.setState({ id: "q" }));
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(2));
  await act(async () => {
    wire.deliver({
      type: "response",
      id: old,
      result: { summary: { branch: "old", ahead: 99, behind: 99 }, files: [] },
    });
    wire.reply("git.status", { summary: { branch: "current", ahead: 2, behind: 3 }, files: [{}, {}] });
  });
  const chip = await screen.findByRole("button", { name: "Git 상태 · 소스로 이동" });
  expect(chip).toHaveTextContent("current ↑2 ↓3 · 변경 2");
  expect(chip).not.toHaveTextContent("old");
  fireEvent.click(chip);
  expect(navigate).toHaveBeenCalledWith({ to: "/source" });
});
