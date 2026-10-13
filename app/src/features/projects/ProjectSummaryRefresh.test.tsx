import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectSwitcher } from "./ProjectSwitcher";
import { ProjectWire, projects } from "./test-wire";

function Harness() {
  const [open, setOpen] = useState(true);
  return <ProjectSwitcher open={open} onOpenChange={setOpen} />;
}
const summary = (terminals: number, attention: number) => ({
  items: [
    {
      id: "a",
      branch: "main",
      dirty: false,
      ahead: 0,
      behind: 0,
      terminals,
      agents: { open: 1, attention },
      error: null,
    },
  ],
});
test("open project cards refresh live counts and stop polling when closed", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Harness />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.summary")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: [projects[0]] });
    wire.reply("projects.summary", summary(0, 0));
  });
  expect(await screen.findByText("터미널 0개")).toBeInTheDocument();
  await waitFor(() => expect(wire.requests("projects.summary")).toHaveLength(2), { timeout: 3000 });
  await act(async () => wire.reply("projects.summary", summary(1, 1)));
  expect(await screen.findByText("터미널 1개")).toBeInTheDocument();
  expect(screen.getByText("에이전트 1 · 입력 대기 1")).toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "프로젝트" })).not.toBeInTheDocument());
  const count = wire.requests("projects.summary").length;
  await act(async () => new Promise((resolve) => setTimeout(resolve, 2300)));
  expect(wire.requests("projects.summary")).toHaveLength(count);
}, 10000);
