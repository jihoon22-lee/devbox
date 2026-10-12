import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useCommands } from "./commands";
import { Shell } from "./Shell";
test("title, Ctrl+Shift+O and registry open the project switcher", async () => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
  await userEvent.click(await screen.findByRole("button", { name: "프로젝트 전환" }));
  expect(screen.getByRole("dialog", { name: "프로젝트" })).toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  fireEvent.keyDown(window, { code: "KeyO", key: "O", ctrlKey: true, shiftKey: true });
  expect(screen.getByRole("dialog", { name: "프로젝트" })).toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  const command = useCommands
    .getState()
    .list()
    .find((item) => item.label === "프로젝트 전환")!;
  expect(command.keys).toEqual(["Ctrl", "Shift", "O"]);
  act(() => command.run());
  expect(screen.getByRole("combobox", { name: "프로젝트 검색" })).toBeInTheDocument();
});
