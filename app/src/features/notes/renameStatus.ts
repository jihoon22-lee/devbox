import { type RpcClient, RpcError } from "../../rpc/client";
import type { IncompleteReason, RenameStatus, RewriteFailure } from "../../rpc/gen/rpc";
import { useToasts } from "../../ui";
import { showError } from "./actions";

export const FAILURE_LABEL: Record<RewriteFailure, string> = {
  conflict: "다른 곳에서 바뀌어 덮어쓰지 않음",
  open: "다른 창에서 편집 중이라 고치지 못함",
  missing: "파일이 없음",
  unsupported: "지원하지 않는 링크 형식(참조형 링크)",
  too_large: "노트가 2 MiB를 넘어 고치지 못함",
};
export const INCOMPLETE_LABEL: Record<IncompleteReason, string> = {
  failed: "일부 노트의 링크를 고치지 못했습니다",
  target_exists: "새 이름의 노트가 이미 있어 멈췄습니다",
  source_missing: "원래 노트가 없어 멈췄습니다",
  move_failed: "노트를 옮기지 못했습니다",
  unreadable: "진행 기록을 읽지 못했습니다",
};

const POLL_MS = 400;
const GIVE_UP_MS = 90_000;
const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Polls `notes.rename_status` until the rename is no longer running (`null` if it is unknown or lost). */
export async function waitRename(client: RpcClient, opId: string): Promise<RenameStatus | null> {
  const until = Date.now() + GIVE_UP_MS;
  while (Date.now() < until) {
    try {
      const status = await client.call("notes.rename_status", { opId });
      if (status.state !== "running") return status;
    } catch (error) {
      // Statuses live in daemon memory: after a restart the operation is simply unknown.
      if (error instanceof RpcError && error.code === "notes.not_found") return null;
    }
    await wait(POLL_MS);
  }
  return null;
}

/** Whether the note itself moved: `unknown` when the result could not be read. */
export type RenameOutcome = "moved" | "kept" | "unknown";

/** Starts a rename that rewrites links, waits for it and reports the outcome with toasts. */
export async function renameNote(client: RpcClient, path: string, newPath: string): Promise<RenameOutcome> {
  let opId: string;
  try {
    ({ opId } = await client.call("notes.rename", { path, newPath, rewriteLinks: true }));
  } catch (error) {
    showError(error);
    return "kept";
  }
  const toasts = useToasts.getState();
  const progress = toasts.push({ tone: "info", title: "노트를 옮기고 링크를 고치는 중입니다" });
  const status = await waitRename(client, opId);
  toasts.dismiss(progress);
  if (!status) {
    toasts.push({ tone: "warn", title: "링크 고치기 결과를 확인하지 못했습니다. 다른 노트의 링크를 확인해 주세요" });
    return "unknown";
  }
  if (status.state === "done" && !status.failed.length) {
    toasts.push({ tone: "ok", title: "노트를 옮기고 링크를 고쳤습니다" });
    return "moved";
  }
  const title = status.failed.length
    ? `링크를 고치지 못한 노트 ${status.failed.length}개${status.truncated ? " 이상" : ""}`
    : INCOMPLETE_LABEL[status.reason ?? "failed"];
  const body = status.failed
    .slice(0, 5)
    .map((item) => `${item.path} — ${FAILURE_LABEL[item.reason]}`)
    .join("\n");
  toasts.push({ tone: "warn", title, ...(body ? { body } : {}) });
  // `failed` means the note moved but some links stayed; the other reasons stop before or at the move.
  return status.state === "done" || status.reason === "failed"
    ? "moved"
    : status.reason === "unreadable"
      ? "unknown"
      : "kept";
}
