import { act, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { AgentDataProvider, useAgentData } from "./AgentData";
import { task } from "./test-fixtures";

function Snapshot() {
  const data = useAgentData();
  return (
    <output>
      {data.list.data?.rev ?? "pending"}:{data.tasks.map((item) => item.id).join(",")}
    </output>
  );
}

test("list pagination restarts revision drift and publishes only a consistent snapshot", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AgentDataProvider>
        <Snapshot />
      </AgentDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  await act(async () => wire.reply("agents.list", { rev: "1", items: [task("a", "idle")], nextAfterId: "a" }));
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(2));
  expect(wire.requests("agents.list")[1]!.params).toEqual({ includeClosedHours: 24, afterId: "a" });
  expect(screen.getByText("pending:")).toBeInTheDocument();
  await act(async () => wire.reply("agents.list", { rev: "2", items: [task("b", "idle")] }));
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(3));
  expect(wire.requests("agents.list")[2]!.params).toEqual({ includeClosedHours: 24 });
  await act(async () => wire.reply("agents.list", { rev: "2", items: [task("c", "idle")], nextAfterId: "c" }));
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(4));
  await act(async () => wire.reply("agents.list", { rev: "2", items: [task("d", "idle")] }));
  expect(await screen.findByText("2:c,d")).toBeInTheDocument();
  expect(screen.queryByText("2:a,b")).not.toBeInTheDocument();
});
