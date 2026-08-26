import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { sanitizePersistedJson, startSseStream } from "./api";
import type { SseUpdate } from "./types";

vi.mock("./api", () => ({
  buildRevealedCurl: vi.fn(),
  copyRawResponseCookies: vi.fn(),
  copyRawResponseHeaders: vi.fn(),
  fetchOpenApiSource: vi.fn(),
  pickMultipartFile: vi.fn(),
  sanitizePersistedJson: vi.fn(),
  sealSecret: vi.fn(),
  sendRequest: vi.fn(),
  startSseStream: vi.fn(),
}));

const sanitizePersistedJsonMock = vi.mocked(sanitizePersistedJson);
const startSseStreamMock = vi.mocked(startSseStream);
const stopMock = vi.fn<() => Promise<void>>();
let emitUpdate: ((update: SseUpdate) => void) | undefined;

async function renderReady() {
  const view = render(<App />);
  fireEvent.change(screen.getByPlaceholderText("https://api.example.com/users"), {
    target: { value: "https://example.test/stream" },
  });
  const start = screen.getByRole("button", { name: "Start SSE" }) as HTMLButtonElement;
  await waitFor(() => expect(start.disabled).toBe(false));
  return { ...view, start };
}

beforeEach(() => {
  localStorage.clear();
  emitUpdate = undefined;
  stopMock.mockReset().mockResolvedValue(undefined);
  sanitizePersistedJsonMock.mockReset().mockImplementation(async (serialized) => serialized);
  startSseStreamMock.mockReset().mockImplementation(async (_request, _environment, _options, onUpdate) => {
    emitUpdate = onUpdate;
    return { sessionId: "browser-sse-1", stop: stopMock };
  });
});

afterEach(() => cleanup());

describe("API Playground SSE lifecycle", () => {
  it("releases the stream handle and listener after a terminal update", async () => {
    const { start } = await renderReady();
    fireEvent.click(start);
    await waitFor(() => expect(startSseStreamMock).toHaveBeenCalledTimes(1));

    act(() => emitUpdate?.({
      sessionId: "browser-sse-1",
      kind: "connected",
      sequence: 0,
      dropped: 0,
    }));
    expect(screen.getByRole("status").textContent).toBe("SSE connected");

    act(() => emitUpdate?.({
      sessionId: "browser-sse-1",
      kind: "closed",
      sequence: 0,
      dropped: 0,
    }));
    await waitFor(() => expect(stopMock).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("status").textContent).toBe("SSE closed");
    expect((screen.getByRole("button", { name: "Stop SSE" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("stops an active stream when the app unmounts", async () => {
    const { start, unmount } = await renderReady();
    fireEvent.click(start);
    await waitFor(() => expect(startSseStreamMock).toHaveBeenCalledTimes(1));
    act(() => emitUpdate?.({
      sessionId: "browser-sse-1",
      kind: "connected",
      sequence: 0,
      dropped: 0,
    }));

    unmount();
    await waitFor(() => expect(stopMock).toHaveBeenCalledTimes(1));
  });
});
