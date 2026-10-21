import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { BottomLogs } from "./BottomLogs";
import { usePinnedLogSources } from "./pinnedSources";
import { runFixture } from "./test-wire";

beforeEach(() => {
  useCurrentProject.setState({ id: "p" });
  usePinnedLogSources.setState({ byProject: {} });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(320);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
});
function setup() {
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <BottomLogs />
    </RpcProvider>,
  );
  return { wire, view };
}
async function replyRecent(wire: ProjectWire, id = "run-1") {
  await waitFor(() => expect(wire.requests("runtime.runs").length).toBeGreaterThan(0));
  await act(async () => wire.reply("runtime.runs", { rev: "1", items: [{ ...runFixture, id }] }));
  await waitFor(() => expect(wire.requests("logs.open").length).toBeGreaterThan(0));
}

test("shows the current project's latest run, and pins the edited sources", async () => {
  const { wire } = setup();
  await replyRecent(wire);
  expect(wire.request("runtime.runs").params).toEqual({ projectId: "p", limit: 1 });
  expect(wire.request("logs.open").params).toMatchObject({ sources: [{ kind: "run", runId: "run-1" }] });
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
  fireEvent.click(screen.getByRole("button", { name: "소스 추가" }));
  fireEvent.change(screen.getByLabelText("소스 종류"), { target: { value: "file" } });
  fireEvent.change(screen.getByLabelText("소스 경로 또는 ID"), { target: { value: "/owned/test.log" } });
  fireEvent.click(screen.getByRole("button", { name: "추가" }));
  fireEvent.click(screen.getByRole("button", { name: "하단 로그에 고정" }));
  expect(usePinnedLogSources.getState().byProject.p).toEqual([
    { kind: "run", runId: "run-1" },
    { kind: "file", path: "/owned/test.log" },
  ]);
  expect(screen.getByRole("button", { name: "고정 해제" })).toBeInTheDocument();
});

test("project changes close old streams, isolate pins, and closing the panel disposes the current stream", async () => {
  usePinnedLogSources.getState().set("p", [{ kind: "file", path: "/p.log" }]);
  usePinnedLogSources.getState().set("q", [{ kind: "container", id: "q-container" }]);
  const { wire, view } = setup();
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  expect(wire.requests("runtime.runs")).toHaveLength(0);
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
  act(() => useCurrentProject.setState({ id: "q" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(2));
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 7)).toHaveLength(1);
  expect(wire.request("logs.open", -1).params).toMatchObject({ sources: [{ kind: "container", id: "q-container" }] });
  await act(async () => wire.reply("logs.open", { streamId: 8 }));
  view.rerender(
    <RpcProvider client={wire.client}>
      <div />
    </RpcProvider>,
  );
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 8)).toHaveLength(1);
});

test("unpin restores latest-run fallback and retains the other project's pins", async () => {
  usePinnedLogSources.getState().set("p", [{ kind: "file", path: "/p.log" }]);
  usePinnedLogSources.getState().set("q", [{ kind: "run", runId: "q" }]);
  const { wire } = setup();
  fireEvent.click(screen.getByRole("button", { name: "고정 해제" }));
  await replyRecent(wire);
  expect(usePinnedLogSources.getState().byProject.p).toBeUndefined();
  expect(usePinnedLogSources.getState().byProject.q).toEqual([{ kind: "run", runId: "q" }]);
  expect(await screen.findByText("run:run-1")).toBeInTheDocument();
});

test("latest-run errors retry and an empty project allows adding a source", async () => {
  const { wire } = setup();
  await waitFor(() => expect(wire.requests("runtime.runs")).toHaveLength(1));
  await act(async () => wire.fail("runtime.runs"));
  expect(await screen.findByRole("alert")).toHaveTextContent("최근 실행을 불러오지 못했습니다");
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("runtime.runs")).toHaveLength(2));
  await act(async () => wire.reply("runtime.runs", { rev: "1", items: [] }));
  expect(await screen.findByRole("button", { name: "소스 추가" })).toBeInTheDocument();
  expect(wire.requests("logs.open")).toHaveLength(0);
});

test("no project does not query or open logs", async () => {
  useCurrentProject.setState({ id: undefined });
  const { wire } = setup();
  expect(screen.getByText("프로젝트를 선택하세요")).toBeInTheDocument();
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(wire.requests("runtime.runs")).toHaveLength(0);
  expect(wire.requests("logs.open")).toHaveLength(0);
});

test("late stream-open responses after project changes are closed and pinned failures remain retryable", async () => {
  usePinnedLogSources.getState().set("p", [{ kind: "file", path: "/p.log" }]);
  usePinnedLogSources.getState().set("q", [{ kind: "file", path: "/q.log" }]);
  const { wire } = setup();
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(1));
  const old = wire.request("logs.open");
  act(() => useCurrentProject.setState({ id: "q" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(2));
  await act(async () => wire.deliver({ type: "response", id: old.id, result: { streamId: 70 } }));
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 70)).toHaveLength(1);
  await act(async () => wire.fail("logs.open"));
  expect(await screen.findByRole("alert")).toHaveTextContent("로그를 열지 못했습니다");
  expect(usePinnedLogSources.getState().byProject.q).toEqual([{ kind: "file", path: "/q.log" }]);
  fireEvent.click(screen.getByRole("button", { name: "다시 연결" }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(3));
  expect(wire.request("logs.open", -1).params).toMatchObject({ sources: [{ kind: "file", path: "/q.log" }] });
});

test("runtime changes replace the recent fallback and close the previous stream", async () => {
  const { wire } = setup();
  await replyRecent(wire);
  await act(async () => wire.reply("logs.open", { streamId: 7 }));
  const subscription = wire.sent.find(
    (message) => message.type === "subscribe" && message.topic === "runtime.changed",
  )!;
  await act(async () =>
    wire.deliver({ type: "event", subId: subscription.id, rev: "2", payload: { rev: "2", projectId: "p" } }),
  );
  await waitFor(() => expect(wire.requests("runtime.runs")).toHaveLength(2));
  await act(async () => wire.reply("runtime.runs", { rev: "2", items: [{ ...runFixture, id: "new-run" }] }));
  await waitFor(() => expect(wire.requests("logs.open")).toHaveLength(2));
  expect(wire.sent.filter((m) => m.type === "streamClose" && m.stream === 7)).toHaveLength(1);
  expect(wire.request("logs.open", -1).params).toMatchObject({ sources: [{ kind: "run", runId: "new-run" }] });
});
