import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { GitControls } from "./GitControls";
import { sourceTargetKey, useSourceGit } from "./gitState";
import { SourceGitSession } from "./SourceGitSession";

const p = { projectId: "p" },
  q = { projectId: "q" };
const remote = {
  branch: "A",
  headOid: "a",
  upstream: { remote: "origin", destinationRef: "refs/heads/A" },
  upstreamOid: "remote-a",
};
afterEach(() => {
  cleanup();
  useSourceGit.setState({ entries: {}, confirmation: undefined });
});
function show() {
  const wire = new NotesWire();
  wire.handlers["git.status"] = () => ({ expect: { state: remote, indexHash: "i", worktreeHash: "w" } });
  const view = (target: typeof p) => (
    <RpcProvider client={wire.client}>
      <SourceGitSession />
      <GitControls target={target} />
    </RpcProvider>
  );
  const rendered = render(view(p));
  return { wire, switchProject: () => rendered.rerender(view(q)) };
}
test("push confirmation survives project switch with exact A expectation and stale refresh requires another click", async () => {
  const { wire, switchProject } = show();
  wire.handlers["git.push"] = () => {
    throw new RpcError("git.state_changed", { current: { ...remote, branch: "B", headOid: "b" } });
  };
  await waitFor(() => expect(screen.getByRole("button", { name: "push…" })).toBeEnabled());
  fireEvent.click(screen.getByRole("checkbox", { name: "force-with-lease" }));
  fireEvent.click(screen.getByRole("button", { name: "push…" }));
  const dialog = await screen.findByRole("dialog", { name: "push 확인" });
  switchProject();
  expect(within(dialog).getByText(/프로젝트 p/)).toBeVisible();
  fireEvent.click(within(dialog).getByRole("button", { name: "확인 후 실행" }));
  await waitFor(() => expect(wire.requests("git.push")).toEqual([{ target: p, expect: remote, forceWithLease: true }]));
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "확인 후 실행" })).toBeDisabled());
  wire.handlers["git.status"] = () => ({
    expect: { state: { ...remote, branch: "B", headOid: "b" }, indexHash: "i2", worktreeHash: "w2" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "새 상태 확인" }));
  await within(dialog).findByText("B");
  expect(wire.requests("git.push")).toHaveLength(1);
  expect(useSourceGit.getState().confirmation?.action.params.target).toEqual(p);
});
test("remote operation events cannot overwrite another target or regress a finished operation", async () => {
  const { wire, switchProject } = show();
  const operation = {
    id: "job",
    target: p,
    state: "running" as const,
    error: null,
    current: null,
    remoteOid: null,
    completionUnknown: false,
  };
  wire.handlers["git.fetch"] = () => operation;
  wire.handlers["git.operation_status"] = () => operation;
  await waitFor(() => expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "fetch" }));
  await screen.findByText("원격 작업 실행 중");
  switchProject();
  await act(async () => {
    wire.emit("git.operation_changed", { ...operation, target: q, state: "succeeded" });
  });
  expect(useSourceGit.getState().entries[sourceTargetKey(p)]?.operation?.state).toBe("running");
  await act(async () => {
    wire.emit("git.operation_changed", { ...operation, state: "cancelled", completionUnknown: true }, "3");
  });
  expect(useSourceGit.getState().entries[sourceTargetKey(p)]?.operation?.completionUnknown).toBe(true);
  await act(async () => {
    wire.emit("git.operation_changed", operation, "4");
  });
  expect(useSourceGit.getState().entries[sourceTargetKey(p)]?.operation?.state).toBe("cancelled");
  expect(screen.queryByText(/원격 작업 취소됨/)).toBeNull();
});
test("cancel reports completion unknown without claiming the remote was unchanged", async () => {
  const { wire } = show();
  const operation = {
    id: "cancel-job",
    target: p,
    state: "running" as const,
    error: null,
    current: null,
    remoteOid: null,
    completionUnknown: false,
  };
  wire.handlers["git.fetch"] = () => operation;
  wire.handlers["git.operation_status"] = () => operation;
  wire.handlers["git.operation_cancel"] = () => ({ ...operation, state: "cancelled", completionUnknown: true });
  await waitFor(() => expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "fetch" }));
  fireEvent.click(await screen.findByRole("button", { name: "원격 작업 취소" }));
  await screen.findByText(/원격 반영 여부를 알 수 없습니다/);
  expect(wire.requests("git.operation_cancel")).toEqual([{ target: p, operationId: "cancel-job" }]);
  expect(screen.queryByRole("button", { name: "원격 작업 취소" })).toBeNull();
});
