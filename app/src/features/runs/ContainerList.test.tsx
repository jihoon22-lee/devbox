import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ContainerInfo } from "../../rpc/gen/rpc";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ContainerList } from "./ContainerList";
import { ProjectRuns } from "./RunsPage";
import { RunWire } from "./test-wire";

const container = (name: string, id: string, projectId?: string): ContainerInfo => ({
  id: id.repeat(64),
  name,
  image: "fake",
  state: "running",
  status: "Up",
  ports: "",
  ...(projectId ? { projectId } : {}),
});
function show(containers: ContainerInfo[], projectsUnavailable = false) {
  const wire = new RunWire();
  wire.containers = containers;
  wire.projectsUnavailable = projectsUnavailable;
  render(
    <RpcProvider client={wire.client}>
      <ProjectRuns projectId="p" />
    </RpcProvider>,
  );
  return wire;
}

test("the current project's containers come first and the rest wait in a collapsed group", async () => {
  const wire = show([
    container("other-db-1", "a", "q"),
    container("app-web-1", "b", "p"),
    container("tool-1", "c"),
    container("app-db-1", "d", "p"),
  ]);
  const section = await screen.findByRole("region", { name: "컨테이너" });
  const mine = await within(section).findByRole("table", { name: "컨테이너 목록" });
  expect(within(mine).getByRole("button", { name: "app-web-1 로그" })).toBeVisible();
  expect(within(mine).getByRole("button", { name: "app-db-1 로그" })).toBeVisible();
  expect(within(mine).queryByText("other-db-1")).not.toBeInTheDocument();
  expect(within(mine).queryByText("tool-1")).not.toBeInTheDocument();
  expect(within(section).getByRole("heading", { name: "컨테이너 2" })).toBeVisible();

  const summary = within(section).getByText("다른 컨테이너 2개", { selector: "summary" });
  const group = summary.closest("details");
  expect(group).not.toHaveAttribute("open");
  // Still in the page, so nothing is lost: only folded away until asked for.
  expect(within(section).getByText("other-db-1")).not.toBeVisible();
  expect(within(section).getByText("tool-1")).not.toBeVisible();

  await userEvent.click(summary);
  expect(group).toHaveAttribute("open");
  const others = within(section).getByRole("table", { name: "다른 컨테이너 목록" });
  expect(within(others).getByRole("button", { name: "other-db-1 로그" })).toBeVisible();
  expect(within(others).getByRole("button", { name: "tool-1 로그" })).toBeVisible();
  expect(within(others).queryByText("app-web-1")).not.toBeInTheDocument();

  // A container of the folded group is still a full row: its log opens and its actions work.
  await userEvent.click(within(others).getByRole("button", { name: "other-db-1 로그" }));
  expect(await screen.findByRole("heading", { name: "other-db-1" })).toBeVisible();
  await userEvent.click(within(others).getByRole("button", { name: "tool-1 재시작" }));
  await waitFor(() => expect(wire.requests("runtime.container_action")).toHaveLength(1));
  expect(wire.request("runtime.container_action").params).toEqual({ id: "c".repeat(64), action: "restart" });
});

test("a project without containers shows its empty state and keeps the others folded", async () => {
  show([container("other-db-1", "a", "q"), container("tool-1", "c")]);
  const section = await screen.findByRole("region", { name: "컨테이너" });
  const summary = await within(section).findByText("다른 컨테이너 2개", { selector: "summary" });
  expect(within(section).getByRole("heading", { name: "컨테이너 0" })).toBeVisible();
  expect(within(section).getByText("현재 프로젝트의 컨테이너 없음")).toBeVisible();
  expect(within(section).queryByRole("table", { name: "컨테이너 목록" })).not.toBeInTheDocument();
  expect(summary.closest("details")).not.toHaveAttribute("open");
  expect(within(section).getByText("other-db-1")).not.toBeVisible();

  await userEvent.click(summary);
  const others = within(section).getByRole("table", { name: "다른 컨테이너 목록" });
  expect(within(others).getByRole("button", { name: "other-db-1 로그" })).toBeVisible();
  expect(within(others).getByRole("button", { name: "tool-1 로그" })).toBeVisible();
});

test("a machine without containers keeps the plain empty state", async () => {
  const wire = show([]);
  const section = await screen.findByRole("region", { name: "컨테이너" });
  await waitFor(() => expect(wire.requests("runtime.containers")).not.toHaveLength(0));
  expect(await within(section).findByText("컨테이너 없음")).toBeVisible();
  expect(within(section).getByRole("heading", { name: "컨테이너 0" })).toBeVisible();
  expect(within(section).queryByText(/다른 컨테이너/)).not.toBeInTheDocument();
  expect(within(section).queryByText("현재 프로젝트의 컨테이너 없음")).not.toBeInTheDocument();
});

test("a project that owns every container shows no folded group", async () => {
  show([container("app-web-1", "b", "p")]);
  const section = await screen.findByRole("region", { name: "컨테이너" });
  expect(await within(section).findByRole("button", { name: "app-web-1 로그" })).toBeVisible();
  expect(within(section).queryByText(/다른 컨테이너/)).not.toBeInTheDocument();
});

test("a failed project lookup keeps every container visible without claiming the current project has none", async () => {
  show([container("tool-1", "c")], true);
  expect(await screen.findByText(/프로젝트 목록을 읽지 못해/)).toBeVisible();
  expect(screen.getByRole("button", { name: "tool-1 로그" })).toBeVisible();
  expect(screen.queryByText("현재 프로젝트의 컨테이너 없음")).not.toBeInTheDocument();
  expect(screen.queryByText(/다른 컨테이너/)).not.toBeInTheDocument();
});

test("an expanded group of other containers stays open across a refreshed listing", async () => {
  const rows = (items: ContainerInfo[]) => (
    <ContainerList
      result={{ available: true, items, projectsUnavailable: false }}
      projectId="p"
      busy={false}
      onSelect={() => {}}
      onAction={() => {}}
    />
  );
  const initial = container("other-db", "a", "q");
  const { rerender } = render(rows([initial]));
  await userEvent.click(screen.getByText("다른 컨테이너 1개", { selector: "summary" }));
  rerender(rows([initial, container("new-db", "b", "q")]));
  expect(screen.getByRole("button", { name: "new-db 로그" })).toBeVisible();
});
