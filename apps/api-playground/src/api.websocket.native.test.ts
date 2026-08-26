import { beforeEach, describe, expect, it, vi } from "vitest";
import { MAX_TEXT_PREVIEW_BYTES } from "./lib/websocket";
import { startWebSocket } from "./api";
import type { RequestTemplate } from "./types";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
}));

vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

let dispatch: ((event: { payload: unknown }) => void) | undefined;

const request: RequestTemplate = {
  method: "GET",
  url: "wss://example.test/socket",
  headers: [],
  cookies: [],
  multipart: [],
  params: [],
  body_kind: "none",
  body: "",
  auth: null,
  timeout_ms: 5_000,
};

beforeEach(() => {
  dispatch = undefined;
  mocks.unlisten.mockReset().mockResolvedValue(undefined);
  mocks.listen.mockReset().mockImplementation(async (_event, callback) => {
    dispatch = callback;
    return mocks.unlisten;
  });
  mocks.invoke.mockReset().mockImplementation(async (command) => {
    if (command === "start_websocket") return "ws-1";
    return undefined;
  });
});

describe("native WebSocket event boundary", () => {
  it("accepts bounded updates and drops oversized or malformed previews", async () => {
    const onUpdate = vi.fn();
    const handle = await startWebSocket(request, [], onUpdate);
    const envelope = {
      sessionId: "ws-1",
      kind: "message",
      direction: "received",
      messageType: "text",
      messageId: 1,
      sequence: 1,
      dropped: 0,
    };

    dispatch?.({ payload: { ...envelope, text: "hello" } });
    dispatch?.({ payload: { ...envelope, messageId: 2, sequence: 2, text: "a".repeat(MAX_TEXT_PREVIEW_BYTES + 1) } });
    dispatch?.({ payload: { ...envelope, messageId: 3, sequence: 3, messageType: "binary", binaryHex: "not-hex" } });

    expect(onUpdate).toHaveBeenCalledTimes(1);
    expect(onUpdate).toHaveBeenCalledWith(expect.objectContaining({ text: "hello" }));

    await handle.stop();
    expect(mocks.unlisten).toHaveBeenCalledTimes(1);
  });
});
