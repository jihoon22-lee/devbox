import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCurrentProject } from "../features/projects/store";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useToasts } from "../ui/Toast";
import { useCommands } from "./commands";
import { Shell } from "./Shell";

test("title, Ctrl+Shift+O and registry open the project switcher", async () => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const wire = new ProjectWire({ trustNotRequired: true });
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

test("launch selects project, navigates to terminal preserving search and keeps success after refresh failure", async () => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  window.history.replaceState({}, "", "/agents?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: [projects[0]] }));
  await userEvent.click(await screen.findByRole("button", { name: "프로젝트 전환" }));
  await waitFor(() => expect(wire.requests("projects.config")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.config", {
      config: { agent: {}, launch: { services: [], terminals: [{ name: "web", command: null }], agents: [] } },
    }),
  );
  await userEvent.click(await screen.findByRole("button", { name: "devbox 시작 구성 실행" }));
  await waitFor(() => expect(wire.requests("projects.launch")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.launch", { terminals: 1, agents: 0, terminalIds: ["launch-shell"], rev: "2" }),
  );
  await waitFor(() => expect(window.location.pathname).toBe("/terminal"));
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  expect(useCurrentProject.getState().id).toBe("a");
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.fail("terminal.list"));
  expect(useToasts.getState().items.at(-1)?.title).toBe("터미널 1개 · 에이전트 0개를 시작했습니다");
  expect(wire.requests("projects.launch")).toHaveLength(1);
  expect(screen.queryByRole("dialog", { name: "프로젝트" })).not.toBeInTheDocument();
});
