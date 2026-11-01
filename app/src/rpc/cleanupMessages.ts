import { CLEANUP_ACTION } from "./agentMessages";
import type { ErrorCode } from "./client";
import { files, type Message } from "./messageDetail";
import { messageFor } from "./messages";

/**
 * `agents.worktree_dirty` where a closed task's cleanup stopped. Cleanup first undoes what changed
 * during it in a work folder that was clean before, so what stops it are changes it does not
 * delete: the user's own, from before cleanup started, or what the undo leaves alone (a nested
 * repository). A commit would move the folder off the head the task closed with, which takes it out
 * of the cleanup list for good, so the copy says not to. The table keeps the copy for a merge or PR.
 */
export const CLEANUP_DIRTY: Message = {
  title: "작업 폴더에 devbox가 지우지 않는 변경이 남아 있습니다.",
  action: "남은 파일은 옮기거나 지우고 바뀐 파일은 되돌린 뒤(커밋하지 말고) 정리 후보에서 다시 정리하세요.",
};

/**
 * What a closed task's cleanup that stopped says, in the task's notice or in the cleanup list's own
 * line. In the notice the stored text, with any files it names, is one [자세히] away, and the next
 * action points to the cleanup list: the table's action belongs to the operation that failed, except
 * a failed teardown's, which is written for cleanup. The list's line keeps the code's own message
 * and names the files the error lists.
 */
export function cleanupMessage(code: ErrorCode, detail: unknown, where: "notice" | "list"): Message {
  if (code === "agents.worktree_dirty") {
    const names = where === "list" ? files(detail) : "";
    return names ? { ...CLEANUP_DIRTY, action: `${CLEANUP_DIRTY.action} 변경 파일: ${names}` } : CLEANUP_DIRTY;
  }
  const message = messageFor(code, detail);
  if (where === "list" || code === "agents.teardown_failed") return message;
  // A folder devbox does not remove: the notice keeps how to remove it, then points to the list.
  if (code === "agents.worktree_has_submodules") return { ...message, action: `${message.action} ${CLEANUP_ACTION}` };
  return { title: message.title, action: CLEANUP_ACTION };
}
