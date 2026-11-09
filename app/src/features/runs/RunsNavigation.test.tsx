import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";
import type { Run } from "../../rpc/gen/rpc";
import { RpcProvider } from "../../rpc/RpcProvider";
import { buildRouter } from "../../shell/router";
import { NotificationCenter } from "../notifications/NotificationCenter";
import { subscribeNativeNavigation } from "../notifications/native";
import { useNotifications } from "../notifications/store";
import { useCurrentProject } from "../projects/store";
import { RunsPage } from "./RunsPage";
import { runsSearch } from "./search";
import { RunWire, runFixture, serviceDefinition } from "./test-wire";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "main" }) }));
beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  useNotifications.getState().clear();
  useCurrentProject.getState().set(undefined);
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});
function setup(wire: RunWire) {
  const router = buildRouter(
    {
      frame: Frame,
      pages: { "/runs": { component: RunsPage, validateSearch: runsSearch } },
      settings: () => null,
      popouts: [],
    },
    createMemoryHistory({ initialEntries: ["/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom"] }),
  );
  function Frame() {
    useEffect(() => subscribeNativeNavigation((target) => router.navigate(target)), []);
    return (
      <>
        <NotificationCenter />
        <Outlet />
      </>
    );
  }
  render(
    <RpcProvider client={wire.client}>
      <RouterProvider router={router} />
    </RpcProvider>,
  );
  return router;
}
test("notification center selects the exact project and historical run beyond the current list", async () => {
  const wire = new RunWire();
  wire.selected = { ...runFixture, id: "historical-run", projectId: "프로젝트/1", defId: "검사/test", pid: 987 };
  wire.runs = [];
  useCurrentProject.getState().set("other-project");
  useNotifications.getState().pushOnce({
    key: "run:e1",
    kind: "run",
    title: "예약 실행 실패",
    body: "검사/test",
    target: {
      to: "/runs",
      search: { projectId: "프로젝트/1", runId: "historical-run", task: "scheduled/parent" },
    },
  });
  const router = setup(wire);
  await userEvent.click(await screen.findByRole("button", { name: "알림 1개" }));
  await userEvent.click(await screen.findByRole("button", { name: /예약 실행 실패/ }));
  expect(await screen.findByText("PID 987")).toBeVisible();
  expect(screen.getByRole("heading", { name: "검사/test" })).toBeVisible();
  expect(useCurrentProject.getState().id).toBe("프로젝트/1");
  expect(wire.request("runtime.list").params).toMatchObject({ projectId: "프로젝트/1" });
  expect(wire.request("runtime.get").params).toEqual({ runId: "historical-run" });
  expect(router.state.location.search).toMatchObject({ gateway: "ws://fixture", renderer: "dom" });
  act(() => useCurrentProject.getState().set("other-project"));
  await waitFor(() => expect(wire.request("runtime.list", -1).params).toMatchObject({ projectId: "other-project" }));
  await userEvent.click(screen.getByRole("button", { name: "알림 0개" }));
  await userEvent.click(await screen.findByRole("button", { name: /예약 실행 실패/ }));
  await waitFor(() => expect(useCurrentProject.getState().id).toBe("프로젝트/1"));
  expect(await screen.findByText("PID 987")).toBeVisible();
  expect(screen.getByRole("heading", { name: "검사/test" })).toBeVisible();
});
test("cold native run target selects its exact task when preflight produced no run, then warm run selects history", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  const wire = new RunWire();
  wire.runs = [];
  wire.defs = [serviceDefinition, { ...serviceDefinition, def: { ...serviceDefinition.def, id: "검사/test" } }];
  let deliver!: (event: { payload: unknown }) => void;
  mocks.listen.mockImplementation(async (_name, callback) => {
    deliver = callback;
    return vi.fn();
  });
  const search = { projectId: "프로젝트/1", task: "검사/test" };
  mocks.invoke.mockImplementation(async (name) =>
    name === "pending_navigation" ? { sequence: 1, to: "/runs", search } : undefined,
  );
  const router = setup(wire);
  expect(await screen.findByRole("heading", { name: "검사/test" })).toBeVisible();
  expect(useCurrentProject.getState().id).toBe("프로젝트/1");
  expect(wire.requests("runtime.get")).toHaveLength(0);
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("acknowledge_navigation", { sequence: 1 }));
  wire.selected = { ...runFixture, id: "another-run", defId: "web", projectId: search.projectId, pid: 222 };
  await act(async () =>
    deliver({ payload: { sequence: 2, to: "/runs", search: { projectId: search.projectId, runId: "another-run" } } }),
  );
  expect(await screen.findByText("PID 222")).toBeVisible();
  expect(wire.request("runtime.get", -1).params).toEqual({ runId: "another-run" });
  expect(router.state.location.search).toMatchObject({ gateway: "ws://fixture", renderer: "dom" });
  await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("acknowledge_navigation", { sequence: 2 }));
});

test("run navigation exposes process observation using the selected project", async () => {
  const wire = new RunWire();
  useCurrentProject.getState().set("p");
  const router = setup(wire);
  await act(async () => router.navigate({ to: "/runs", search: {} }));
  await userEvent.click(await screen.findByRole("button", { name: "프로세스 목록" }));
  expect(await screen.findByRole("heading", { name: "프로세스 목록" })).toBeVisible();
  expect(router.state.location.search).toMatchObject({ view: "processes" });
  await waitFor(() => expect(wire.requests("observe.processes").length).toBeGreaterThan(0));
  expect(wire.request("observe.processes", -1).params).toEqual({ projectId: "p" });
  await userEvent.click(screen.getByRole("button", { name: "포트 목록" }));
  expect(await screen.findByRole("heading", { name: "포트 목록" })).toBeVisible();
});
test("saving a definition reads the auto-detected suggestions again", async () => {
  const wire = new RunWire();
  wire.suggestions = [{ id: "build", command: "pnpm build", kind: "task", from: "package.json" }];
  useCurrentProject.getState().set("p");
  const router = setup(wire);
  await act(async () => router.navigate({ to: "/runs", search: {} }));
  await userEvent.click(await screen.findByRole("button", { name: "작업 추가 ▾" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "자동 감지 제안" }));
  await userEvent.click(await screen.findByRole("button", { name: "명령 확인 후 추가" }));
  await waitFor(() => expect(wire.requests("runtime.detect")).toHaveLength(1));
  await userEvent.click(await screen.findByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  const params = wire.request("runtime.define").params as { def: { id: string } };
  expect(params.def.id).toBe("build");
  await act(async () =>
    wire.reply("runtime.define", {
      rev: "2",
      definition: { source: "projectFile", def: { ...serviceDefinition.def, id: "build" } },
    }),
  );
  await waitFor(() => expect(wire.requests("runtime.detect")).toHaveLength(2));
});
test("지난 실행에서 old를 고르면 그 실행을 연다", async () => {
  const wire = new RunWire();
  const latest: Run = { ...runFixture, id: "new", startedMs: 3_000 };
  const old: Run = {
    ...runFixture,
    id: "old",
    state: "exited",
    exit: { attemptId: "attempt-old", code: 0, endedMs: 1_500, by: "wrapper" },
    startedMs: 1_000,
    endedMs: 1_500,
    pid: 222,
  };
  wire.runs = [latest];
  wire.selected = latest;
  // The project's latest runs of every definition; only web's are offered.
  wire.history = [latest, { ...runFixture, id: "api-run", defId: "api", startedMs: 2_000 }, old];
  useCurrentProject.getState().set("p");
  const router = setup(wire);
  await act(async () => router.navigate({ to: "/runs", search: {} }));
  const picker = await screen.findByRole("combobox", { name: "지난 실행" });
  expect(wire.request("runtime.runs").params).toEqual({ projectId: "p", limit: 100 });
  expect(screen.getByText("최근 100개 실행 중")).toBeVisible();
  const options = within(picker).getAllByRole("option");
  expect(options.map((option) => option.getAttribute("value"))).toEqual(["new", "old"]);
  expect(options[1]).toHaveTextContent(/ · 종료 코드 0$/);
  expect(picker).toHaveValue("new");
  wire.selected = old;
  await userEvent.selectOptions(picker, "old");
  await waitFor(() => expect(wire.request("runtime.get", -1).params).toEqual({ runId: "old" }));
  expect(await screen.findByText("PID 222")).toBeVisible();
  expect(screen.getByRole("heading", { name: "web" })).toBeVisible();
  expect(screen.getByRole("combobox", { name: "지난 실행" })).toHaveValue("old");
});
