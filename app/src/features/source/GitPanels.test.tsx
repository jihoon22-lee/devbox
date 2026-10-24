import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { BranchList } from "./BranchList";
import { CleanupPanel } from "./CleanupPanel";
import { CommitBox } from "./CommitBox";
import { useSourceGit } from "./gitState";
import { SourceGitSession } from "./SourceGitSession";

const target = { projectId: "p" };
const expectState = {
  state: { branch: "main", headOid: "head", upstream: null, upstreamOid: null },
  indexHash: "index",
  worktreeHash: "work",
};
const status = {
  expect: expectState,
  files: [],
  warnings: [],
  summary: { branch: "main", ahead: 0, behind: 0, staged: 1, unstaged: 0, untracked: 0, conflicted: [], clean: false },
};
afterEach(() => {
  cleanup();
  useSourceGit.setState({ entries: {}, confirmation: undefined });
});
function show(panel: "branches" | "cleanup" | "commit") {
  const wire = new NotesWire();
  wire.handlers["git.status"] = () => status;
  wire.handlers["git.branches"] = () => [
    { name: "main", oid: "head", current: true, remote: false, upstream: null },
    { name: "topic", oid: "topic-oid", current: false, remote: false, upstream: null },
    { name: "origin/main", oid: "remote", current: false, remote: true, upstream: null },
  ];
  wire.handlers["git.worktrees"] = () => [
    { path: "/owned", headOid: "head", branch: "main", locked: false, prunable: false, main: true },
  ];
  wire.handlers["git.stash_list"] = () => [];
  wire.handlers["git.cleanup_candidates"] = () => [{ branch: "agent/done", path: null, oid: "candidate" }];
  wire.handlers["git.checkout"] = () => ({});
  wire.handlers["git.commit"] = () => ({});
  wire.handlers["git.cleanup"] = () => {
    throw new RpcError("git.state_changed", { current: expectState.state });
  };
  render(
    <RpcProvider client={wire.client}>
      <SourceGitSession />
      {panel === "branches" ? (
        <BranchList target={target} />
      ) : panel === "cleanup" ? (
        <CleanupPanel target={target} />
      ) : (
        <CommitBox target={target} />
      )}
    </RpcProvider>,
  );
  return wire;
}
test("branch create checkout captures the selected repository expectation and remote branches are distinct", async () => {
  const wire = show("branches");
  await screen.findByText("origin/main");
  fireEvent.change(screen.getByRole("textbox", { name: "새 브랜치 이름" }), { target: { value: "feature" } });
  fireEvent.click(screen.getByRole("button", { name: "만들고 전환" }));
  await waitFor(() =>
    expect(wire.requests("git.checkout")).toEqual([{ target, branch: "feature", create: true, expect: expectState }]),
  );
  expect(screen.getByRole("button", { name: "main 삭제" })).toBeDisabled();
});
test("cleanup only selects supplied candidates and retains stale failure until explicit snapshot reconfirmation", async () => {
  const wire = show("cleanup");
  expect(await screen.findByRole("checkbox", { name: /agent\/done/ })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: /main/ })).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox", { name: /agent\/done/ }));
  fireEvent.click(screen.getByRole("button", { name: "선택한 대상 정리" }));
  const dialog = await screen.findByRole("dialog", { name: "작업 폴더 정리 확인" });
  fireEvent.click(within(dialog).getByRole("button", { name: "확인 후 실행" }));
  await waitFor(() => expect(wire.requests("git.cleanup")).toHaveLength(1));
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "확인 후 실행" })).toBeDisabled());
  expect(within(dialog).getByText(/candidate/)).toBeVisible();
  wire.handlers["git.cleanup_candidates"] = () => [{ branch: "agent/done", path: null, oid: "fresh-candidate" }];
  fireEvent.click(within(dialog).getByRole("button", { name: "새 상태 확인" }));
  await within(dialog).findByText(/fresh-candidate/);
  expect(wire.requests("git.cleanup")).toHaveLength(1);
});
test("commit Ctrl+Enter warns at 72 characters and after success does not offer an accidental duplicate", async () => {
  const wire = show("commit");
  const input = screen.getByRole("textbox", { name: "커밋 메시지" });
  fireEvent.change(input, { target: { value: "x".repeat(73) } });
  expect(screen.getByText(/72자/)).toBeVisible();
  await waitFor(() => expect(screen.getByRole("button", { name: "커밋" })).toBeEnabled());
  fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
  await waitFor(() => expect(wire.requests("git.commit")).toHaveLength(1));
  await screen.findByText("커밋을 완료했습니다.");
  expect(screen.getByRole("button", { name: "커밋" })).toBeDisabled();
});
