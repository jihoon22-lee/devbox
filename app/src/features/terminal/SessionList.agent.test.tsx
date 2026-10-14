import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { TooltipProvider } from "../../ui";
import { ProjectWire } from "../projects/test-wire";
import { SessionList } from "./SessionList";

test("agent terminal menu directs lifecycle work to the agent page", async () => {
  const wire = new ProjectWire();
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <SessionList
          sessions={[
            {
              id: "owned-agent",
              projectId: "p",
              title: "에이전트 작업",
              cwd: "/fixture",
              command: "fixture",
              kind: "agent",
              alive: true,
              createdMs: 1,
            },
          ]}
          loading={false}
          failed={false}
          disabled={false}
          projectTitle="프로젝트"
          onRetry={vi.fn()}
          onOpen={vi.fn()}
          onNew={vi.fn()}
          onEnded={vi.fn()}
        />
      </RpcProvider>
    </TooltipProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "에이전트 작업 메뉴" }));
  const end = await screen.findByRole("menuitem", { name: "에이전트 화면에서 종료" });
  expect(end).toHaveAttribute("aria-disabled", "true");
  await userEvent.click(end);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(wire.requests("terminal.close")).toHaveLength(0);
});
