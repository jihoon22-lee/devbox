import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { AddProjectAction, NoProjectState } from "./AddProjectAction";
import { ProjectWire, projects } from "./test-wire";

function setup(onAdded?: (id: string) => void) {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <AddProjectAction onAdded={onAdded} />
    </RpcProvider>,
  );
  return wire;
}
test("add_project_action_opens_the_add_dialog", async () => {
  setup();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "프로젝트 추가" }));
  expect(await screen.findByRole("dialog", { name: "프로젝트 추가" })).toBeInTheDocument();
});
test("add_project_action_reports_the_added_project_id", async () => {
  const onAdded = vi.fn();
  const wire = setup(onAdded);
  await userEvent.click(screen.getByRole("button", { name: "프로젝트 추가" }));
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  await act(async () => wire.reply("projects.scan", { candidates: [] }));
  await userEvent.type(await screen.findByLabelText("폴더 경로"), "/fixture/devbox");
  await userEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
  await act(async () => wire.reply("projects.add", projects[0]));
  await waitFor(() => expect(onAdded).toHaveBeenCalledWith("a"));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
});
test("add_project_is_hidden_when_the_project_list_fails_to_load", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <NoProjectState body="본문" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.list")).toHaveLength(1));
  await act(async () => wire.fail("projects.list"));
  expect(screen.getByText("프로젝트를 선택하세요")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "프로젝트 추가" })).not.toBeInTheDocument();
});
