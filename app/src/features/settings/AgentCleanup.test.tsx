import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { AgentCleanup } from "./AgentCleanup";

const WORKTREE = "/fixture/owned";
const BRANCH = "agent/closed";

/** Shows the list for one project, picks its folder and branch and opens the confirmation. */
async function confirmCleanup(candidates: Record<string, unknown> = { worktrees: [WORKTREE], branches: [BRANCH] }) {
  const wire = new ProjectWire({ trustNotRequired: true });
  useCurrentProject.getState().set("owned");
  render(
    <RpcProvider client={wire.client}>
      <AgentCleanup projectId="owned" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.cleanup_candidates")).toHaveLength(1));
  await act(async () => wire.reply("agents.cleanup_candidates", candidates));
  await userEvent.click(await screen.findByRole("checkbox", { name: WORKTREE }));
  await userEvent.click(screen.getByRole("checkbox", { name: BRANCH }));
  await userEvent.click(screen.getByRole("button", { name: "선택 항목 정리" }));
  await waitFor(() => expect(wire.requests("agents.cleanup_candidates")).toHaveLength(2));
  await act(async () => wire.reply("agents.cleanup_candidates", candidates));
  const dialog = await screen.findByRole("dialog", { name: "선택 항목 정리" });
  return { wire, dialog };
}

/** Confirms, and the daemon refuses the cleanup with `error`. */
async function refuse(error: { code: string; detail?: unknown }, diagnosticId?: string) {
  const { wire, dialog } = await confirmCleanup();
  await userEvent.click(within(dialog).getByRole("button", { name: "정리" }));
  await waitFor(() => expect(wire.requests("agents.cleanup")).toHaveLength(1));
  await act(async () => wire.deliver({ type: "response", id: wire.request("agents.cleanup").id, error, diagnosticId }));
  return within(dialog).findByRole("alert");
}

test("a cleanup stopped by changes it will not delete says to clear them, not commit them, and names the files", async () => {
  const alert = await refuse({ code: "agents.worktree_dirty", detail: { files: ["notes.txt", "build/out.bin"] } });
  // After its own undo, what stops cleanup are changes devbox does not delete. A commit would move
  // the folder off the head the task closed with and take it out of this list for good.
  expect(alert).toHaveTextContent(
    "작업 폴더에 devbox가 지우지 않는 변경이 남아 있습니다. 남은 파일은 옮기거나 지우고 바뀐 파일은 되돌린 뒤(커밋하지 말고) 정리 후보에서 다시 정리하세요.",
  );
  expect(alert).toHaveTextContent("변경 파일: notes.txt, build/out.bin");
  expect(alert).not.toHaveTextContent("커밋한 뒤");
});
