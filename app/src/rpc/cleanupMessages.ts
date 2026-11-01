import { CLEANUP_ACTION, CLEANUP_LIST, teardownFailed } from "./agentMessages";
import type { ErrorCode } from "./client";
import { files, type Message } from "./messageDetail";
import { messageFor } from "./messages";

/**
 * `agents.worktree_dirty` where a closed task's cleanup stopped. Cleanup first undoes what changed
 * during it in a work folder that was clean before, so what stops it are changes it does not
 * delete: the user's own, from before cleanup started, or what the undo leaves alone, such as a
 * nested repository the teardown made, which it makes again on every retry. A commit would move the
 * folder off the head the task closed with, which takes it out of the cleanup list for good, so the
 * copy says not to. The teardown is to blame only for what comes back: from a folder cleared as the
 * copy says, the retry undoes the files the teardown writes. The table keeps the copy for a merge
 * or PR. The next step, which depends on where the copy is shown, goes between `action` and
 * `retry`.
 */
const CLEANUP_DIRTY = {
  title: "작업 폴더에 devbox가 지우지 않는 변경이 남아 있습니다.",
  action: "남은 파일은 옮기거나 지우고 바뀐 파일은 되돌린 뒤(커밋하지 말고)",
  retry: "다시 정리해도 같은 것이 또 생기면 정리 명령부터 고치세요.",
};

/**
 * `agents.operation_in_progress` where a closed task's cleanup stopped: the work folder holds a
 * merge, rebase, cherry-pick or revert that was started and not finished, whose state its removal
 * would take with it. Finishing it commits, which moves the folder off the head the task closed
 * with and takes it out of the cleanup list for good, so the copy says to abort it instead. It is
 * the folder's own operation, not another one, as the table's copy for a rebase or merge says. The
 * next step goes between `action` and `finished`, as for `CLEANUP_DIRTY`.
 */
const CLEANUP_OPERATION = {
  title: "작업 폴더에 끝나지 않은 Git 작업(병합·rebase·cherry-pick 등)이 있습니다.",
  action: "작업 폴더에서 그 작업을 마치지 말고 중단한 뒤(예: git merge --abort, git rebase --abort)",
  finished: "마치면 커밋이 생겨 이 작업 폴더는 다시 정리 후보에 나오지 않습니다.",
};

/**
 * What a closed task's cleanup that stopped says, in the task's notice or in the cleanup list's own
 * line. The notice is outside settings, so its next step names the whole way to the list; the
 * list's line says only to clean up again. In the notice the stored text, with any files it names,
 * is one [자세히] away, and the table's action, written for the operation that failed, gives way
 * to the list's. The list's line keeps the code's own message and names the files the error lists.
 */
export function cleanupMessage(code: ErrorCode, detail: unknown, where: "notice" | "list"): Message {
  const again = where === "notice" ? `${CLEANUP_LIST}에서 다시 정리하세요.` : "다시 정리하세요.";
  if (code === "agents.worktree_dirty") {
    const names = where === "list" ? files(detail) : "";
    const { title, action, retry } = CLEANUP_DIRTY;
    return { title, action: `${action} ${again} ${retry}${names ? ` 변경 파일: ${names}` : ""}` };
  }
  if (code === "agents.operation_in_progress") {
    const { title, action, finished } = CLEANUP_OPERATION;
    return { title, action: `${action} ${again} ${finished}` };
  }
  if (code === "agents.teardown_failed") return teardownFailed(detail, again);
  const message = messageFor(code, detail);
  if (where === "list") return message;
  // A folder devbox does not remove: the notice keeps how to remove it, then points to the list.
  if (code === "agents.worktree_has_submodules") return { ...message, action: `${message.action} ${CLEANUP_ACTION}` };
  return { title: message.title, action: CLEANUP_ACTION };
}
