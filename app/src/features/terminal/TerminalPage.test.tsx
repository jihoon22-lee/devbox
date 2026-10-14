import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { TooltipProvider } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { useTerminalFocus } from "./focus";
import { openTab, projectKey, sessionsIn, useTerminalLayout } from "./store";
import { TerminalPage } from "./TerminalPage";
import type { Session } from "./useSessions";

vi.mock("./TerminalView", () => ({
  TerminalView: ({ id, onCwd, onBell }: { id: string; onCwd?: (cwd: string) => void; onBell?: () => void }) => (
    <div data-keyscope="terminal">
      <button type="button" onClick={() => onCwd?.(`/cwd/${id}`)}>
        focus {id}
      </button>
      <button type="button" onClick={onBell}>
        bell {id}
      </button>
    </div>
  ),
}));
export const session = (id: string, projectId: string | null = null): Session => ({
  id,
  projectId,
  title: id,
  cwd: "/fixture",
  command: "bash",
  kind: "shell",
  alive: true,
  createdMs: 1,
});
beforeEach(() => {
  useCurrentProject.getState().set(undefined);
  useTerminalLayout.setState({ byProject: {} });
  useTerminalFocus.setState({ byProject: {}, cwd: {}, bells: {} });
});
function mount() {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <CacheProbe />
      <TooltipProvider>
        <TerminalPage />
      </TooltipProvider>
    </RpcProvider>,
  );
  return wire;
}
let cache: QueryClient;
function CacheProbe() {
  cache = useQueryClient();
  return null;
}
async function replyList(wire: ProjectWire, items: Session[]) {
  await waitFor(() => expect(wire.requests("terminal.list").length).toBeGreaterThan(0));
  await act(async () => wire.reply("terminal.list", { rev: "1", items }));
}
test("no-project filters the all-project RPC, closes panes without ending sessions, and confirms session ending", async () => {
  const wire = mount();
  await replyList(wire, [session("s1"), session("other", "p")]);
  expect(screen.queryByRole("button", { name: /other/ })).not.toBeInTheDocument();
  await userEvent.click(await screen.findByRole("button", { name: /s1.*bash/ }));
  await userEvent.click(screen.getByRole("button", { name: "focus s1" }));
  fireEvent.keyDown(window, { code: "KeyW", ctrlKey: true, shiftKey: true });
  expect(useTerminalLayout.getState().byProject.none?.tabs).toHaveLength(0);
  expect(wire.requests("terminal.close")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: /s1.*bash/ }));
  await userEvent.click(await screen.findByRole("button", { name: "s1 메뉴" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "세션 끝내기" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("s1");
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  expect(wire.requests("terminal.close")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "취소" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "s1 메뉴" })).toHaveFocus());
  await userEvent.click(screen.getByRole("button", { name: "s1 메뉴" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "세션 끝내기" }));
  await screen.findByRole("dialog");
  await userEvent.click(screen.getByRole("button", { name: "세션 끝내기" }));
  await waitFor(() => expect(wire.requests("terminal.close")).toHaveLength(1));
  await act(async () => wire.fail("terminal.close"));
  expect(await screen.findByRole("alert")).toHaveTextContent("끝내지 못했습니다");
  expect(useTerminalLayout.getState().byProject.none?.tabs).toHaveLength(1);
});
test("in-flight split captures its project/focused CWD and survives switching projects", async () => {
  useCurrentProject.getState().set("p");
  useTerminalLayout.getState().update(projectKey("p"), (layout) => openTab(layout, "s1"));
  const wire = mount();
  await replyList(wire, [session("s1", "p")]);
  await userEvent.click(screen.getByRole("button", { name: "focus s1" }));
  fireEvent.keyDown(window, { code: "KeyE", ctrlKey: true, shiftKey: true });
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
  expect(wire.request("terminal.create").params).toMatchObject({ projectId: "p", cwd: "/cwd/s1" });
  await act(async () => useCurrentProject.getState().set("q"));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [] }));
  await act(async () => wire.reply("terminal.create", session("s2", "p")));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(4));
  await act(async () => {
    for (const request of wire.requests("terminal.list").slice(2))
      wire.deliver({
        type: "response",
        id: request.id,
        result: {
          rev: "2",
          items:
            (request.params as { projectId: string }).projectId === "p" ? [session("s1", "p"), session("s2", "p")] : [],
        },
      });
  });
  await waitFor(() =>
    expect(sessionsIn(useTerminalLayout.getState().byProject[projectKey("p")]!.tabs[0]!.root)).toEqual(["s1", "s2"]),
  );
  expect(useTerminalLayout.getState().byProject[projectKey("q")]?.tabs ?? []).toHaveLength(0);
  expect(screen.queryByRole("button", { name: "focus s2" })).not.toBeInTheDocument();
});
test("rename preserves failure input; profiles and zellij choices use actual creation parameters", async () => {
  const wire = mount();
  await replyList(wire, [session("s1")]);
  await waitFor(() => expect(wire.requests("terminal.profiles")).toHaveLength(1));
  await act(async () =>
    wire.reply("terminal.profiles", {
      items: [
        { name: "기본 셸", command: "bash -l" },
        { name: "Python", command: "python3" },
      ],
    }),
  );
  await userEvent.click(screen.getByRole("button", { name: "s1 메뉴" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "이름 바꾸기" }));
  const input = screen.getByRole("textbox", { name: "s1 이름" });
  await userEvent.clear(input);
  await userEvent.type(input, "renamed");
  await userEvent.keyboard("{Enter}");
  await waitFor(() => expect(wire.requests("terminal.rename")).toHaveLength(1));
  expect(wire.request("terminal.rename").params).toEqual({ id: "s1", title: "renamed" });
  await act(async () => wire.fail("terminal.rename"));
  expect(input).toHaveValue("renamed");
  await userEvent.click(screen.getByRole("button", { name: "취소" }));
  await userEvent.click(screen.getByRole("button", { name: "새 세션 ▾" }));
  expect(await screen.findAllByRole("menuitem", { name: "기본 셸" })).toHaveLength(1);
  await userEvent.click(await screen.findByRole("menuitem", { name: "Python" }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
  expect(wire.request("terminal.create").params).toMatchObject({ command: "python3", attachZellij: null });
  await act(async () => wire.reply("terminal.create", session("python")));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("s1"), session("python")] }));
  await userEvent.click(screen.getByRole("button", { name: "새 세션 ▾" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "기존 zellij 세션에 붙기" }));
  await waitFor(() => expect(wire.requests("terminal.zellij_list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.zellij_list", { items: ["existing"] }));
  await userEvent.click(await screen.findByRole("button", { name: "existing" }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(2));
  expect(wire.request("terminal.create", 1).params).toMatchObject({ command: null, attachZellij: "existing" });
});

test("a cancelled pre-create list reply cannot remove the newly opened pane", async () => {
  const wire = mount();
  await replyList(wire, []);
  await screen.findByText("터미널 세션이 없습니다");
  await userEvent.click(screen.getByRole("button", { name: "새 터미널" }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
  act(() => {
    void cache.invalidateQueries({ queryKey: ["terminal.list"] });
  });
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  const obsolete = wire.request("terminal.list", 1);
  await act(async () => wire.reply("terminal.create", session("new")));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(3));
  await act(async () => wire.deliver({ type: "response", id: obsolete.id, result: { rev: "1", items: [] } }));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("new")] }));
  expect(await screen.findByRole("button", { name: "focus new" })).toBeVisible();
  expect(sessionsIn(useTerminalLayout.getState().byProject.none!.tabs[0]!.root)).toEqual(["new"]);
});
