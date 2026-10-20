import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DefineRun, DefinitionList } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { DefineDialog } from "./DefineDialog";

const emptyList: DefinitionList = {
  rev: "1",
  defs: [],
  shadowed: [],
  runs: [],
  trust: "not_required",
  nextCursor: null,
};

async function openDialog() {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <DefineDialog projectId="project-1" onClose={() => {}} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(1));
  await act(async () => wire.reply("runtime.list", emptyList));
  await userEvent.type(screen.getByRole("textbox", { name: "명령" }), "echo hello");
  return wire;
}

test("define_success_then_list_failure_keeps_edit_mode_and_does_not_create_twice", async () => {
  const wire = await openDialog();
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  const first = wire.request("runtime.define");
  const params = first.params as DefineRun;
  expect(params.def.id).toBeTruthy();
  await act(async () =>
    wire.reply("runtime.define", {
      rev: "2",
      definition: { source: "thisPc", def: params.def },
    }),
  );
  expect(await screen.findByRole("heading", { name: "작업 수정" })).toBeVisible();
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(2));
  await act(async () => wire.fail("runtime.list"));
  expect(await screen.findByRole("alert")).toHaveTextContent("목록");
  expect(screen.getByRole("button", { name: "다시 불러오기" })).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(2));
  const second = wire.request("runtime.define", 1);
  expect((second.params as DefineRun).def.id).toBe(params.def.id);
  expect(second.clientRequestId).not.toBe(first.clientRequestId);
  expect(new Set(wire.requests("runtime.define").map((request) => (request.params as DefineRun).def.id)).size).toBe(1);
});

test("ambiguous define retry keeps the same definition and request identities", async () => {
  const wire = await openDialog();
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  const first = wire.request("runtime.define");
  await act(async () => wire.close());
  await screen.findByRole("alert");
  await act(async () => wire.connect());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(2));
  const retry = wire.request("runtime.define", 1);
  expect(retry.params).toEqual(first.params);
  expect(retry.clientRequestId).toBe(first.clientRequestId);
});

test("refresh retry repairs the list without another mutation or losing edit mode", async () => {
  const wire = await openDialog();
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  const params = wire.request("runtime.define").params as DefineRun;
  const definition = { source: "thisPc" as const, def: params.def };
  await act(async () => wire.reply("runtime.define", { rev: "2", definition }));
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(2));
  await act(async () => wire.fail("runtime.list"));
  await userEvent.click(await screen.findByRole("button", { name: "다시 불러오기" }));
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(3));
  await act(async () => wire.reply("runtime.list", { ...emptyList, rev: "2", defs: [definition] }));
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  expect(screen.getByRole("heading", { name: "작업 수정" })).toBeVisible();
  expect(wire.requests("runtime.define")).toHaveLength(1);
});

test("editing a failed request starts a new intent while retaining its definition identity", async () => {
  const wire = await openDialog();
  const name = screen.getByRole("textbox", { name: "작업 이름 (식별자)" });
  await userEvent.clear(name);
  await userEvent.type(name, "hello");
  await userEvent.dblClick(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  const first = wire.request("runtime.define");
  expect(screen.getByRole("textbox", { name: "명령" })).toBeDisabled();
  await act(async () => wire.close());
  await screen.findByRole("alert");
  expect(name).toHaveAttribute("readonly");
  expect((first.params as DefineRun).def.id).toBe("hello");
  await userEvent.type(screen.getByRole("textbox", { name: "명령" }), " world");
  await act(async () => wire.connect());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(2));
  const second = wire.request("runtime.define", 1);
  expect(second.clientRequestId).not.toBe(first.clientRequestId);
  expect((second.params as DefineRun).def.id).toBe((first.params as DefineRun).def.id);
});

test("existing task preserves its settings when changing save source", async () => {
  const wire = new ProjectWire();
  const def: DefineRun["def"] = {
    kind: "task",
    id: "build",
    command: "pnpm build",
    cwd: "app",
    worktree: "/fixture/tree",
    target: "windows",
    env: { MODE: "production" },
    secrets: ["TOKEN"],
    dependsOn: ["prepare"],
  };
  render(
    <RpcProvider client={wire.client}>
      <DefineDialog projectId="project-1" definition={{ source: "thisPc", def }} onClose={() => {}} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(1));
  await act(async () => wire.reply("runtime.list", emptyList));
  expect(screen.getByRole("textbox", { name: "작업 이름 (식별자)" })).toHaveAttribute("readonly");
  await userEvent.selectOptions(screen.getByRole("combobox", { name: "저장 위치" }), "project");
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  expect(wire.request("runtime.define").params).toEqual({ projectId: "project-1", def, saveTo: "project" });
});

function TrustProbe() {
  const list = useRpcQuery("runtime.list", { projectId: "project-1", cursor: null });
  return <output aria-label="목록 신뢰 상태">{list.data?.trust}</output>;
}

test("authoritative same-revision refresh updates trust after a successful project save", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <DefineDialog projectId="project-1" onClose={() => {}} />
      <TrustProbe />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(1));
  await act(async () => wire.reply("runtime.list", { ...emptyList, trust: "approval_required" }));
  await userEvent.type(screen.getByRole("textbox", { name: "명령" }), "echo hello");
  await userEvent.selectOptions(screen.getByRole("combobox", { name: "저장 위치" }), "project");
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await waitFor(() => expect(wire.requests("runtime.define")).toHaveLength(1));
  const params = wire.request("runtime.define").params as DefineRun;
  const definition = { source: "projectFile" as const, def: params.def };
  await act(async () => wire.reply("runtime.define", { rev: "2", definition }));
  await waitFor(() => expect(wire.requests("runtime.list")).toHaveLength(2));
  expect(screen.getByLabelText("목록 신뢰 상태")).toHaveTextContent("approval_required");
  await act(async () => wire.reply("runtime.list", { ...emptyList, rev: "2", defs: [definition], trust: "approved" }));
  await waitFor(() => expect(screen.getByLabelText("목록 신뢰 상태")).toHaveTextContent(/^approved$/));
  expect(screen.getByRole("heading", { name: "작업 수정" })).toBeVisible();
});
