import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectDataProvider } from "../projects/ProjectData";
import { ProjectWire, projects } from "../projects/test-wire";
import { AgentDataProvider } from "./AgentData";
import { AgentGrid } from "./AgentGrid";
import { AgentList } from "./AgentList";
import { AgentRow } from "./AgentRow";
import { AgentHeader } from "./detail/AgentHeader";
import { useAgentsData } from "./hooks";
import { profiles, task } from "./test-fixtures";

const terminal = vi.fn();
vi.mock("../terminal/TerminalView", () => ({
  TerminalView: (props: unknown) => {
    terminal(props);
    return <div />;
  },
}));
beforeEach(() => {
  terminal.mockClear();
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
test.each([
  [4, 2],
  [5, 3],
  [9, 3],
  [10, 4],
])("%i tasks use %i columns with read-only previews", (count, columns) => {
  const open = vi.fn();
  render(<AgentGrid tasks={Array.from({ length: count }, (_, i) => task(String(i)))} onOpen={open} />);
  expect(screen.getByRole("region", { name: "에이전트 터미널 격자" })).toHaveStyle({
    gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
  });
  expect(terminal).toHaveBeenCalledWith(expect.objectContaining({ readOnly: true, scale: expect.any(Number) }));
  fireEvent.click(screen.getByRole("button", { name: "작업 0 터미널 미리보기" }));
  expect(open).toHaveBeenCalledWith("0");
});
test("tiles retain title and state without a session and observe cell width", () => {
  const observers: ResizeObserverCallback[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        observers.push(callback);
      }
      observe() {}
      disconnect() {}
    },
  );
  render(
    <AgentGrid tasks={[{ ...task("a", "preparing"), terminalId: null }, task("b", "waiting")]} onOpen={() => {}} />,
  );
  expect(screen.getByRole("button", { name: "작업 a 터미널 미리보기" })).toHaveTextContent("준비 중");
  act(() => observers[1]!([{ contentRect: { width: 350 } } as ResizeObserverEntry], {} as ResizeObserver));
  expect(terminal).toHaveBeenLastCalledWith(expect.objectContaining({ id: "t", readOnly: true, scale: 0.35 }));
  vi.unstubAllGlobals();
});

test("closed work never appears in grid previews, column counts or the nonempty state", () => {
  const tasks = [task("a"), task("b"), task("c"), task("d"), task("closed", "closed")];
  const view = render(<AgentGrid tasks={tasks} onOpen={() => {}} />);
  expect(screen.queryByRole("button", { name: "작업 closed 터미널 미리보기" })).toBeNull();
  expect(screen.getByRole("region", { name: "에이전트 터미널 격자" })).toHaveStyle({
    gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
  });
  view.rerender(<AgentGrid tasks={[task("closed", "closed")]} onOpen={() => {}} />);
  expect(screen.queryByRole("region", { name: "에이전트 터미널 격자" })).toBeNull();
  expect(screen.getByText("에이전트 작업이 없습니다")).toBeInTheDocument();
});

test("agent_state_mark_matches_across_list_header_and_grid", async () => {
  const wire = new ProjectWire();
  const items = [
    { ...task("w", "waiting"), attention: { kind: "input", message: "Bash: pnpm test", atMs: 1 } },
    { ...task("i", "idle"), attention: { kind: "idle", message: "다음 지시를 기다립니다", atMs: 1 } },
    task("r", "running"),
  ];
  render(
    <RpcProvider client={wire.client}>
      {items.map((item) => (
        <div key={item.id}>
          <AgentRow
            task={item}
            selected={false}
            projectName="devbox"
            toolName="Claude Code"
            now={1000}
            onSelect={() => {}}
          />
          <AgentHeader task={item} projectName="devbox" toolName="Claude Code" now={1000} />
        </div>
      ))}
      <AgentGrid tasks={items} onOpen={() => {}} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  const expected = {
    w: { state: "waiting", name: "입력 대기", tone: "var(--warn)" },
    i: { state: "idle", name: "다음 지시 대기", tone: "var(--fg-muted)" },
    r: { state: "running", name: "실행 중", tone: "var(--progress)" },
  } as const;
  for (const [id, want] of Object.entries(expected)) {
    const surfaces = {
      row: screen.getByRole("button", { name: new RegExp(`작업 ${id}(?! 터미널 미리보기)`) }),
      header: screen.getByRole("heading", { name: `작업 ${id}` }).closest("header")!,
      grid: screen.getByRole("button", { name: `작업 ${id} 터미널 미리보기` }),
    };
    const seen = Object.entries(surfaces).map(([surface, element]) => {
      // Each surface shows the state name once, as text.
      const name = within(element).getByText(want.name, { exact: true });
      const mark = name.closest<HTMLElement>("[data-state]");
      const icon = mark?.querySelector("svg");
      return {
        surface,
        state: mark?.dataset.state,
        name: name.textContent,
        icon: icon?.getAttribute("class"),
        hidden: icon?.getAttribute("aria-hidden"),
        tone: icon?.style.color,
      };
    });
    const icon = seen[0]?.icon;
    expect(icon).toBeTruthy();
    expect(seen).toEqual(
      ["row", "header", "grid"].map((surface) => ({
        surface,
        state: want.state,
        name: want.name,
        icon,
        hidden: "true",
        tone: want.tone,
      })),
    );
  }
  // The detail header no longer draws a second state picture next to the name.
  expect(screen.queryAllByRole("img")).toHaveLength(0);
});

function ListAndGrid() {
  const data = useAgentsData();
  return (
    <>
      <AgentList data={data} onSelect={() => {}} header={null} />
      <AgentGrid tasks={data.tasks} onOpen={() => {}} />
    </>
  );
}
test.each([
  ["the profile list names the tool", { items: profiles }, "Codex"],
  ["the profile list has not arrived", undefined, "codex"],
  ["the profile list has no such tool", { items: profiles.filter((p) => p.id !== "codex") }, "codex"],
])("row_and_grid_show_project_and_tool: %s", async (_, profileList, tool) => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <ProjectDataProvider>
        <AgentDataProvider>
          <ListAndGrid />
        </AgentDataProvider>
      </ProjectDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.profiles")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: [{ ...projects[0]!, name: "app" }] });
    wire.reply("agents.list", { rev: "1", items: [{ ...task("c"), profile: "codex" }] });
    if (profileList) wire.reply("agents.profiles", profileList);
  });
  const row = within(await screen.findByRole("region", { name: "실행 중" })).getByRole("button");
  // The second line of the row: project · tool.
  expect(row.children[1]).toHaveTextContent(new RegExp(`^app · ${tool}$`));
  const tile = screen.getByRole("button", { name: "작업 c 터미널 미리보기" });
  expect(within(tile).getByText(`app · ${tool}`, { exact: true })).toBeInTheDocument();
});
