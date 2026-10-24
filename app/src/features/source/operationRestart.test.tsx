import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { GitControls } from "./GitControls";
import { sourceTargetKey, useSourceGit } from "./gitState";
import { SourceGitSession } from "./SourceGitSession";

const target = { projectId: "p", worktreeId: "owned" },
  other = { projectId: "q" };
const operation = {
  id: "old-job",
  target,
  state: "running" as const,
  error: null,
  current: null,
  remoteOid: null,
  completionUnknown: false,
};
const status = {
  expect: {
    state: { branch: "main", headOid: "now", upstream: null, upstreamOid: null },
    indexHash: "i",
    worktreeHash: "w",
  },
  summary: { branch: "main" },
  files: [],
};
afterEach(() => {
  cleanup();
  useSourceGit.setState({ entries: {}, confirmation: undefined });
});
async function show(error = "git.invalid_target") {
  const wire = new NotesWire();
  wire.handlers["git.fetch"] = () => operation;
  wire.handlers["git.operation_status"] = () => {
    throw new RpcError(error as "git.invalid_target" | "git.source_failed");
  };
  wire.handlers["git.operation_cancel"] = () => {
    throw new RpcError("git.invalid_target");
  };
  wire.handlers["git.status"] = () => status;
  const view = (selected: typeof target | typeof other) => (
    <RpcProvider client={wire.client}>
      <SourceGitSession />
      <GitControls target={selected} />
    </RpcProvider>
  );
  const rendered = render(view(target));
  await waitFor(() => expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "fetch" }));
  await waitFor(() => expect(wire.requests("git.operation_status")).toHaveLength(1));
  await waitFor(() => {
    const entry = useSourceGit.getState().entries[sourceTargetKey(target)];
    expect(entry?.error || entry?.lostOperation).toBeTruthy();
  });
  return { wire, select: (selected: typeof target | typeof other) => rendered.rerender(view(selected)) };
}
test("missing operation stays blocked until same-target read and explicit acknowledgement, never replaying", async () => {
  const { wire, select } = await show();
  const inspect = await screen.findByRole("button", { name: "현재 Git 상태 읽기" });
  expect(screen.getByRole("button", { name: "fetch" })).toBeDisabled();
  expect(screen.queryByRole("button", { name: "같은 요청 결과 다시 확인" })).toBeNull();
  expect(screen.queryByRole("button", { name: "원격 작업 취소" })).toBeNull();
  expect(screen.queryByText("원격 작업 완료")).toBeNull();
  fireEvent.click(inspect);
  await screen.findByRole("button", { name: "이 상태를 확인하고 새 작업 허용" });
  expect(wire.requests("git.status")).toEqual([{ target }]);
  expect(screen.getByRole("button", { name: "fetch" })).toBeDisabled();
  select(other);
  expect(screen.queryByRole("button", { name: "이 상태를 확인하고 새 작업 허용" })).toBeNull();
  expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled();
  select(target);
  fireEvent.click(screen.getByRole("button", { name: "이 상태를 확인하고 새 작업 허용" }));
  expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled();
  await act(async () => wire.emit("git.operation_changed", { ...operation, state: "succeeded" }));
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.operation).toBeUndefined();
  expect(wire.requests("git.fetch")).toHaveLength(1);
  expect(wire.requests("git.operation_cancel")).toHaveLength(0);
});
test("temporary polling error remains running and cancel missing-ID opens reconciliation", async () => {
  const { wire } = await show("git.source_failed");
  expect(screen.queryByRole("button", { name: "현재 Git 상태 읽기" })).toBeNull();
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.operation?.completionUnknown).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "원격 작업 취소" }));
  await screen.findByRole("button", { name: "현재 Git 상태 읽기" });
  expect(wire.requests("git.operation_cancel")).toEqual([{ target, operationId: operation.id }]);
  expect(screen.getByRole("button", { name: "fetch" })).toBeDisabled();
});
test("a late terminal event can resolve a transient poll failure without a replay", async () => {
  const { wire } = await show("git.source_failed");
  await act(async () => wire.emit("git.operation_changed", { ...operation, state: "succeeded" }));
  expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled();
  expect(screen.getByText("원격 작업 완료")).toBeVisible();
  expect(wire.requests("git.fetch")).toHaveLength(1);
});
test("changed daemon cannot complete the old operation with a reused ID", async () => {
  const { wire } = await show("git.source_failed");
  const deliver = wire.deliver.bind(wire);
  wire.deliver = (message: unknown) => {
    if (message && typeof message === "object" && "type" in message && message.type === "welcome")
      deliver({ ...message, daemonId: "replacement" });
    else deliver(message);
  };
  await act(async () => {
    wire.close();
    wire.connect();
  });
  await screen.findByRole("button", { name: "현재 Git 상태 읽기" });
  await act(async () => wire.emit("git.operation_changed", { ...operation, state: "succeeded" }));
  expect(screen.getByRole("button", { name: "fetch" })).toBeDisabled();
  expect(screen.queryByText("원격 작업 완료")).toBeNull();
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.operationDaemonId).toBe("notes");
  expect(wire.requests("git.fetch")).toHaveLength(1);
});
test("temporary status failure polls again and accepts its matching terminal result", async () => {
  const { wire } = await show("git.source_failed");
  wire.handlers["git.operation_status"] = () => ({ ...operation, state: "succeeded" });
  await waitFor(() => expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled(), { timeout: 3500 });
  expect(wire.requests("git.operation_status").length).toBeGreaterThan(1);
  expect(screen.getByText("원격 작업 완료")).toBeVisible();
  expect(screen.queryByRole("button", { name: "현재 Git 상태 읽기" })).toBeNull();
});
test("late matching terminal evidence invalidates an earlier read and still requires acknowledgement", async () => {
  const { wire } = await show();
  fireEvent.click(await screen.findByRole("button", { name: "현재 Git 상태 읽기" }));
  await screen.findByRole("button", { name: "이 상태를 확인하고 새 작업 허용" });
  await act(async () => wire.emit("git.operation_changed", { ...operation, state: "succeeded" }));
  expect(screen.getByText("원격 작업 완료")).toBeVisible();
  expect(screen.getByRole("button", { name: "fetch" })).toBeDisabled();
  expect(screen.queryByRole("button", { name: "이 상태를 확인하고 새 작업 허용" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "현재 Git 상태 읽기" }));
  fireEvent.click(await screen.findByRole("button", { name: "이 상태를 확인하고 새 작업 허용" }));
  expect(screen.getByRole("button", { name: "fetch" })).toBeEnabled();
  expect(wire.requests("git.fetch")).toHaveLength(1);
  expect(wire.requests("git.status")).toHaveLength(2);
});
