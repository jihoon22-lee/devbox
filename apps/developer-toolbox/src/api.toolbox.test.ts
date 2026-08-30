import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import {
  acceptToolboxText,
  discardToolboxText,
  onOpenRequest,
  previewToolboxText,
  renewToolboxText,
  takePendingOpen,
  TOOLBOX_TEXT_BROWSER_ERROR,
} from "./api";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  isTauri: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("./lib/isTauri", () => ({ isTauri: mocks.isTauri }));

const handoffId = "0123456789abcdef0123456789abcdef";

beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.listen.mockReset().mockResolvedValue(() => undefined);
  mocks.isTauri.mockReset().mockReturnValue(false);
});

afterEach(() => vi.clearAllMocks());

describe("Developer Toolbox handoff API", () => {
  it("does not claim or listen in browser preview mode", async () => {
    await expect(takePendingOpen()).resolves.toBeNull();
    const unlisten = await onOpenRequest(vi.fn());
    await expect(previewToolboxText(handoffId)).rejects.toThrow(TOOLBOX_TEXT_BROWSER_ERROR);
    await expect(renewToolboxText(handoffId)).rejects.toThrow(TOOLBOX_TEXT_BROWSER_ERROR);
    await expect(acceptToolboxText(handoffId)).rejects.toThrow(TOOLBOX_TEXT_BROWSER_ERROR);
    await expect(discardToolboxText(handoffId)).rejects.toThrow(TOOLBOX_TEXT_BROWSER_ERROR);
    expect(unlisten()).toBeUndefined();
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(mocks.listen).not.toHaveBeenCalled();
  });

  it("uses the pending slot and exact native command wire names", async () => {
    mocks.isTauri.mockReturnValue(true);
    mocks.invoke
      .mockResolvedValueOnce({
        target: { kind: "handoff", handoffKind: "toolbox-text/v1", id: handoffId },
        from: "api-playground",
      })
      .mockResolvedValueOnce({
        handoffId,
        producerId: "api-playground",
        expiresAtMs: 1_700_000_600_000,
        text: "safe text",
        redacted: false,
      })
      .mockResolvedValueOnce({ leaseUntilMs: 1_700_000_060_000 })
      .mockResolvedValueOnce("accepted text")
      .mockResolvedValueOnce(undefined);

    await expect(takePendingOpen()).resolves.toEqual({
      target: { kind: "handoff", handoffKind: "toolbox-text/v1", id: handoffId },
      from: "api-playground",
    });
    await expect(previewToolboxText(handoffId)).resolves.toMatchObject({ handoffId, text: "safe text" });
    await expect(renewToolboxText(handoffId)).resolves.toEqual({ leaseUntilMs: 1_700_000_060_000 });
    await expect(acceptToolboxText(handoffId)).resolves.toBe("accepted text");
    await expect(discardToolboxText(handoffId)).resolves.toBeUndefined();
    expect(mocks.invoke.mock.calls.map(([command]) => command)).toEqual([
      "take_pending_open",
      "preview_toolbox_text",
      "renew_toolbox_text",
      "accept_toolbox_text",
      "discard_toolbox_text",
    ]);
    expect(mocks.invoke.mock.calls[1][1]).toEqual({ handoffId });
  });

  it("ignores event payloads and re-takes only through the wakeup callback", async () => {
    mocks.isTauri.mockReturnValue(true);
    let wakeup!: (payload: unknown) => void;
    mocks.listen.mockImplementationOnce(async (_name: string, callback: (payload: unknown) => void) => {
      wakeup = callback;
      return () => undefined;
    });
    const handler = vi.fn();
    await onOpenRequest(handler);
    wakeup({ target: { kind: "handoff", handoffKind: "toolbox-text/v1", id: "bad" } });
    expect(handler).toHaveBeenCalledWith();
    expect(listen).toHaveBeenCalledWith("devbox://open", expect.any(Function));
  });

  it("rejects an unallowlisted native source before showing it to the renderer", async () => {
    mocks.isTauri.mockReturnValue(true);
    mocks.invoke.mockResolvedValueOnce({
      handoffId,
      producerId: "unknown-app",
      expiresAtMs: 1_700_000_600_000,
      text: "safe text",
      redacted: false,
    });
    await expect(previewToolboxText(handoffId)).rejects.toThrow("텍스트 handoff 응답을 사용할 수 없습니다");
  });
});
