import { openExternal } from "./external";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import { openUrl } from "@tauri-apps/plugin-opener";

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});
test("only web URLs reach browser/native openers", async () => {
  const open = vi.spyOn(window, "open").mockReturnValue(null);
  for (const uri of ["garbage", "javascript:alert(1)", "file:///tmp", "mailto:a@b", "tel:1"]) await openExternal(uri);
  expect(open).not.toHaveBeenCalled();
  expect(openUrl).not.toHaveBeenCalled();
  await openExternal("https://example.com/path");
  expect(open).toHaveBeenCalledExactlyOnceWith("https://example.com/path", "_blank", "noopener,noreferrer");
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  await openExternal("http://example.com/");
  expect(openUrl).toHaveBeenCalledExactlyOnceWith("http://example.com/");
});
