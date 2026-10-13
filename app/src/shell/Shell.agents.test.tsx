import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { profiles, task } from "../features/agents/test-fixtures";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useCommands } from "./commands";
import { useLayout } from "./layout";
import { Shell } from "./Shell";

vi.mock("../features/terminal/TerminalView", () => ({ TerminalView: () => <div /> }));

async function setup(path: string) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1280 });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
  useLayout.setState({ listCollapsed: false, bottomOpen: false });
  window.history.replaceState({}, "", path);
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: projects });
    wire.reply("agents.list", {
      rev: "1",
      items: [task("w", "waiting", 1), task("i", "idle", 2), task("f", "failed", 3)],
    });
    wire.reply("agents.profiles", { items: profiles });
  });
  return { wire, view };
}
afterEach(() => {
  cleanup();
  window.history.replaceState({}, "", "/");
});

test("shared badges and global waiting navigation work outside agents and preserve connection search", async () => {
  const { wire, view } = await setup("/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  expect(await screen.findByLabelText("주의가 필요한 에이전트 작업 3개")).toHaveAttribute("data-tone", "danger");
  expect(screen.getByRole("button", { name: "에이전트 3(입력 대기 2)" })).toBeInTheDocument();
  expect(wire.requests("agents.resources")).toHaveLength(0);
  const subscription = wire.sent.find((message) => message.topic === "agents.changed")!;
  await act(async () =>
    wire.deliver({ type: "event", subId: subscription.id, rev: "2", payload: { rev: "2", id: "new-waiting" } }),
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(2));
  await act(async () =>
    wire.reply("agents.list", {
      rev: "2",
      items: [task("w", "waiting", 1), task("i", "idle", 2), task("f", "failed", 3), task("new-waiting", "waiting", 4)],
    }),
  );
  expect(await screen.findByRole("button", { name: "에이전트 4(입력 대기 3)" })).toBeInTheDocument();
  expect(screen.getByLabelText("주의가 필요한 에이전트 작업 4개")).toBeInTheDocument();
  fireEvent.keyDown(window, { code: "KeyJ", ctrlKey: true, shiftKey: true });
  await waitFor(() => expect(window.location.search).toContain("id=w"));
  expect(screen.getByRole("button", { current: true })).toHaveTextContent("작업 w");
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  act(() => useCommands.getState().items.get("agents.nextWaiting")!.run());
  await waitFor(() => expect(window.location.search).toContain("id=i"));
  expect(wire.requests("agents.list")).toHaveLength(2);
  expect(wire.sent.filter((m) => m.topic === "agents.changed")).toHaveLength(1);
  view.unmount();
  expect(useCommands.getState().items.has("agents.new")).toBe(false);
});

test("global new opens a single dialog and IME/modal guards block duplicate shortcuts", async () => {
  await setup("/notes?renderer=dom");
  fireEvent.keyDown(window, { code: "KeyN", ctrlKey: true, shiftKey: true, isComposing: true });
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.keyDown(window, { code: "KeyN", ctrlKey: true, shiftKey: true });
  expect(await screen.findAllByRole("dialog", { name: "새 에이전트 작업" })).toHaveLength(1);
  fireEvent.keyDown(window, { code: "KeyN", ctrlKey: true, shiftKey: true });
  expect(screen.getAllByRole("dialog", { name: "새 에이전트 작업" })).toHaveLength(1);
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("unknown selected id presents typed detail error and keeps gateway; invalid view validates to list", async () => {
  const { wire } = await setup("/agents?id=missing&view=invalid&gateway=ws%3A%2F%2Ffixture&renderer=webgl");
  await waitFor(() => expect(wire.requests("agents.get")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("agents.get").id,
      error: { code: "agents.not_found", detail: { id: "missing" } },
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("에이전트 작업을 찾을 수 없습니다");
  expect(window.location.search).toContain("gateway=");
  expect(screen.getByRole("button", { name: "목록" })).toHaveAttribute("aria-pressed", "true");
});

test("confirmed new selection survives stale list and a failed refresh", async () => {
  const { wire } = await setup("/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  act(() => useCommands.getState().items.get("agents.new")!.run());
  expect(await screen.findByRole("dialog", { name: "새 에이전트 작업" })).toBeInTheDocument();
  // No current project is required globally; creation requires an explicit valid choice.
  act(() => screen.getByRole("combobox", { name: "프로젝트" }).focus());
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
  await userEvent.keyboard("{Enter}");
  await screen.findByRole("listbox");
  await userEvent.keyboard("{Home}{Enter}");
  await waitFor(() => expect(wire.requests("projects.prompts")).toHaveLength(1));
  await act(async () => wire.reply("projects.prompts", { items: [] }));
  await userEvent.type(screen.getByLabelText("제목"), "New task");
  await userEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(wire.requests("agents.create")).toHaveLength(1));
  await act(async () => wire.reply("agents.create", { ...task("new", "preparing"), title: "New task" }));
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(2));
  await act(async () => wire.fail("agents.list"));
  await waitFor(() => expect(window.location.search).toContain("id=new"));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(await screen.findByRole("article", { name: "에이전트 작업 미리보기" })).toHaveTextContent("New task");
  expect(screen.queryByRole("button", { name: "작업 new" })).toBeNull();
  expect(window.location.search).toContain("gateway=");
});

test("new command from palette keeps the created dialog input focused", async () => {
  await setup("/notes");
  fireEvent.keyDown(window, { code: "KeyP", ctrlKey: true, shiftKey: true });
  await userEvent.type(await screen.findByRole("combobox", { name: "명령 검색" }), "새 에이전트");
  await userEvent.keyboard("{Enter}");
  expect(await screen.findByLabelText("제목")).toHaveFocus();
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
});
