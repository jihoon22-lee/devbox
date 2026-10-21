import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RpcProvider, useRpcSession } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { SystemOverview } from "./SystemOverview";

const mocks = vi.hoisted(() => ({ navigate: vi.fn(), open: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => mocks.navigate }));
vi.mock("../../shell/external", () => ({ openExternal: mocks.open }));
const port = {
  port: 8080,
  proto: "tcp",
  addr: "::",
  pid: 123,
  process: "owned",
  startTimeMs: 100,
  projectId: "p",
  runId: null,
  containerId: null,
};
function setup(view: "ports" | "processes" = "ports") {
  const wire = new ProjectWire();
  function CacheProbe() {
    const { qc } = useRpcSession();
    return (
      <button type="button" onClick={() => qc.setQueryData(["observe.ports", {}], [{ ...port, startTimeMs: 150 }])}>
        테스트 갱신
      </button>
    );
  }
  render(
    <RpcProvider client={wire.client}>
      <CacheProbe />
      <SystemOverview view={view} projectId="p" />
    </RpcProvider>,
  );
  return wire;
}
test("ports open IPv6 loopback and require frozen identity confirmation even after refresh", async () => {
  const wire = setup();
  await waitFor(() => expect(wire.requests("observe.ports")).toHaveLength(1));
  await act(async () => wire.reply("observe.ports", [port]));
  fireEvent.click(await screen.findByRole("button", { name: "포트 8080 브라우저로 열기" }));
  expect(mocks.open).toHaveBeenCalledWith("http://[::1]:8080");
  fireEvent.click(screen.getByRole("button", { name: "PID 123 종료" }));
  expect(wire.requests("observe.kill")).toHaveLength(0);
  const dialog = screen.getByRole("dialog");
  expect(dialog).toHaveTextContent("PID 123");
  expect(dialog).toHaveTextContent("시작 시각");
  fireEvent.click(screen.getByRole("button", { name: "테스트 갱신", hidden: true }));
  expect(dialog).toHaveTextContent("100ms");
  fireEvent.click(within(dialog).getByRole("button", { name: "종료" }));
  await waitFor(() => expect(wire.requests("observe.kill")).toHaveLength(1));
  expect(wire.request("observe.kill").params).toEqual({ pid: 123, signal: "term", expect: { startTimeMs: 100 } });
  await act(async () =>
    wire.deliver({ type: "response", id: wire.request("observe.kill").id, error: { code: "observe.state_changed" } }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("다시 확인");
  await waitFor(() => expect(wire.requests("observe.ports")).toHaveLength(2));
  expect(screen.getByRole("button", { name: "PID 123 종료" })).toBeDisabled();
  await act(async () => wire.reply("observe.ports", [{ ...port, startTimeMs: 200 }]));
  await waitFor(() => expect(screen.getByRole("button", { name: "PID 123 종료" })).toBeEnabled());
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(wire.requests("observe.kill")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "PID 123 종료" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "종료" }));
  await waitFor(() => expect(wire.requests("observe.kill")).toHaveLength(2));
  expect(wire.request("observe.kill", -1).params).toMatchObject({ expect: { startTimeMs: 200 } });
  await act(async () => wire.reply("observe.kill", { pid: 123 }));
});
test("unknown identities and container owners cannot be terminated and UDP cannot open browser", async () => {
  const wire = setup();
  await waitFor(() => expect(wire.requests("observe.ports")).toHaveLength(1));
  await act(async () =>
    wire.reply("observe.ports", [
      { ...port, startTimeMs: null },
      { ...port, port: 8081, containerId: "container", proto: "udp" },
      { ...port, port: 8082, pid: null, startTimeMs: null },
    ]),
  );
  await screen.findByText("8082");
  for (const button of screen.getAllByRole("button", { name: /종료/ })) expect(button).toBeDisabled();
  expect(screen.getByRole("button", { name: "포트 8081 브라우저로 열기" })).toBeDisabled();
  expect(wire.requests("observe.processes")).toHaveLength(0);
});
test("process view requests current project, preserves priority order, cancels and confirms termination", async () => {
  const wire = setup("processes");
  await waitFor(() => expect(wire.requests("observe.processes")).toHaveLength(1));
  expect(wire.request("observe.processes").params).toEqual({ projectId: "p" });
  const process = {
    pid: 789,
    name: "project-worker",
    uid: 1000,
    cwd: "/project",
    startTimeMs: 1234,
    cpuPercent: 1,
    rssBytes: 1048576,
    unit: null,
    runId: null,
    projectId: "p",
  };
  await act(async () =>
    wire.reply("observe.processes", [
      process,
      { ...process, pid: 999, name: "other", cpuPercent: 99, projectId: null },
    ]),
  );
  expect((await screen.findAllByRole("row"))[1]).toHaveTextContent("project-worker");
  expect(wire.requests("observe.ports")).toHaveLength(0);
  expect(wire.requests("observe.system")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "PID 789 종료" }));
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(wire.requests("observe.kill")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "PID 789 종료" }));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "종료" }));
  await waitFor(() => expect(wire.requests("observe.kill")).toHaveLength(1));
  expect(wire.request("observe.kill").params).toEqual({ pid: 789, signal: "term", expect: { startTimeMs: 1234 } });
  await act(async () => wire.reply("observe.kill", { pid: 789 }));
});
