import { focusManager } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { AgentDataProvider, useAgentData } from "./AgentData";
import { useAgentsData } from "./hooks";
import { profiles, task } from "./test-fixtures";

function Summary() {
  const data = useAgentData();
  return (
    <output>
      {data.attention}:{data.hookMissing ? "missing" : "ok"}:
      {data.groups.some((g) => g.key === "closed") ? "closed" : "none"}
    </output>
  );
}
function Metrics() {
  useAgentsData();
  return null;
}
afterEach(() => {
  focusManager.setFocused(undefined);
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test("one shared list subscription serves consumers; initially hidden metrics never start and poll only visible", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let visible = false;
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => (visible ? "visible" : "hidden"));
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <Summary />
        <Summary />
        <Metrics />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  expect(wire.sent.filter((m) => m.topic === "agents.changed")).toHaveLength(1);
  expect(wire.requests("agents.resources")).toHaveLength(0);
  expect(wire.requests("agents.git_stats")).toHaveLength(0);
  await act(async () => {
    wire.reply("agents.list", { rev: "1", items: [task("w", "waiting")] });
    wire.reply("agents.profiles", { items: profiles });
  });
  expect(await screen.findAllByText("1:ok:none")).toHaveLength(2);
  focusManager.setFocused(false);
  await act(async () => {
    visible = true;
    fireEvent(document, new Event("visibilitychange"));
  });
  await waitFor(() => expect(wire.requests("agents.resources")).toHaveLength(1));
  await act(async () => {
    wire.reply("agents.resources", { items: [] });
    wire.reply("agents.git_stats", { items: [] });
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3100);
  });
  expect(wire.requests("agents.resources")).toHaveLength(2);
  expect(wire.requests("agents.git_stats")).toHaveLength(1);
  await act(async () => wire.reply("agents.resources", { items: [] }));
  await act(async () => {
    visible = false;
    fireEvent(document, new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(11000);
  });
  expect(wire.requests("agents.resources")).toHaveLength(2);
  expect(wire.requests("agents.git_stats")).toHaveLength(1);
  expect(wire.requests("agents.list")).toHaveLength(1);
});

test("unchanged list crosses missing-hook and closed-age thresholds on visible time", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <Summary />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  const now = Date.now();
  await act(async () => {
    wire.reply("agents.list", {
      rev: "1",
      items: [
        { ...task("r", "running", now - 89000), sessionId: null },
        task("c", "closed", now - 24 * 3600_000 + 2000),
      ],
    });
    wire.reply("agents.profiles", { items: profiles });
  });
  expect(await screen.findByText("0:ok:closed")).toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4000);
  });
  expect(screen.getByText("0:missing:none")).toBeInTheDocument();
  expect(wire.requests("agents.list")).toHaveLength(1);
});
