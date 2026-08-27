import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { startWebSocket } from "./api";
import type { RequestTemplate, WebSocketUpdate } from "./types";

vi.mock("./lib/isTauri", () => ({ isTauri: () => false }));

class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  readonly url: string;
  readyState = MockWebSocket.CONNECTING;
  binaryType = "blob";
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  send = vi.fn();
  close = vi.fn(() => {
    this.readyState = MockWebSocket.CLOSED;
  });

  constructor(url: string) {
    this.url = url;
    instances.push(this);
  }
}

const instances: MockWebSocket[] = [];

function request(patch: Partial<RequestTemplate> = {}): RequestTemplate {
  return {
    method: "GET",
    url: "ws://localhost:9000/socket",
    headers: [],
    cookies: [],
    multipart: [],
    params: [],
    body_kind: "none",
    body: "",
    auth: null,
    timeout_ms: 100,
    ...patch,
  };
}

beforeEach(() => {
  instances.length = 0;
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", MockWebSocket);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("browser WebSocket lifecycle", () => {
  it("closes a socket that remains connecting past the request timeout", async () => {
    const updates: WebSocketUpdate[] = [];
    const handle = await startWebSocket(request(), [], (update) => updates.push(update));
    const socket = instances[0];

    expect(socket?.url).toBe("ws://localhost:9000/socket");
    expect(updates[updates.length - 1]).toMatchObject({ kind: "state", state: "connecting" });

    await vi.advanceTimersByTimeAsync(100);

    expect(updates[updates.length - 1]).toMatchObject({
      kind: "state",
      state: "error",
      message: "WebSocket 연결 시간이 초과되었습니다",
    });
    expect(socket?.close).toHaveBeenCalledTimes(1);

    await handle.stop();
    expect(socket?.close).toHaveBeenCalledTimes(1);
  });
});
