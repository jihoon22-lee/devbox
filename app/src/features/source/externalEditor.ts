import { invoke } from "@tauri-apps/api/core";
import type { RpcClient } from "../../rpc/client";
import type { DocKey, SourceEditor } from "../../rpc/gen/rpc";
export async function openExternalEditor(client: Pick<RpcClient, "call">, key: DocKey, editor: SourceEditor) {
  const result = await client.call("files.external_editor", { key, editor });
  if (result.opened) return;
  if (!("__TAURI_INTERNALS__" in window))
    throw new Error("편집기를 실행하지 못했습니다. Windows 앱에서 다시 시도하세요.");
  await invoke("open_external_editor", { editor, path: result.path });
}
