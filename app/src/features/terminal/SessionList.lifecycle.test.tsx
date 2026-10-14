import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { TooltipProvider } from "../../ui";
import { ProjectWire } from "../projects/test-wire";
import { SessionList } from "./SessionList";

test("an exited terminal still exposes record cleanup", async () => {
  const wire = new ProjectWire();
  render(
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <SessionList
          sessions={[
            {
              id: "dead",
              projectId: "p",
              title: "끝난 작업",
              cwd: "/fixture",
              command: "bash",
              kind: "shell",
              alive: false,
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
  await userEvent.click(screen.getByRole("button", { name: "끝난 작업 메뉴" }));
  expect(await screen.findByRole("menuitem", { name: "세션 끝내기" })).not.toHaveAttribute("aria-disabled", "true");
  await userEvent.click(screen.getByRole("menuitem", { name: "세션 끝내기" }));
  await userEvent.click(await screen.findByRole("button", { name: "세션 끝내기" }));
  await waitFor(() => expect(wire.requests("terminal.close")).toHaveLength(1));
});
