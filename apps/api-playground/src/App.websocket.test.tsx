import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { sanitizePersistedJson, startWebSocket, type WebSocketHandle } from "./api";
import type { WebSocketUpdate } from "./types";

vi.mock("./api", () => ({
  buildRevealedCurl: vi.fn(),
  copyRawResponseCookies: vi.fn(),
  copyRawResponseHeaders: vi.fn(),
  discardCurrentResponse: vi.fn(async () => undefined),
  fetchOpenApiSource: vi.fn(),
  onOpenRequest: vi.fn(async () => () => undefined),
  pickMultipartFile: vi.fn(),
  readJsonFile: vi.fn(),
  renewApiRequest: vi.fn(),
  restoreApiRequest: vi.fn(),
  saveJsonFile: vi.fn(),
  saveResponseBinary: vi.fn(),
  sanitizePersistedJson: vi.fn(),
  sealSecret: vi.fn(),
  sendSelectionToToolbox: vi.fn(),
  sendRequest: vi.fn(),
  startSseStream: vi.fn(),
  startWebSocket: vi.fn(),
  takePendingOpen: vi.fn(async () => null),
}));

const sanitizePersistedJsonMock = vi.mocked(sanitizePersistedJson);
const startWebSocketMock = vi.mocked(startWebSocket);
const stopMock = vi.fn<() => Promise<void>>();
const saveBinaryMock = vi.fn<(messageId: number) => Promise<boolean>>();
let emitUpdate: ((update: WebSocketUpdate) => void) | undefined;

function handle(): WebSocketHandle {
  return {
    sessionId: "browser-ws-1",
    send: vi.fn(),
    ping: vi.fn(),
    close: vi.fn(),
    saveBinary: saveBinaryMock,
    stop: stopMock,
  };
}

async function renderReady() {
  const view = render(<App />);
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/users"), {
    target: { value: "ws://localhost:9000/socket" },
  });
  const connect = screen.getByRole("button", { name: "Connect WebSocket" }) as HTMLButtonElement;
  await waitFor(() => expect(connect.disabled).toBe(false));
  return { ...view, connect };
}

beforeEach(() => {
  localStorage.clear();
  emitUpdate = undefined;
  stopMock.mockReset().mockResolvedValue(undefined);
  saveBinaryMock.mockReset().mockResolvedValue(true);
  sanitizePersistedJsonMock.mockReset().mockImplementation(async (serialized) => serialized);
  startWebSocketMock.mockReset().mockImplementation(async (_request, _environment, onUpdate) => {
    emitUpdate = onUpdate;
    return handle();
  });
});

afterEach(() => cleanup());

describe("API Playground WebSocket lifecycle", () => {
  it("stops a handle when a terminal update arrives before start resolves", async () => {
    startWebSocketMock.mockImplementation(async (_request, _environment, onUpdate) => {
      onUpdate({
        sessionId: "browser-ws-1",
        kind: "state",
        state: "closed",
        sequence: 0,
        dropped: 0,
      });
      return handle();
    });
    const { connect } = await renderReady();

    fireEvent.click(connect);

    await waitFor(() => expect(stopMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Closed", { selector: "span.websocket-state" })).toBeTruthy();
    expect((screen.getByRole("button", { name: "Disconnect WebSocket" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("releases an installed handle on terminal state", async () => {
    const { connect } = await renderReady();
    fireEvent.click(connect);
    await waitFor(() => expect(startWebSocketMock).toHaveBeenCalledTimes(1));

    act(() => emitUpdate?.({
      sessionId: "browser-ws-1",
      kind: "state",
      state: "open",
      sequence: 0,
      dropped: 0,
    }));
    act(() => emitUpdate?.({
      sessionId: "browser-ws-1",
      kind: "message",
      direction: "received",
      messageType: "binary",
      messageId: 1,
      binaryHex: "0102",
      binarySize: 2,
      sequence: 1,
      dropped: 0,
    }));
    act(() => emitUpdate?.({
      sessionId: "browser-ws-1",
      kind: "state",
      state: "error",
      sequence: 1,
      dropped: 0,
      message: "WebSocket 연결이 끊어졌습니다",
    }));

    await waitFor(() => expect(stopMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Error", { selector: "span.websocket-state" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save binary message 1" }));
    await waitFor(() => expect(saveBinaryMock).toHaveBeenCalledWith(1));
  });
});
