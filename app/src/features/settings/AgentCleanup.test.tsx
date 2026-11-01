import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useCurrentProject } from "../projects/store";
import { ProjectWire } from "../projects/test-wire";
import { AgentCleanup } from "./AgentCleanup";

const WORKTREE = "/fixture/owned";
const BRANCH = "agent/closed";

/** Shows the list for one project, with `candidates` as the daemon offers them. */
async function showList(candidates: Record<string, unknown>) {
  const wire = new ProjectWire({ trustNotRequired: true });
  useCurrentProject.getState().set("owned");
  render(
    <RpcProvider client={wire.client}>
      <AgentCleanup projectId="owned" />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("agents.cleanup_candidates")).toHaveLength(1));
  await act(async () => wire.reply("agents.cleanup_candidates", candidates));
  return wire;
}

/** Shows the list for one project, picks its folder and branch and opens the confirmation. */
async function confirmCleanup(candidates: Record<string, unknown> = { worktrees: [WORKTREE], branches: [BRANCH] }) {
  const wire = await showList(candidates);
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
  // Only what comes back on a retry is the teardown's to fix: the undo leaves what it does not
  // take, such as a nested repository, and the teardown makes it again on every retry.
  expect(alert).toHaveTextContent(
    "작업 폴더에 devbox가 지우지 않는 변경이 남아 있습니다. 남은 파일은 옮기거나 지우고 바뀐 파일은 되돌린 뒤(커밋하지 말고) 다시 정리하세요. 다시 정리해도 같은 것이 또 생기면 정리 명령부터 고치세요.",
  );
  expect(alert).toHaveTextContent("변경 파일: notes.txt, build/out.bin");
  expect(alert).not.toHaveTextContent("커밋한 뒤");
  // This line is in the list already.
  expect(alert).not.toHaveTextContent("정리 후보에서");
});

test("a cleanup refused for an unfinished merge or rebase says to abort it in the work folder, not finish it", async () => {
  const alert = await refuse({ code: "agents.operation_in_progress" });
  // Finishing it commits, which moves the folder off the head the task closed with.
  expect(alert).toHaveTextContent(
    "작업 폴더에 끝나지 않은 Git 작업(병합·rebase·cherry-pick 등)이 있습니다. 작업 폴더에서 그 작업을 마치지 말고 중단한 뒤(예: git merge --abort, git rebase --abort) 다시 정리하세요. 마치면 커밋이 생겨 이 작업 폴더는 다시 정리 후보에 나오지 않습니다.",
  );
  expect(alert).not.toHaveTextContent("다른 Git 작업");
  expect(alert).not.toHaveTextContent("정리 후보에서");
});

test("a cleanup stopped by a failed teardown names which command to run in the work folder, and its exit code", async () => {
  const alert = await refuse({ code: "agents.teardown_failed", detail: { index: 2, exit_code: 3 } }, "diag-7");
  expect(alert).toHaveTextContent(
    "정리 명령이 실패했습니다(종료 코드 3). .devbox/devbox.toml의 [agent] teardown 2번째 명령을 작업 폴더에서 직접 실행해 원인을 고친 뒤 다시 정리하세요. · 진단 diag-7",
  );
  expect(alert).not.toHaveTextContent("정리 후보에서");
});

const SUBMODULES = `하위 모듈이 있는 작업 폴더는 devbox가 지우지 않습니다. 하위 모듈의 변경은 커밋하고 커밋은 push한 뒤 프로젝트 폴더의 터미널에서 git worktree remove --force '${WORKTREE}'로 직접 지우세요.`;

test("a work folder with submodules is listed apart with how to remove it, and cannot be selected", async () => {
  await showList({ worktrees: [], branches: [], submodules: [WORKTREE] });
  const kept = await screen.findByRole("region", { name: "직접 지울 작업 폴더" });
  expect(kept).toHaveTextContent(`${WORKTREE} ${SUBMODULES}`);
  // Git removes it only with force, which devbox never uses: it is not among the safe items.
  expect(within(kept).queryByRole("checkbox")).not.toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: WORKTREE })).not.toBeInTheDocument();
  expect(screen.getByText("정리할 항목이 없습니다")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "선택 항목 정리" })).toBeDisabled();
});

test("a cleanup refused for submodules gives the command that removes the folder", async () => {
  const alert = await refuse({ code: "agents.worktree_has_submodules", detail: { path: WORKTREE } });
  expect(alert).toHaveTextContent(SUBMODULES);
});

const OFF_BASE = "스쿼시·PR로 닫힘 — 브랜치의 개별 커밋은 기준 브랜치에 없습니다";
const MIXED = {
  worktrees: [WORKTREE, "/fixture/merged"],
  branches: [BRANCH, "agent/merged"],
  offBase: [WORKTREE, BRANCH],
};

test("the folder and branch of a task closed by squash or PR say the branch's own commits are not on the base branch", async () => {
  await showList(MIXED);
  // Only the items the daemon marks; the ones merged as usual have no such note.
  expect(await screen.findByRole("checkbox", { name: WORKTREE })).toHaveAccessibleDescription(OFF_BASE);
  expect(screen.getByRole("checkbox", { name: BRANCH })).toHaveAccessibleDescription(OFF_BASE);
  expect(screen.getByRole("checkbox", { name: "/fixture/merged" })).not.toHaveAccessibleDescription();
  expect(screen.getByRole("checkbox", { name: "agent/merged" })).not.toHaveAccessibleDescription();
});

test("the confirmation repeats that note for each such item it is about to delete", async () => {
  const { dialog } = await confirmCleanup(MIXED);
  expect(within(dialog).getAllByText(OFF_BASE)).toHaveLength(2);
});
