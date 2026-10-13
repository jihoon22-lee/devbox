import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { profiles, task } from "../features/agents/test-fixtures";
import { useAgentsUi } from "../features/agents/ui";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useCommands } from "./commands";
import { useLayout } from "./layout";
import { Shell } from "./Shell";

vi.mock("../features/terminal/TerminalView", () => ({ TerminalView: () => <div /> }));

async function setup(path: string) {
  useAgentsUi.getState().setCreating(false);
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
  expect(await screen.findByRole("article", { name: "에이전트 작업 상세" })).toHaveTextContent("New task");
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

test("detail uses live terminal status despite missing CLI session metadata; explicit instruct focuses without polling theft", async () => {
  const { wire } = await setup("/agents?id=w&gateway=ws%3A%2F%2Ffixture&renderer=dom");
  await waitFor(() => expect(wire.requests("agents.get")).toHaveLength(1));
  await act(async () => wire.reply("agents.get", { ...task("w", "waiting"), sessionId: null }));
  await waitFor(() =>
    expect(
      wire.requests("terminal.list").some((request) => (request.params as { projectId: string }).projectId === "a"),
    ).toBe(true),
  );
  expect(screen.getByRole("button", { name: "세션 확인 중…" })).toBeDisabled();
  const request = wire
    .requests("terminal.list")
    .find((request) => (request.params as { projectId: string }).projectId === "a")!;
  await act(async () => wire.deliver({ type: "response", id: request.id, error: { code: "internal" } }));
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "세션 확인 중…" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "다시 확인" }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
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
          alive: false,
          command: "fixture",
          createdMs: 1,
        },
      ],
    }),
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "재개" })).toBeEnabled());
  const subscription = wire.sent.filter((message) => message.topic === "terminal.changed").at(-1)!;
  await act(async () => wire.deliver({ type: "event", subId: subscription.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(3));
  await act(async () =>
    wire.reply("terminal.list", {
      rev: "2",
      items: [
        {
          id: "t",
          projectId: "a",
          title: "agent",
          cwd: "/fixture",
          kind: "agent",
          alive: true,
          command: "fixture",
          createdMs: 1,
        },
      ],
    }),
  );
  const box = await screen.findByLabelText("에이전트에게 보낼 지시");
  await waitFor(() => expect(screen.getByRole("button", { name: "이어서 지시" })).toBeEnabled());
  await userEvent.click(screen.getByRole("button", { name: "이어서 지시" }));
  expect(box).toHaveFocus();
  fireEvent.change(box, { target: { value: "초안 유지" } });
  await userEvent.click(screen.getByRole("tab", { name: "변경" }));
  expect(box).toHaveValue("초안 유지");
  await waitFor(() => expect(wire.requests("agents.changes")).toHaveLength(1));
  await act(async () =>
    wire.reply("agents.changes", { base: "main", ahead: 0, behind: 1, headOid: "h", baseOid: "b", files: [] }),
  );
  expect(screen.getByRole("tab", { name: "변경" })).toHaveFocus();
  expect(box).not.toHaveFocus();
  await waitFor(() => expect(screen.getByRole("tabpanel", { name: "변경" })).toHaveTextContent("뒤처짐 1"));
  expect(wire.requests("agents.resources")).toHaveLength(1);
  expect(wire.sent.filter((message) => message.topic === "agents.changed")).toHaveLength(1);
});
test("agent popout mounts full detail without frame and retains gateway/renderer", async () => {
  const { wire } = await setup("/popout/agent/w?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  expect(await screen.findByRole("article", { name: "에이전트 작업 상세" })).toHaveTextContent("작업 w");
  expect(screen.getByRole("tab", { name: "변경" })).toBeInTheDocument();
  expect(screen.queryByRole("navigation", { name: "섹션" })).toBeNull();
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  expect(wire.requests("agents.list")).toHaveLength(1);
});

test("grid selection keeps routing search and popout grid never mounts attention bridge", async () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  const { wire, view } = await setup("/agents?view=grid&gateway=ws%3A%2F%2Ffixture&renderer=dom");
  expect(await screen.findByRole("button", { name: "작업 w 터미널 미리보기" })).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "작업 w 터미널 미리보기" }));
  await waitFor(() => expect(window.location.search).toContain("view=list"));
  expect(window.location.search).toContain("id=w");
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(1);
  view.unmount();
  const popout = await setup("/popout/agents-grid?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  expect(await screen.findByRole("region", { name: "에이전트 터미널 격자" })).toBeInTheDocument();
  expect(popout.wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(0);
  expect(screen.queryByRole("button", { name: /알림 \d+개/ })).toBeNull();
  expect(document.title).toBe("devbox");
  popout.view.unmount();
  vi.unstubAllGlobals();
});

test("a popout keeps attention ownership disabled after global section and waiting navigation", async () => {
  document.title = "분리 창";
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  const { wire, view } = await setup("/popout/agents-grid?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  await screen.findByRole("button", { name: "작업 w 터미널 미리보기" });
  fireEvent.keyDown(window, { code: "KeyJ", ctrlKey: true, shiftKey: true });
  await waitFor(() => expect(window.location.pathname).toBe("/agents"));
  expect(window.location.search).toContain("id=w");
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(0);
  expect(document.title).toBe("분리 창");
  fireEvent.keyDown(window, { code: "Digit2", ctrlKey: true });
  await waitFor(() => expect(window.location.pathname).toBe("/terminal"));
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(0);
  view.unmount();
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: { metadata: { currentWindow: { label: "popout-agents-grid-main" } } },
    configurable: true,
  });
  try {
    const native = await setup("/agents?gateway=ws%3A%2F%2Ffixture&renderer=dom");
    await screen.findByRole("button", { name: "에이전트 3(입력 대기 2)" });
    expect(native.wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(0);
    native.view.unmount();
  } finally {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    document.title = "devbox";
    vi.unstubAllGlobals();
  }
});

test("opening a detail from the popout grid keeps the reusable grid window on its route", async () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  const popup = { closed: false, focus: vi.fn() };
  const open = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
  const { view } = await setup("/popout/agents-grid?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  await userEvent.click(await screen.findByRole("button", { name: "작업 w 터미널 미리보기" }));
  await waitFor(() =>
    expect(open).toHaveBeenCalledWith(
      "/popout/agent/w?gateway=ws%3A%2F%2Ffixture&renderer=dom",
      expect.any(String),
      expect.any(String),
    ),
  );
  expect(window.location.pathname).toBe("/popout/agents-grid");
  expect(screen.getByRole("region", { name: "에이전트 터미널 격자" })).toBeInTheDocument();
  popup.closed = true;
  view.unmount();
  vi.unstubAllGlobals();
});

test("hook connection banner opens agent settings while shared data and attention retain their owners", async () => {
  const { wire } = await setup("/agents?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  const subscription = wire.sent.find((message) => message.topic === "agents.changed")!;
  await act(async () => wire.deliver({ type: "event", subId: subscription.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(2));
  await act(async () =>
    wire.reply("agents.list", { rev: "2", items: [{ ...task("unlinked", "running"), sessionId: null }] }),
  );
  await userEvent.click(await screen.findByRole("button", { name: "연결 확인" }));
  await waitFor(() => expect(window.location.pathname).toBe("/settings"));
  expect(screen.getByRole("tab", { name: "에이전트" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("tabpanel", { name: "에이전트" })).toBeInTheDocument();
  expect(window.location.search).toContain("gateway=");
  expect(window.location.search).toContain("renderer=dom");
  expect(wire.sent.filter((message) => message.topic === "agents.changed")).toHaveLength(1);
  expect(wire.sent.filter((message) => message.topic === "agents.attention")).toHaveLength(1);
  expect(wire.requests("agents.list")).toHaveLength(2);
});
