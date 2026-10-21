import { invoke } from "@tauri-apps/api/core";
import type { RpcClient } from "../rpc/client";
import type { LogSource } from "../rpc/gen/rpc";
export async function openFileLocation(
  client: RpcClient,
  projectId: string,
  location: {
    path: string;
    line: number;
    column?: number;
    source: LogSource;
  },
  editor: "vscode" | "cursor" = "vscode",
) {
  const resolved = await client.call("observe.resolve_location", {
    projectId,
    ...location,
    column: location.column ?? null,
  });
  if (!("__TAURI_INTERNALS__" in window)) throw new Error("파일 위치는 Windows 앱에서 외부 편집기로 열 수 있습니다.");
  await invoke("open_file_location", { editor, ...resolved });
}
