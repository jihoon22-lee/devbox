import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire, projects } from "../projects/test-wire";
import { NewTaskDialog } from "./NewTaskDialog";
import { profiles } from "./test-fixtures";

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
  localStorage.clear();
});

const antigravity = {
  id: "antigravity",
  label: "Antigravity",
  command: "agy",
  hooks: "antigravity" as const,
  waitingPatterns: [],
  promptArg: null,
};

/** The dialog with the Antigravity profile last used for the project, or Claude Code. */
async function dialog(saved: "antigravity" | "claude") {
  localStorage.setItem("devbox.newTask.lastProfile.a", saved);
  useCurrentProject.getState().set("a");
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <RpcProvider client={wire.client}>
      <NewTaskDialog open onOpenChange={vi.fn()} onCreated={vi.fn()} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.profiles")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: projects });
    wire.reply("agents.profiles", { items: [...profiles, antigravity] });
  });
  await waitFor(() => expect(wire.requests("projects.prompts")).toHaveLength(1));
  await act(async () => wire.reply("projects.prompts", { items: [] }));
  return wire;
}

/** Opens the tool list with the keyboard, as the dialog's other tests do. */
async function openTools() {
  act(() => screen.getByRole("combobox", { name: "도구" }).focus());
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
}

test("the Antigravity tool is offered by its label like the others", async () => {
  await dialog("claude");
  await openTools();
  const offered = (await screen.findAllByRole("option")).map((option) => option.textContent);
  expect(offered).toEqual(["Claude Code", "Codex", "Antigravity"]);
});

test("an Antigravity task always has a work folder, and its prompt is sent like any other", async () => {
  const wire = await dialog("antigravity");
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveTextContent("Antigravity");
  // The work folder cannot be turned off: devbox keeps the tool's connection beside it.
  const folder = screen.getByLabelText("작업 폴더 만들기");
  expect(folder).toBeChecked();
  expect(folder).toBeDisabled();
  expect(screen.getByText("Antigravity 작업에는 작업 폴더가 필요합니다.")).toBeInTheDocument();
  await userEvent.type(screen.getByLabelText("제목"), "Fix login");
  await userEvent.type(screen.getByLabelText("지시문"), "fix it");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({
    profile: "antigravity",
    worktree: true,
    prompt: "fix it",
  });
});

test("a work folder turned off for another tool is back on for Antigravity, and stays the user's choice elsewhere", async () => {
  const wire = await dialog("claude");
  const folder = screen.getByLabelText("작업 폴더 만들기");
  await userEvent.click(folder);
  expect(folder).not.toBeChecked();
  expect(screen.queryByText(/작업 폴더가 필요합니다/)).not.toBeInTheDocument();
  // Picking Antigravity turns it on and shows the branch name again.
  await openTools();
  await userEvent.keyboard("{End}{Enter}");
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveTextContent("Antigravity");
  expect(folder).toBeChecked();
  expect(folder).toBeDisabled();
  expect(screen.queryByText("현재 체크아웃에서 실행")).not.toBeInTheDocument();
  // Back on Claude Code, the user's own choice holds again.
  await openTools();
  await userEvent.keyboard("{Home}{Enter}");
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveTextContent("Claude Code");
  expect(folder).not.toBeChecked();
  expect(folder).toBeEnabled();
  await userEvent.type(screen.getByLabelText("제목"), "Fix login");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ profile: "claude", worktree: false });
});
