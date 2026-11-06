import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { buildRouter } from "../../shell/router";
import { ProjectDataProvider } from "../projects/ProjectData";
import { useCurrentProject } from "../projects/store";
import { ProjectWire, projects } from "../projects/test-wire";
import { RunsPage } from "./RunsPage";
import { runsSearch } from "./search";

async function setup(items: typeof projects) {
  useCurrentProject.setState({ id: undefined });
  const wire = new ProjectWire();
  const router = buildRouter(
    {
      frame: () => <Outlet />,
      pages: { "/runs": { component: RunsPage, validateSearch: runsSearch } },
      settings: () => null,
      popouts: [],
    },
    createMemoryHistory({ initialEntries: ["/runs"] }),
  );
  render(
    <RpcProvider client={wire.client}>
      <ProjectDataProvider>
        <RouterProvider router={router} />
      </ProjectDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items }));
}
test("shows_add_project_when_no_project_is_registered", async () => {
  await setup([]);
  expect(await screen.findByText("등록된 프로젝트가 없습니다")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "프로젝트 추가" })).toBeInTheDocument();
});
test("hides_add_project_when_projects_exist", async () => {
  await setup(projects);
  expect(await screen.findByText("프로젝트를 선택하세요")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "프로젝트 추가" })).not.toBeInTheDocument();
});
