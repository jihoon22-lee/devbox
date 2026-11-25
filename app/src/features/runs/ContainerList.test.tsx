import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ContainerInfo } from "../../rpc/gen/rpc";
import { RpcProvider } from "../../rpc/RpcProvider";
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
function show(containers: ContainerInfo[]) {
  const wire = new RunWire();
  wire.containers = containers;
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
  expect(within(section).getByText("이 프로젝트의 컨테이너 없음")).toBeVisible();
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
  expect(within(section).queryByText("이 프로젝트의 컨테이너 없음")).not.toBeInTheDocument();
});

test("a project that owns every container shows no folded group", async () => {
  show([container("app-web-1", "b", "p")]);
  const section = await screen.findByRole("region", { name: "컨테이너" });
  expect(await within(section).findByRole("button", { name: "app-web-1 로그" })).toBeVisible();
  expect(within(section).queryByText(/다른 컨테이너/)).not.toBeInTheDocument();
});
