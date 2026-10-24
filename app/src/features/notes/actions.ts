import { type RpcClient, RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { useToasts } from "../../ui";
import { useNoteSelection } from "./selection";

export function showError(error: unknown) {
  const e = error instanceof RpcError ? error : new RpcError("internal");
  const message = messageFor(e.code, e.detail);
  useToasts
    .getState()
    .push({ tone: "danger", title: message.title, ...(message.action ? { body: message.action } : {}) });
}

/** `notes.create` with the default body; the daemon picks a free name (`새 노트 2`, …). */
export async function createNote(client: RpcClient, folder = ""): Promise<string | undefined> {
  try {
    const { path } = await client.call("notes.create", { folder, title: "새 노트", template: null });
    useNoteSelection.getState().select(path);
    return path;
  } catch (error) {
    showError(error);
    return undefined;
  }
}

/** Today's daily note (`notes.daily`, created from the daily template when missing). */
export async function openDaily(client: RpcClient): Promise<string | undefined> {
  try {
    const { path } = await client.call("notes.daily", { date: null });
    useNoteSelection.getState().select(path);
    return path;
  } catch (error) {
    showError(error);
    return undefined;
  }
}
