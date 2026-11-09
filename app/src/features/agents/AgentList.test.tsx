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
// [프로젝트 추가] and [새 작업] are the body's actions (AgentsPage.test.tsx), so they stay in view with the list collapsed.
test.each([
  ["no project is registered", [], "프로젝트를 추가한 뒤 새 작업을 만들 수 있습니다."],
  ["projects exist", projects, "새 작업으로 도구와 지시문을 선택하세요."],
])("the empty list keeps its title and leaves the action to the body: %s", async (_, items, body) => {
  await setup(items);
  expect(await screen.findByText("에이전트 작업이 없습니다")).toBeInTheDocument();
  expect(screen.getByText(body)).toBeInTheDocument();
  expect(screen.queryByRole("button")).toBeNull();
});
