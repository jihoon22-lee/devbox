import { act, render, screen, waitFor } from "@testing-library/react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectDataProvider } from "../projects/ProjectData";
import { ProjectWire, projects } from "../projects/test-wire";
import { AgentDataProvider } from "./AgentData";
import { AgentList } from "./AgentList";
import { useAgentsData } from "./hooks";

function List() {
  return <AgentList data={useAgentsData()} onSelect={() => {}} header={null} />;
}
async function setup(items: typeof projects) {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <ProjectDataProvider>
        <AgentDataProvider>
          <List />
        </AgentDataProvider>
      </ProjectDataProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.list")).toHaveLength(1));
  await act(async () => {
    wire.reply("projects.list", { rev: "1", items });
    wire.reply("agents.list", { rev: "1", items: [] });
  });
}
test("shows_add_project_when_no_project_is_registered", async () => {
  await setup([]);
  expect(await screen.findByText("등록된 프로젝트가 없습니다")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "프로젝트 추가" })).toBeInTheDocument();
});
test("hides_add_project_when_projects_exist", async () => {
  await setup(projects);
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "프로젝트 추가" })).not.toBeInTheDocument();
});
