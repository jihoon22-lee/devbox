import { invoke } from "@tauri-apps/api/core";
import { afterEach, expect, test, vi } from "vitest";
import { openExternalEditor } from "./externalEditor";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});
test("tries the daemon editor first and falls back to the native fixed editor command", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  const key = { root: "project:p", path: "가 & $(x).ts" };
  const call = vi.fn().mockResolvedValue({ opened: false, path: "/owned/가 & $(x).ts" });
  await openExternalEditor({ call } as never, key, "cursor");
  expect(call).toHaveBeenCalledWith("files.external_editor", { key, editor: "cursor" });
  expect(invoke).toHaveBeenCalledWith("open_external_editor", { editor: "cursor", path: "/owned/가 & $(x).ts" });
  call.mockResolvedValue({ opened: true, path: "/owned/x" });
  await openExternalEditor({ call } as never, key, "code");
  expect(invoke).toHaveBeenCalledTimes(1);
});
test("browser fallback says unavailable rather than executing another command", async () => {
  const call = vi.fn().mockResolvedValue({ opened: false, path: "/owned/x" });
  await expect(openExternalEditor({ call } as never, { root: "project:p", path: "x" }, "zed")).rejects.toThrow(
    "Windows 앱",
  );
  expect(invoke).not.toHaveBeenCalled();
});
test("native editor launch rejection reports a Korean actionable error", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.mocked(invoke).mockRejectedValueOnce(new Error("native spawn failed"));
  const call = vi.fn().mockResolvedValue({ opened: false, path: "/owned/a.ts" });
  await expect(openExternalEditor({ call } as never, { root: "project:p", path: "a.ts" }, "zed")).rejects.toThrow(
    "Zed를 실행하지 못했습니다. Windows에 설치되어 있는지 확인하세요.",
  );
});
