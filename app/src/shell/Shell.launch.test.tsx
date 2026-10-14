import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCurrentProject } from "../features/projects/store";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { projectKey, sessionsIn, useTerminalLayout } from "../features/terminal/store";
import type { Session } from "../features/terminal/useSessions";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { Shell } from "./Shell";

vi.mock("../features/terminal/TerminalView", () => ({
  TerminalView: ({ id }: { id: string }) => <div>owned terminal {id}</div>,
}));
let cache: QueryClient;
function CacheProbe() {
  cache = useQueryClient();
  return null;
}
const terminal = (id: string, title: string): Session => ({
  id,
  title,
  projectId: "a",
  cwd: "/fixture",
  command: "bash",
  kind: "shell",
  alive: true,
  createdMs: 1,
});

test("launch opens its exact sessions through Shell navigation and fences a stale empty list", async () => {
  useTerminalLayout.setState({ byProject: {} });
  useCurrentProject.getState().set(undefined);
  window.history.replaceState({}, "", "/agents?gateway=ws%3A%2F%2Ffixture&renderer=dom");
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const wire = new ProjectWire({ trustNotRequired: true });
  render(
    <RpcProvider client={wire.client}>
      <CacheProbe />
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items: [projects[0]] });
    wire.reply("agents.list", { rev: "1", items: [] });
    wire.reply("agents.profiles", { items: [] });
    cache.setQueryData(["terminal.list", { projectId: "a" }], { rev: "9", items: [] });
  });
  await userEvent.click(await screen.findByRole("button", { name: "프로젝트 전환" }));
  await waitFor(() => expect(wire.requests("projects.config")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.config", {
      config: { agent: {}, launch: { services: [], terminals: [{ name: "web" }, { name: "logs" }], agents: [] } },
    }),
  );
  await userEvent.click(await screen.findByRole("button", { name: "devbox 시작 구성 실행" }));
  await waitFor(() => expect(wire.requests("projects.launch")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.launch", { terminals: 2, agents: 0, terminalIds: ["web-id", "logs-id"], rev: "10" }),
  );
  await waitFor(() => expect(window.location.pathname).toBe("/terminal"));
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(1));
  await act(async () => wire.reply("terminal.list", { rev: "9", items: [] }));
  await waitFor(() => expect(cache.getQueryState(["terminal.list", { projectId: "a" }])?.fetchStatus).toBe("idle"));
  const opened = () =>
    useTerminalLayout.getState().byProject[projectKey("a")]?.tabs.flatMap((tab) => sessionsIn(tab.root));
  expect(opened()).toEqual(["web-id", "logs-id"]);
  let refresh!: Promise<void>;
  await act(async () => {
    refresh = cache.invalidateQueries({ queryKey: ["terminal.list"] });
  });
  await waitFor(() => expect(wire.requests("terminal.list")).toHaveLength(2));
  await act(async () => {
    wire.reply("terminal.list", { rev: "10", items: [terminal("web-id", "web"), terminal("logs-id", "logs")] });
    await refresh;
  });
  expect(await screen.findByRole("tab", { name: "web" })).toBeVisible();
  expect(screen.getByRole("tab", { name: "logs" })).toBeVisible();
  expect(opened()).toEqual(["web-id", "logs-id"]);
  await act(async () => {
    refresh = cache.invalidateQueries({ queryKey: ["terminal.list"] });
  });
  await act(async () => {
    wire.reply("terminal.list", { rev: "11", items: [terminal("web-id", "web")] });
    await refresh;
  });
  await waitFor(() => expect(opened()).toEqual(["web-id"]));
  expect(wire.requests("projects.launch")).toHaveLength(1);
});
