import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { beforeEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { BottomTerminal } from "./BottomTerminal";
import type { Session } from "./useSessions";

vi.mock("./TerminalView", () => ({
  TerminalView: ({ id, focusRequest }: { id: string; focusRequest?: number }) => (
    <p data-focus-request={focusRequest}>terminal {id}</p>
  ),
}));
const session = (id: string, projectId: string | null = null): Session => ({
  id,
  projectId,
  title: "하단",
  cwd: "/fixture",
  command: "bash",
  kind: "shell",
  alive: true,
  createdMs: 1,
});
beforeEach(() => {
  localStorage.clear();
  useCurrentProject.getState().set(undefined);
});
function mount() {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <StrictMode>
        <BottomTerminal />
      </StrictMode>
    </RpcProvider>,
  );
  return wire;
}
test("StrictMode creates once, retains a just-created ID through list refresh, and remembers per project", async () => {
  const wire = mount();
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [] }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
  await act(async () => wire.reply("terminal.create", session("bottom")));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("bottom")] }));
  expect(await screen.findByText("terminal bottom")).toBeVisible();
  expect(wire.requests("terminal.create")).toHaveLength(1);
  expect(screen.getByText("terminal bottom")).toBeVisible();
  await act(async () => useCurrentProject.getState().set("p"));
  await waitFor(() =>
    expect(
      wire.requests("terminal.list").filter((request) => (request.params as { projectId: string }).projectId === "p")
        .length,
    ).toBeGreaterThan(0),
  );
  await act(async () => {
    for (const request of wire
      .requests("terminal.list")
      .filter((request) => (request.params as { projectId: string }).projectId === "p"))
      wire.deliver({ type: "response", id: request.id, result: { rev: "2", items: [] } });
  });
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(2));
  expect(wire.request("terminal.create", 1).params).toMatchObject({ projectId: "p", title: "하단" });
  await act(async () => useCurrentProject.getState().set(undefined));
  const beforeRefresh = wire.requests("terminal.list").length;
  await act(async () => wire.reply("terminal.create", session("project-bottom", "p")));
  await waitFor(() => expect(wire.requests("terminal.list").length).toBeGreaterThan(beforeRefresh));
  await act(async () => {
    for (const request of wire.requests("terminal.list").slice(beforeRefresh))
      wire.deliver({
        type: "response",
        id: request.id,
        result: {
          rev: "3",
          items:
            (request.params as { projectId: string | null }).projectId === "p"
              ? [session("project-bottom", "p")]
              : [session("bottom"), session("project-bottom", "p")],
        },
      });
  });
  expect(localStorage.getItem("devbox.bottomTerminal.project:p")).toBe("project-bottom");
  expect(screen.queryByText("terminal project-bottom")).not.toBeInTheDocument();
});

test("a confirmed session disappears in a newer list without automatically replacing it", async () => {
  localStorage.setItem("devbox.bottomTerminal.none", "existing");
  const wire = mount();
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("existing")] }));
  expect(await screen.findByText("terminal existing")).toBeVisible();
  // Reconnect refreshes queries without ending terminal sessions by itself.
  await act(async () => wire.close());
  await act(async () => wire.connect());
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [] }));
  expect(await screen.findByRole("alert")).toHaveTextContent("세션이 끝났습니다");
  expect(wire.requests("terminal.create")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
});
test("failed automatic creation has no hot loop and retries only after the explicit button", async () => {
  const wire = mount();
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [] }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
  await act(async () => wire.fail("terminal.create"));
  expect(await screen.findByRole("alert")).toHaveTextContent("만들지 못했습니다");
  expect(wire.requests("terminal.create")).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(2));
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  await act(async () => wire.reply("terminal.create", session("retry")));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("retry")] }));
  expect(await screen.findByText("terminal retry")).toBeVisible();
});

test("failed post-create refresh retries the list and reuses the confirmed created session", async () => {
  const wire = mount();
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [] }));
  await waitFor(() => expect(wire.requests("terminal.create")).toHaveLength(1));
  await act(async () => wire.reply("terminal.create", session("confirmed")));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => wire.fail("terminal.list"));
  expect(await screen.findByRole("alert")).toHaveTextContent("목록을 불러오지 못했습니다");
  await waitFor(() => expect(localStorage.getItem("devbox.bottomTerminal.none")).toBe("confirmed"));
  expect(wire.requests("terminal.create")).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(3));
  await act(async () => wire.reply("terminal.list", { rev: "2", items: [session("confirmed")] }));
  expect(await screen.findByText("terminal confirmed")).toBeVisible();
  expect(wire.requests("terminal.create")).toHaveLength(1);
});

test("global focus reaches the retained bottom terminal without creating another session", async () => {
  const { useNativeRuntime } = await import("../../shell/nativeRuntime");
  useNativeRuntime.setState({ bottomFocus: 0 });
  localStorage.setItem("devbox.bottomTerminal.none", "kept");
  const wire = mount();
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "1", items: [session("kept")] }));
  const terminal = await screen.findByText("terminal kept");
  act(() => useNativeRuntime.setState({ bottomFocus: 7 }));
  expect(terminal).toHaveAttribute("data-focus-request", "7");
  expect(wire.requests("terminal.create")).toHaveLength(0);
});
