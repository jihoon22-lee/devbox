import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire, projects } from "../projects/test-wire";
import { NewTaskDialog } from "./NewTaskDialog";
import { profiles } from "./test-fixtures";
import { type AgentDraft, useAgentsUi } from "./ui";

beforeEach(() => {
  localStorage.clear();
  useCurrentProject.getState().set("a");
});
afterEach(() => useAgentsUi.getState().setCreating(false));

/** Opens the form the way the app does: the draft is in place before the form mounts, the lists arrive after. */
async function open(draft: AgentDraft) {
  useAgentsUi.getState().setCreating(true, draft);
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <RpcProvider client={wire.client}>
      <NewTaskDialog open onOpenChange={vi.fn()} onCreated={vi.fn()} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.profiles")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: projects });
    wire.reply("agents.profiles", { items: profiles });
  });
  return wire;
}
// The select shows its value's name a tick after its options arrive.
const project = () => screen.getByRole("combobox", { name: "프로젝트" });
const CHOOSE = "에이전트 작업을 만들 프로젝트를 선택해 주세요.";

test("a draft's title, project, base branch and work folder choice fill the form, and only [시작] starts it", async () => {
  const wire = await open({
    prompt: "원래 지시",
    title: "Same again",
    projectId: "b",
    baseBranch: "develop",
    worktree: false,
  });
  expect(screen.getByLabelText("제목")).toHaveValue("Same again");
  await waitFor(() => expect(project()).toHaveTextContent("family"));
  expect(screen.getByLabelText("기준 브랜치")).toHaveValue("develop");
  expect(screen.getByLabelText("작업 폴더 만들기")).not.toBeChecked();
  expect(screen.getByLabelText("지시문")).toHaveValue("원래 지시");
  await waitFor(() => expect(wire.requests("projects.prompts")).toHaveLength(1));
  expect(wire.request("projects.prompts").params).toEqual({ id: "b" });
  await act(async () => wire.reply("projects.prompts", { items: [] }));
  expect(wire.requests("agents.create")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toEqual({
    projectId: "b",
    title: "Same again",
    profile: "claude",
    baseBranch: "develop",
    worktree: false,
    prompt: "원래 지시",
  });
});

test("a draft whose project was removed since opens on the usual default project", async () => {
  const wire = await open({ prompt: "원래 지시", projectId: "gone" });
  await waitFor(() => expect(project()).toHaveTextContent("devbox"));
  expect(screen.queryByText(CHOOSE)).toBeNull();
  await waitFor(() => expect(wire.requests("projects.prompts")).toHaveLength(1));
  expect(wire.request("projects.prompts").params).toEqual({ id: "a" });
  await act(async () => wire.reply("projects.prompts", { items: [] }));
  await userEvent.type(screen.getByLabelText("제목"), "Fallback");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  expect(wire.request("agents.create").params).toMatchObject({ projectId: "a", prompt: "원래 지시" });
});

test("a draft's project removed while the form is open blocks the start instead of moving to another project", async () => {
  const wire = await open({ prompt: "원래 지시", projectId: "b" });
  await waitFor(() => expect(project()).toHaveTextContent("family"));
  await userEvent.type(screen.getByLabelText("제목"), "Removed meanwhile");
  const sub = wire.sent.find((message) => message.topic === "projects.changed")!;
  await act(async () => wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(2));
  // Only the current project is left; the form must not quietly switch to it.
  await act(async () => wire.reply("projects.list", { rev: "2", items: [projects[0]] }));
  expect(await screen.findByText(CHOOSE)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  expect(wire.requests("agents.create")).toHaveLength(0);
});
