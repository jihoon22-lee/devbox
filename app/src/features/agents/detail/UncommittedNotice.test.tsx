import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../../rpc/RpcProvider";
import { TooltipProvider } from "../../../ui";
import { ProjectDataProvider } from "../../projects/ProjectData";
import { ProjectWire } from "../../projects/test-wire";
import { AgentDataProvider } from "../AgentData";
import { useAgentsData } from "../hooks";
import { task } from "../test-fixtures";
import { AgentActions } from "./AgentActions";
import { AgentDetail } from "./AgentDetail";
import { useDrafts } from "./drafts";

vi.mock("../../terminal/TerminalView", () => ({ TerminalView: () => <div data-testid="terminal" /> }));

type Task = ReturnType<typeof task>;
const NOTICE = (count: number) => `커밋하지 않은 변경 ${count}개가 있어 병합·PR을 진행할 수 없습니다.`;
const INSTRUCTION = (count: number) =>
  `작업 폴더에 커밋하지 않은 변경이 ${count}개 있습니다. 변경을 확인하고 알맞은 메시지로 모두 커밋해 주세요.`;
const SHOWN = /커밋하지 않은 변경|변경 없음|변경을 확인하지 못했습니다/;
const status = (count: number) => ({
  expect: {
    state: { branch: "agent/a1", headOid: "h1", upstream: null, upstreamOid: null },
    indexHash: "i",
    worktreeHash: "w",
  },
  summary: {
    branch: "agent/a1",
    ahead: 0,
    behind: 0,
    staged: 0,
    unstaged: count,
    untracked: 0,
    conflicted: [],
    clean: count === 0,
  },
  files: Array.from({ length: count }, (_, index) => ({
    path: `file${index}.txt`,
    oldPath: null,
    indexStatus: ".",
    worktreeStatus: "M",
    conflicted: false,
  })),
  warnings: [],
});
beforeEach(() => useDrafts.setState({ text: {} }));
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function Detail() {
  const data = useAgentsData();
  return <AgentDetail id="a1" data={data} />;
}
/** The whole detail view: the notice's button has to reach the composer below it. */
async function showDetail(state: Task["state"]) {
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <AgentDataProvider>
          <ProjectDataProvider>
            <Detail />
          </ProjectDataProvider>
        </AgentDataProvider>
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.get")).toHaveLength(1));
  await act(async () => wire.reply("agents.get", task("a1", state)));
  await waitFor(() => expect(wire.requests("terminal.list").length).toBeGreaterThan(0));
  await act(async () =>
    wire.reply("terminal.list", {
      rev: "1",
      items: [
        {
          id: "t",
          projectId: "a",
          title: "agent",
          cwd: "/fixture",
          kind: "agent",
          alive: true,
          command: "x",
          createdMs: 1,
        },
      ],
    }),
  );
  const box = await screen.findByLabelText("에이전트에게 보낼 지시");
  await waitFor(() => expect(box).toBeEnabled());
  return { wire, box };
}
async function renderActions(item: Task, onInstruct = vi.fn()) {
  const wire = new ProjectWire({ trustNotRequired: true });
  const node = (next: Task) => (
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <AgentActions task={next} sessionAlive onInstruct={onInstruct} />
      </RpcProvider>
    </TooltipProvider>
  );
  const view = render(node(item));
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  return { wire, rerender: (next: Task) => view.rerender(node(next)) };
}
const noticeOf = async (text: string) => (await screen.findByText(text)).closest<HTMLElement>('[role="status"]')!;

test("shows_task_worktree_changes_and_prepares_commit_instruction_without_sending", async () => {
  const { wire, box } = await showDetail("idle");
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  // The task's own folder, as conflict resolution reads it: the daemon finds the folder from the task.
  expect(wire.request("git.status").params).toEqual({ target: { projectId: "a", worktreeId: "a1" } });
  await act(async () => wire.reply("git.status", status(2)));
  const notice = await noticeOf(NOTICE(2));
  await userEvent.click(within(notice).getByRole("button", { name: "커밋 지시 작성" }));
  expect(box).toHaveValue(INSTRUCTION(2));
  expect(box).toHaveFocus();
  expect(useDrafts.getState().text.a1).toBe(INSTRUCTION(2));
  // A draft only: the user reads it and sends it.
  expect(wire.requests("agents.send")).toHaveLength(0);
  expect(screen.getByRole("button", { name: "병합" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "PR" })).toBeEnabled();
});

test("status를 읽지 못하면 '변경 없음'으로 보이지 않는다", async () => {
  const { wire } = await renderActions(task("a1", "idle"));
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  // Still reading: neither a count nor a clean folder.
  expect(screen.queryByText(SHOWN)).toBeNull();
  await act(async () => wire.fail("git.status"));
  const notice = await noticeOf("변경을 확인하지 못했습니다");
  expect(within(notice).queryByRole("button")).toBeNull();
  expect(screen.queryByText(/커밋하지 않은 변경|변경 없음/)).toBeNull();
  expect(screen.queryByRole("button", { name: "커밋 지시 작성" })).toBeNull();
});

test("[커밋 지시 작성] adds the instruction after the draft, and not again while the draft ends with it", async () => {
  useDrafts.getState().set("a1", "먼저 테스트를 돌려 주세요");
  const onInstruct = vi.fn();
  const { wire } = await renderActions(task("a1", "idle"), onInstruct);
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  await act(async () => wire.reply("git.status", status(2)));
  const write = await screen.findByRole("button", { name: "커밋 지시 작성" });
  await userEvent.click(write);
  await userEvent.click(write);
  expect(useDrafts.getState().text.a1).toBe(`먼저 테스트를 돌려 주세요\n${INSTRUCTION(2)}`);
  act(() => useDrafts.getState().set("a1", `${INSTRUCTION(2)}\n`));
  await userEvent.click(write);
  expect(useDrafts.getState().text.a1).toBe(`${INSTRUCTION(2)}\n`);
  // Each press still takes the user to the composer.
  expect(onInstruct).toHaveBeenCalledTimes(3);
  expect(wire.requests("agents.send")).toHaveLength(0);
});

test.each(["waiting", "idle", "review", "failed"] as const)(
  "an open task in the %s state names its uncommitted changes",
  async (state) => {
    const { wire } = await renderActions(task("a1", state));
    await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
    await act(async () => wire.reply("git.status", status(1)));
    expect(await noticeOf(NOTICE(1))).toBeInTheDocument();
  },
);

test.each([
  ["running", task("a1", "running")],
  ["preparing", task("a1", "preparing")],
  ["closed", task("a1", "closed")],
  ["without a folder", { ...task("a1", "idle"), worktreePath: null, branch: null }],
])("a task %s reads no folder and shows no notice", async (_name, item) => {
  const { wire } = await renderActions(item);
  // The project's settings are read in the same pass as the folder would be.
  if (item.state !== "closed") await waitFor(() => expect(wire.requests("projects.config")).toHaveLength(1));
  await act(async () => {});
  expect(wire.requests("git.status")).toHaveLength(0);
  expect(screen.queryByText(SHOWN)).toBeNull();
});

test("a clean folder shows nothing", async () => {
  const { wire } = await renderActions(task("a1", "idle"));
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  await act(async () => wire.reply("git.status", status(0)));
  expect(screen.queryByText(SHOWN)).toBeNull();
  expect(screen.queryByRole("button", { name: "커밋 지시 작성" })).toBeNull();
});

test("a turn that ends reads the folder again and shows no earlier count meanwhile", async () => {
  const item = task("a1", "idle");
  const { wire, rerender } = await renderActions(item);
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  await act(async () => wire.reply("git.status", status(2)));
  expect(await noticeOf(NOTICE(2))).toBeInTheDocument();
  // Mid-turn changes are normal: nothing is read or shown.
  rerender({ ...item, state: "running", activitySeq: 1 });
  expect(screen.queryByText(SHOWN)).toBeNull();
  await act(async () => {});
  expect(wire.requests("git.status")).toHaveLength(1);
  // The count from before the turn is no longer known to be true.
  rerender({ ...item, state: "idle", activitySeq: 2 });
  expect(screen.queryByText(SHOWN)).toBeNull();
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(2));
  await act(async () => wire.reply("git.status", status(0)));
  expect(screen.queryByText(SHOWN)).toBeNull();
  // A task that changes without leaving the state reads again too, once.
  rerender({ ...item, state: "idle", activitySeq: 3 });
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(3));
  await act(async () => wire.reply("git.status", status(1)));
  expect(await noticeOf(NOTICE(1))).toBeInTheDocument();
  expect(wire.requests("git.status")).toHaveLength(3);
});

test("an answer to a read started before the task changed is not shown", async () => {
  const item = task("a1", "idle");
  const { wire, rerender } = await renderActions(item);
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  const old = wire.request("git.status").id;
  rerender({ ...item, state: "waiting", activitySeq: 1 });
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(2));
  await act(async () => wire.deliver({ type: "response", id: old, result: status(3) }));
  expect(screen.queryByText(SHOWN)).toBeNull();
  await act(async () => wire.reply("git.status", status(2)));
  expect(await noticeOf(NOTICE(2))).toBeInTheDocument();
  expect(screen.queryByText(NOTICE(3))).toBeNull();
});

test("the folder is read again every 15 seconds while the page is visible, and not while hidden", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let visible = true;
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => (visible ? "visible" : "hidden"));
  const { wire } = await renderActions(task("a1", "idle"));
  await waitFor(() => expect(wire.requests("git.status")).toHaveLength(1));
  await act(async () => wire.reply("git.status", status(1)));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15_100);
  });
  expect(wire.requests("git.status")).toHaveLength(2);
  await act(async () => wire.reply("git.status", status(1)));
  await act(async () => {
    visible = false;
    fireEvent(document, new Event("visibilitychange"));
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(wire.requests("git.status")).toHaveLength(2);
});
