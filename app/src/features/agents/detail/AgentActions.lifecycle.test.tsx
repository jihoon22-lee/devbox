import { render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider } from "../../../ui";
import { ProjectWire } from "../../projects/test-wire";
import { task } from "../test-fixtures";
import { AgentActions } from "./AgentActions";

test("an ended tool offers resume while its terminal remains alive", async () => {
  const wire = new ProjectWire();
  const ended = { ...task("a1", "review"), toolExited: true };
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <AgentActions task={ended} sessionAlive />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(screen.getByRole("button", { name: "재개" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: "이어서 지시" })).toBeNull();
});
