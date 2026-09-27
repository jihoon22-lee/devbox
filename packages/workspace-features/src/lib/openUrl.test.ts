import { afterEach, expect, it, vi } from "vitest";
import { openUrl } from "./openUrl";
import { openUrl as nativeOpen } from "@tauri-apps/plugin-opener";
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it("rejects PR non-HTTPS URLs and credentials before invoking the opener", async () => {
  const open = vi.spyOn(window, "open").mockReturnValue(null);
  for (const value of [
    "http://example.test",
    "file:///private",
    "javascript:alert(1)",
    "https://user:secret@example.test",
    "https://example.test/\nprivate",
  ])
    await expect(openUrl(value)).rejects.toThrow();
  expect(open).not.toHaveBeenCalled();
  expect(nativeOpen).not.toHaveBeenCalled();
});
it("preserves explicit terminal HTTP and uses a separate browser context", async () => {
  const open = vi.spyOn(window, "open").mockReturnValue(null);
  await openUrl("http://localhost:3000", true);
  await openUrl("https://example.test/pr/1");
  expect(open).toHaveBeenCalledWith("http://localhost:3000", "_blank", "noopener,noreferrer");
  expect(open).toHaveBeenCalledWith("https://example.test/pr/1", "_blank", "noopener,noreferrer");
});
