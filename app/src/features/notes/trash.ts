import type { RpcClient } from "../../rpc/client";
import { confirmPolicy } from "../../rpc/query";
import { useToasts } from "../../ui";
import { useDocuments } from "../documents/store";
import { showError } from "./actions";
import { VAULT } from "./notePaths";

/**
 * Moves a note to the vault trash. `notes.delete` is reversible, so there is no confirmation;
 * a 10 s toast offers [되돌리기] (`notes.restore` to the same path). The toast lives in the shell,
 * so the undo survives navigation. An open note with unsaved edits is left alone.
 */
export async function moveToTrash(client: RpcClient, path: string): Promise<boolean> {
  const key = { root: VAULT, path };
  const docs = useDocuments.getState();
  const open = docs.find(key);
  if (open && (open.editSeq > open.savedSeq || open.state === "Saving" || open.state === "Recovering")) {
    useToasts.getState().push({ tone: "warn", title: "저장되지 않은 내용이 있어 휴지통으로 옮기지 않았습니다" });
    return false;
  }
  // The trash keeps lease and journal on the old key; release ours first so nothing recreates the file.
  if (open) {
    docs.close(key);
    await docs.settled();
  }
  let trashPath: string;
  try {
    ({ trashPath } = await client.call("notes.delete", { path }));
  } catch (error) {
    showError(error);
    return false;
  }
  const name = path.split("/").pop() ?? path;
  const undo =
    confirmPolicy("notes.delete") === "reversible"
      ? () => void client.call("notes.restore", { trashPath, path }).catch(showError)
      : undefined;
  useToasts.getState().push({ tone: "info", title: `휴지통으로 옮겼습니다: ${name}`, ...(undo ? { undo } : {}) });
  return true;
}
