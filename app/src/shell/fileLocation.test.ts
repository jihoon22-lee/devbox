import { invoke } from "@tauri-apps/api/core";
import type { RpcClient } from "../rpc/client";
import { openFileLocation } from "./fileLocation";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
test("resolves source location before invoking the typed native editor without URL opening", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  const call = vi.fn().mockResolvedValue({ path: "/owned/old cwd/x.ts", line: 10, column: 2 });
  const location = { path: "x.ts", line: 10, column: 2, source: { kind: "run" as const, runId: "old-run" } };
  try {
    await openFileLocation({ call } as unknown as RpcClient, "p", location, "cursor");
    expect(call).toHaveBeenCalledWith("observe.resolve_location", { projectId: "p", ...location });
    expect(invoke).toHaveBeenCalledWith("open_file_location", {
      editor: "cursor",
      path: "/owned/old cwd/x.ts",
      line: 10,
      column: 2,
    });
  } finally {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  }
});
