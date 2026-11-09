import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
async function setup({
  path = "/agents",
  items,
  tasks,
  collapsed = false,
}: {
  path?: string;
  items: typeof projects;
  tasks: Task[];
  collapsed?: boolean;
}) {
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
  // Nothing is decided until both lists have arrived: no flash of an action that does not apply.
  expect(actions()).toEqual([]);
  await act(async () => wire.reply("projects.list", { rev: "1", items }));
  expect(actions()).toEqual([]);
  await act(async () => {
    wire.reply("agents.list", { rev: "1", items: tasks });
    wire.reply("agents.profiles", { items: profiles });
  });
  return { wire, router, body, actions };
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
