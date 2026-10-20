import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider } from "../../../ui";
import { ProjectWire } from "../../projects/test-wire";
import { runFixture } from "../../runs/test-wire";
import { task } from "../test-fixtures";
import { RunTab, useAgentRuns } from "./RunTab";

vi.mock("../../terminal/TerminalView", () => ({ TerminalView: () => <div>터미널</div> }));
vi.mock("../../runs/LogView", () => ({
  LogView: ({ sources }: { sources: unknown }) => <div>실행 로그 {JSON.stringify(sources)}</div>,
}));
beforeEach(() => useAgentRuns.setState({ runs: {} }));
async function setup(state: ReturnType<typeof task>["state"] = "running") {
  const wire = new ProjectWire({ trustNotRequired: true });
  useAgentRuns.getState().set("a1", "run-1");
  const node = (state: ReturnType<typeof task>["state"]) => (
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <RunTab task={task("a1", state)} />
      </TooltipProvider>
    </RpcProvider>
  );
  const view = render(node(state));
  await waitFor(() => expect(wire.requests("runtime.get")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.config", { config: { agent: { test: "true" } }, error: null });
    wire.reply("runtime.get", {
      rev: "1",
      run: { ...runFixture, kind: "agentTest", state: "failed", exit: { code: 1 } },
    });
  });
  await screen.findByText(/종료 코드 1/);
  return { wire, view, node };
}
test("agent test displays runtime logs and its runtime result", async () => {
  await setup();
  expect(screen.getByText(/실행 로그/)).toBeVisible();
  expect(screen.getByText(/실행 ID run-1 · 종료 코드 1/)).toBeVisible();
});
test("explicit failure button reads masked server logs and prevents duplicate clicks", async () => {
  const { wire } = await setup();
  const button = screen.getByRole("button", { name: "에이전트에게 실패 로그 보내기" });
  fireEvent.click(button);
  fireEvent.click(button);
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  expect(wire.request("logs.search").params).toMatchObject({
    sources: [{ kind: "run", runId: "run-1" }],
    query: "",
    limit: 200,
  });
  await act(async () =>
    wire.reply("logs.search", {
      items: [
        { src: { kind: "run", runId: "run-1" }, text: "masked [REDACTED]", ts: "2", stream: "stdout" },
        { src: { kind: "run", runId: "run-1" }, text: "older", ts: "1", stream: "stderr" },
      ],
      nextCursor: null,
    }),
  );
  await waitFor(() => expect(wire.requests("agents.send")).toHaveLength(1));
  expect(wire.request("agents.send").params).toMatchObject({
    id: "a1",
    text: expect.stringContaining("> older\n> masked [REDACTED]"),
  });
  fireEvent.click(button);
  expect(wire.requests("agents.send")).toHaveLength(1);
  await act(async () => wire.reply("agents.send", task("a1")));
  expect(await screen.findByText("실패 로그를 보냈습니다.")).toBeVisible();
  expect(button).toBeDisabled();
});
test("closing an agent during log read prevents send and removes actions", async () => {
  const { wire, view, node } = await setup();
  fireEvent.click(screen.getByRole("button", { name: "에이전트에게 실패 로그 보내기" }));
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  view.rerender(node("closed"));
  await act(async () => wire.reply("logs.search", { items: [], nextCursor: null }));
  expect(wire.requests("agents.send")).toHaveLength(0);
  expect(screen.queryByRole("button", { name: "에이전트에게 실패 로그 보내기" })).toBeNull();
  expect(screen.queryByRole("button", { name: "테스트 다시 실행" })).toBeNull();
});
test("failed send shows feedback; rerun returns a new runtime ID after a trust check", async () => {
  const { wire } = await setup();
  fireEvent.click(screen.getByRole("button", { name: "에이전트에게 실패 로그 보내기" }));
  await waitFor(() => expect(wire.requests("logs.search")).toHaveLength(1));
  await act(async () => wire.fail("logs.search"));
  expect(await screen.findByRole("status")).not.toHaveTextContent("보냈습니다");
  fireEvent.click(screen.getByRole("button", { name: "테스트 다시 실행" }));
  await waitFor(() => expect(wire.requests("agents.test")).toHaveLength(1));
  await act(async () => wire.reply("agents.test", { runId: "run-2" }));
  await waitFor(() => expect(wire.requests("runtime.get")).toHaveLength(2));
  expect(wire.request("runtime.get", -1).params).toEqual({ runId: "run-2" });
  expect(screen.getByText(/실행 로그/)).toHaveTextContent("run-2");
});
test("runtime.changed refreshes the selected result", async () => {
  const { wire } = await setup();
  const subscription = wire.sent.find(
    (message) => message.type === "subscribe" && message.topic === "runtime.changed",
  )!;
  await act(async () =>
    wire.deliver({
      type: "event",
      subId: subscription.id,
      topic: "runtime.changed",
      rev: "2",
      payload: { rev: "2", projectId: "a" },
    }),
  );
  await waitFor(() => expect(wire.requests("runtime.get")).toHaveLength(2));
  await act(async () =>
    wire.reply("runtime.get", {
      rev: "2",
      run: { ...runFixture, kind: "agentTest", state: "exited", exit: { code: 0 } },
    }),
  );
  expect(await screen.findByText(/종료 코드 0/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "에이전트에게 실패 로그 보내기" })).toBeNull();
});
