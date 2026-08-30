import { beforeEach, describe, expect, it, vi } from "vitest";
import { sendSelectionToToolbox, TOOLBOX_SELECTION_BROWSER_ERROR } from "./api";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("./lib/isTauri", () => ({ isTauri: mocks.isTauri }));

beforeEach(() => {
  mocks.invoke.mockReset().mockResolvedValue({ handoffId: "handoff-1", redacted: false });
  mocks.isTauri.mockReset().mockReturnValue(true);
});

describe("sendSelectionToToolbox", () => {
  it("invokes the native command with only the selected rendered text", async () => {
    await expect(sendSelectionToToolbox("safe response selection")).resolves.toEqual({
      handoffId: "handoff-1",
      redacted: false,
    });
    expect(mocks.invoke).toHaveBeenCalledWith("send_selection_to_toolbox", {
      text: "safe response selection",
    });
  });

  it("reports native-only availability in browser mode without invoking or copying", async () => {
    mocks.isTauri.mockReturnValue(false);

    await expect(sendSelectionToToolbox("safe response selection")).rejects.toThrow(
      TOOLBOX_SELECTION_BROWSER_ERROR,
    );
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});
