import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire, projects } from "../projects/test-wire";
import { NewTaskDialog } from "./NewTaskDialog";
import { profiles, task } from "./test-fixtures";

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});

async function dialog(items = projects, saved = "claude") {
  localStorage.setItem("devbox.lastProfile", saved);
  useCurrentProject.getState().set("a");
  const wire = new ProjectWire(),
    created = vi.fn(),
    changed = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <NewTaskDialog open onOpenChange={changed} onCreated={created} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.profiles")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items });
    wire.reply("agents.profiles", { items: profiles });
  });
  if (items.length) {
    await waitFor(() => expect(wire.requests("projects.prompts")).toHaveLength(1));
    expect(wire.request("projects.prompts").params).toEqual({ id: "a" });
    await act(async () =>
      wire.reply("projects.prompts", {
        items: [{ name: "bugfix", body: "버그: {{title}}\n{{branch}} {{project}} {{other}}" }],
      }),
    );
  }
  return { wire, created, changed };
}

test("snapshots chosen profile, project, template, branch and worktree; guards IME and rapid duplicate submits", async () => {
  const { wire, created, changed } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Fix login");
  act(() => screen.getByRole("combobox", { name: "도구" }).focus());
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{Escape}");
  expect(screen.getByRole("dialog", { name: "새 에이전트 작업" })).toBeInTheDocument();
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveFocus();
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{End}{Enter}");
  await userEvent.type(screen.getByLabelText("기준 브랜치"), "develop");
  await userEvent.click(screen.getByLabelText("작업 폴더 만들기"));
  await userEvent.click(screen.getByRole("button", { name: "템플릿: bugfix" }));
  expect(screen.getByLabelText("지시문")).toHaveValue("버그: Fix login\ndevelop devbox {{other}}");
  fireEvent.keyDown(screen.getByLabelText("지시문"), { key: "Enter", ctrlKey: true, isComposing: true });
  expect(wire.requests("agents.create")).toHaveLength(0);
  fireEvent.keyDown(screen.getByLabelText("지시문"), { key: "Enter", ctrlKey: true });
  fireEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toEqual({
    projectId: "a",
    title: "Fix login",
    profile: "codex",
    baseBranch: "develop",
    worktree: false,
    prompt: "버그: Fix login\ndevelop devbox {{other}}",
  });
  await userEvent.keyboard("{Escape}");
  expect(changed).not.toHaveBeenCalled();
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
  await waitFor(() => expect(created).toHaveBeenCalledWith("new"));
  expect(changed).toHaveBeenCalledWith(false);
  expect(localStorage.getItem("devbox.lastProfile")).toBe("codex");
});

test("requires a registered project and never sends null project", async () => {
  const { wire } = await dialog([]);
  await userEvent.type(screen.getByLabelText("제목"), "Test");
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  expect(screen.getByText("에이전트 작업을 만들 프로젝트를 등록해 주세요.")).toBeInTheDocument();
  expect(wire.requests("projects.prompts")).toHaveLength(0);
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("invalid saved profile falls back and a known failure starts a fresh request identity", async () => {
  const { wire } = await dialog(projects, "removed");
  await userEvent.type(screen.getByLabelText("제목"), "Retry");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  const first = wire.request("agents.create");
  expect((first.params as Record<string, unknown>).profile).toBe("claude");
  await act(async () => wire.deliver({ type: "response", id: first.id, error: { code: "agents.title_invalid" } }));
  expect(await screen.findByRole("alert")).toHaveTextContent("제목");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(2));
  expect(wire.request("agents.create", 1).clientRequestId).not.toBe(first.clientRequestId);
});

test("lost create response retains request identity after reconnect", async () => {
  const { wire, created } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Ambiguous");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  const first = wire.request("agents.create");
  await act(async () => wire.close());
  expect(await screen.findByRole("alert")).toHaveTextContent("실행 결과");
  await act(async () => wire.connect());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(2));
  expect(wire.request("agents.create", 1).clientRequestId).toBe(first.clientRequestId);
  await act(async () => wire.reply("agents.create", task("new", "preparing")));
  expect(created).toHaveBeenCalledWith("new");
});

test("Ctrl+Enter while choosing a tool selects it without launching the previous profile", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Choose tool");
  act(() => screen.getByRole("combobox", { name: "도구" }).focus());
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{End}{Control>}{Enter}{/Control}");
  expect(wire.requests("agents.create")).toHaveLength(0);
  expect(screen.getByRole("combobox", { name: "도구" })).toHaveTextContent("Codex");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect((wire.request("agents.create").params as Record<string, unknown>).profile).toBe("codex");
});

test("a removed project is revalidated on refreshed lists and blocks stale creation", async () => {
  const { wire } = await dialog();
  await userEvent.type(screen.getByLabelText("제목"), "Removed project");
  const sub = wire.sent.find((m) => m.topic === "projects.changed")!;
  await act(async () => wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(2));
  await act(async () => wire.reply("projects.list", { rev: "2", items: [projects[1]] }));
  expect(await screen.findByText("에이전트 작업을 만들 프로젝트를 선택해 주세요.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  expect(wire.requests("agents.create")).toHaveLength(0);
});
