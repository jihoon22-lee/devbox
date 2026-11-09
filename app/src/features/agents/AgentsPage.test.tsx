import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { messageFor } from "../../rpc/messages";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useLayout } from "../../shell/layout";
import { buildRouter } from "../../shell/router";
import { TooltipProvider } from "../../ui";
import { ProjectDataProvider } from "../projects/ProjectData";
import { ProjectWire, projects } from "../projects/test-wire";
import { AgentCommands } from "./AgentCommands";
import { AgentDataProvider } from "./AgentData";
import { AgentsPage, agentsSearch } from "./AgentsPage";
import { profiles, task } from "./test-fixtures";
import { useAgentsUi } from "./ui";

vi.mock("../terminal/TerminalView", () => ({ TerminalView: () => <div /> }));
beforeEach(() => {
  localStorage.clear();
  useLayout.setState({ listWidth: 260, listCollapsed: false, listAutoCollapsed: false, bottomOpen: false });
  useAgentsUi.getState().setCreating(false);
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
type Task = ReturnType<typeof task>;
/** Mounts the page with both lists requested and unanswered. */
async function mount({ path = "/agents", collapsed = false }: { path?: string; collapsed?: boolean }) {
  useLayout.setState({ listCollapsed: collapsed });
  const wire = new ProjectWire({ trustNotRequired: true });
  const router = buildRouter(
    {
      frame: () => <Outlet />,
      pages: { "/agents": { component: AgentsPage, validateSearch: agentsSearch } },
      settings: () => null,
      popouts: [],
    },
    createMemoryHistory({ initialEntries: [path] }),
  );
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <ProjectDataProvider>
          <AgentDataProvider>
            <RouterProvider router={router} />
            {/* The shell mounts the new task dialog here, as in every window. */}
            <AgentCommands
              navigate={(id) =>
                void router.navigate({
                  to: "/agents",
                  search: (previous: Record<string, unknown>) => ({ ...previous, id, view: "list" }),
                })
              }
              currentId={() => undefined}
            />
          </AgentDataProvider>
        </ProjectDataProvider>
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(1));
  const body = await screen.findByRole("region", { name: "본문" });
  const actions = () =>
    ["프로젝트 추가", "새 작업", "첫 입력 대기 작업 열기"].filter((name) =>
      within(body).queryByRole("button", { name }),
    );
  // Nothing is decided before the lists arrive: no flash of an action that does not apply.
  expect(actions()).toEqual([]);
  return { wire, router, body, actions };
}
async function setup({
  path,
  items,
  tasks,
  collapsed,
}: {
  path?: string;
  items: typeof projects;
  tasks: Task[];
  collapsed?: boolean;
}) {
  const page = await mount({ path, collapsed });
  await act(async () => page.wire.reply("projects.list", { rev: "1", items }));
  expect(page.actions()).toEqual([]);
  await act(async () => {
    page.wire.reply("agents.list", { rev: "1", items: tasks });
    page.wire.reply("agents.profiles", { items: profiles });
  });
  return page;
}

const now = Date.now();
test.each([
  { name: "no project", items: [], tasks: [], action: "프로젝트 추가", opens: "프로젝트 추가" },
  {
    name: "a project and no task",
    items: projects.slice(0, 1),
    tasks: [],
    action: "새 작업",
    opens: "새 에이전트 작업",
  },
  {
    name: "the grid with closed work only",
    path: "/agents?view=grid",
    items: projects.slice(0, 1),
    tasks: [task("c", "closed", now)],
    action: "새 작업",
    opens: "새 에이전트 작업",
  },
])(
  "agent_empty_body_keeps_next_action_available_without_list: $name",
  async ({ path, items, tasks, action, opens }) => {
    const { wire, body, actions } = await setup({ path, items, tasks, collapsed: true });
    // The list and its own buttons are hidden; the body still offers the one next step.
    expect(screen.queryByRole("complementary", { name: "목록" })).toBeNull();
    await waitFor(() => expect(actions()).toEqual([action]));
    await userEvent.click(within(body).getByRole("button", { name: action }));
    expect(await screen.findByRole("dialog", { name: opens })).toBeInTheDocument();
    expect(wire.requests("agents.create")).toHaveLength(0);
  },
);

test("agent_empty_body_keeps_next_action_available_without_list: a waiting task and nothing selected", async () => {
  // The idle task is first in the attention group; the action goes to the oldest task that waits for an answer.
  const tasks = [task("i", "idle", 1), task("w2", "waiting", 3), task("w1", "waiting", 2), task("r", "running", 4)];
  const { wire, router, body, actions } = await setup({ items: projects.slice(0, 1), tasks, collapsed: true });
  await waitFor(() => expect(actions()).toEqual(["첫 입력 대기 작업 열기"]));
  expect(body).toHaveTextContent("입력 대기 작업이 2개 있습니다.");
  await userEvent.click(within(body).getByRole("button", { name: "첫 입력 대기 작업 열기" }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ id: "w1", view: "list" }));
  expect(await screen.findByRole("article", { name: "에이전트 작업 상세" })).toHaveTextContent("작업 w1");
  expect(wire.requests("agents.create")).toHaveLength(0);
});

test("open tasks without one waiting for an answer leave the choice to the list", async () => {
  const { body, actions } = await setup({
    items: projects.slice(0, 1),
    tasks: [task("i", "idle", 1), task("r", "running", 2)],
  });
  await waitFor(() => expect(body).toHaveTextContent("에이전트 작업을 고르세요"));
  expect(actions()).toEqual([]);
});

// Moved from AgentList.test.tsx: [프로젝트 추가] is the body's action now, not the list's.
test("shows_add_project_when_no_project_is_registered", async () => {
  const { body } = await setup({ items: [], tasks: [] });
  expect(await within(body).findByText("등록된 프로젝트가 없습니다")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "프로젝트 추가" })).toBe(
    within(body).getByRole("button", { name: "프로젝트 추가" }),
  );
});
test("hides_add_project_when_projects_exist", async () => {
  const { body } = await setup({ items: projects, tasks: [] });
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "프로젝트 추가" })).not.toBeInTheDocument();
  expect(within(body).getByRole("button", { name: "새 작업" })).toBeInTheDocument();
});

test("tasks that arrive before the project list decide nothing until it arrives", async () => {
  const { wire, actions } = await mount({});
  await act(async () => {
    wire.reply("agents.list", { rev: "1", items: [] });
    wire.reply("agents.profiles", { items: profiles });
  });
  // The task list has arrived (the list says so), but the first step depends on the project count:
  // no [새 작업] flashes up for someone who has no project yet.
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeInTheDocument();
  expect(actions()).toEqual([]);
  await act(async () => wire.reply("projects.list", { rev: "1", items: [] }));
  await waitFor(() => expect(actions()).toEqual(["프로젝트 추가"]));
});

const FAILED = messageFor("internal").title;
test("a failed project list neither blanks the pick nor goes unexplained", async () => {
  const { wire, body, actions } = await mount({});
  await act(async () => {
    wire.fail("projects.list");
    wire.reply("agents.list", { rev: "1", items: [task("i", "idle", 1), task("w", "waiting", 2), task("r")] });
  });
  // Picking a task needs only the task list.
  await waitFor(() => expect(actions()).toEqual(["첫 입력 대기 작업 열기"]));
  // Nothing else on the page shows the failed project list (the switcher only once opened), so the body does.
  const alert = within(body).getByRole("alert");
  expect(alert).toHaveTextContent(FAILED);
  await userEvent.click(within(alert).getByRole("button", { name: "다시 확인" }));
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(2));
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
  await waitFor(() => expect(within(body).queryByRole("alert")).toBeNull());
  expect(actions()).toEqual(["첫 입력 대기 작업 열기"]);
});

test.each(["/agents", "/agents?view=grid"])(
  "with no open task, a failed project list shows its error instead of a guessed first step: %s",
  async (path) => {
    const { wire, body, actions } = await mount({ path });
    await act(async () => {
      wire.fail("projects.list");
      wire.reply("agents.list", { rev: "1", items: [] });
    });
    const alert = await within(body).findByRole("alert");
    expect(alert).toHaveTextContent(FAILED);
    expect(actions()).toEqual([]);
    await userEvent.click(within(alert).getByRole("button", { name: "다시 확인" }));
    await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(2));
    await act(async () => wire.reply("projects.list", { rev: "1", items: [] }));
    await waitFor(() => expect(actions()).toEqual(["프로젝트 추가"]));
    expect(within(body).queryByRole("alert")).toBeNull();
  },
);

test.each(["/agents", "/agents?view=grid"])(
  "a failed task list shows its error in the body only while the list is collapsed: %s",
  async (path) => {
    const { wire, body, actions } = await mount({ path, collapsed: true });
    await act(async () => {
      wire.reply("projects.list", { rev: "1", items: projects });
      wire.fail("agents.list");
    });
    // The collapsed list hides its own error, so the body shows it, with no action guessed from no data.
    const alert = await within(body).findByRole("alert");
    expect(alert).toHaveTextContent(FAILED);
    expect(actions()).toEqual([]);
    // An open list shows the error itself; the body does not repeat it.
    act(() => useLayout.getState().toggleList());
    const list = await screen.findByRole("complementary", { name: "목록" });
    expect(within(list).getByRole("alert")).toHaveTextContent(FAILED);
    expect(within(body).queryByRole("alert")).toBeNull();
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    act(() => useLayout.getState().toggleList());
    // [다시 확인] in the body recovers without opening the list.
    await userEvent.click(within(await within(body).findByRole("alert")).getByRole("button", { name: "다시 확인" }));
    await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(2));
    await act(async () => wire.reply("agents.list", { rev: "1", items: [] }));
    await waitFor(() => expect(actions()).toEqual(["새 작업"]));
    expect(within(body).queryByRole("alert")).toBeNull();
  },
);
