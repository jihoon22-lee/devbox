import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { RpcProvider, useRpcSession } from "../../../rpc/RpcProvider";
import { TooltipProvider } from "../../../ui";
import { useCurrentProject } from "../../projects/store";
import { ProjectWire } from "../../projects/test-wire";
import { task } from "../test-fixtures";
import { AgentActions } from "./AgentActions";

beforeAll(() =>
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  }),
);
afterEach(() => useCurrentProject.setState({ id: undefined }));
function Fixture({ wire, visible }: { wire: ProjectWire; visible: boolean }) {
  return (
    <TooltipProvider>
      <RpcProvider client={wire.client}>
        <CachedStatus>
          {visible && <AgentActions task={{ ...task("a1", "review"), toolExited: true }} sessionAlive={false} />}
        </CachedStatus>
      </RpcProvider>
    </TooltipProvider>
  );
}
function CachedStatus({ children }: { children: ReactNode }) {
  const { qc } = useRpcSession();
  return (
    <>
      <button
        type="button"
        onClick={() => qc.setQueryData(["agents.rebase_status", { id: "a1" }], { owned: true, files: ["old.txt"] })}
      >
        캐시 준비
      </button>
      {children}
    </>
  );
}
test("owned conflict controls return after remount from fresh daemon state", async () => {
  const wire = new ProjectWire();
  const view = render(<Fixture wire={wire} visible />);
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase_status", { owned: false, files: [] }));
  await userEvent.click(screen.getByRole("button", { name: "기준 반영" }));
  await waitFor(() => expect(wire.requests("agents.rebase")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase", { result: "conflict", files: ["a.txt"] }));
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(2));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["a.txt"] }));
  expect(await screen.findByRole("button", { name: "되돌리기" })).toBeInTheDocument();
  view.rerender(<Fixture wire={wire} visible={false} />);
  await userEvent.click(screen.getByRole("button", { name: "캐시 준비" }));
  view.rerender(<Fixture wire={wire} visible />);
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(3));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["a.txt"] }));
  expect(await screen.findByRole("button", { name: "되돌리기" })).toBeInTheDocument();
  expect(screen.getByText(/에이전트를 정지했습니다/)).toBeInTheDocument();
  expect(wire.requests("agents.changes")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() => expect(wire.requests("agents.rebase_abort")).toHaveLength(1));
  expect(wire.request("agents.rebase_abort").params).toEqual({ id: "a1" });
});
test("cached owned recovery is not revived when a fresh remount reports no owned rebase", async () => {
  const wire = new ProjectWire();
  const view = render(<Fixture wire={wire} visible />);
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["old.txt"] }));
  expect(await screen.findByRole("button", { name: "되돌리기" })).toBeInTheDocument();
  view.rerender(<Fixture wire={wire} visible={false} />);
  view.rerender(<Fixture wire={wire} visible />);
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(2));
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  await act(async () => wire.reply("agents.rebase_status", { owned: false, files: ["foreign.txt"] }));
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  expect(screen.queryByRole("button", { name: "해결 지시 보내기" })).toBeNull();
  expect(screen.queryByText(/old.txt|foreign.txt/)).toBeNull();
  expect(wire.requests("agents.changes")).toHaveLength(0);
});
test("owned rebase preview and apply keep the agent worktree when the source project differs", async () => {
  useCurrentProject.setState({ id: "unrelated-source-project" });
  const wire = new ProjectWire();
  render(<Fixture wire={wire} visible />);
  await waitFor(() => expect(wire.requests("agents.rebase_status")).toHaveLength(1));
  await act(async () => wire.reply("agents.rebase_status", { owned: true, files: ["a.txt"] }));
  await userEvent.click(await screen.findByRole("button", { name: "충돌 해결" }));
  await waitFor(() => expect(wire.requests("git.conflicts")).toHaveLength(1));
  expect(wire.request("git.conflicts").params).toEqual({ target: { projectId: "a", worktreeId: "a1" } });
  await waitFor(() => expect(wire.requests("git.conflict_preview")).toHaveLength(1));
  expect(wire.request("git.conflict_preview").params).toEqual({
    target: { projectId: "a", worktreeId: "a1" },
    path: "a.txt",
  });
  const target = { projectId: "a", worktreeId: "a1" };
  const token = {
    target,
    path: "a.txt",
    worktreeHash: "w",
    stageOids: ["base", "ours", "theirs"],
    identity: "owned-rebase",
  };
  await act(async () =>
    wire.reply("git.conflict_preview", {
      base: "base",
      ours: "ours",
      theirs: "theirs",
      merged: "resolution",
      textSupported: true,
      labels: { base: "base", ours: "ours", theirs: "theirs", rebase: true },
      token,
    }),
  );
  await screen.findByRole("textbox", { name: "해결안" });
  act(() => useCurrentProject.setState({ id: "changed-source-project" }));
  await userEvent.click(screen.getByRole("button", { name: "해결안 적용" }));
  await waitFor(() => expect(wire.requests("git.conflict_apply")).toHaveLength(1));
  expect(wire.request("git.conflict_apply").params).toEqual({ target, path: "a.txt", resolution: "resolution", token });
});
