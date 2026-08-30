import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { discardCurrentResponse, sanitizePersistedJson, startSseStream } from "./api";
import type { SseUpdate } from "./types";

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
  takePendingOpen: vi.fn(async () => null),
}));

const sanitizePersistedJsonMock = vi.mocked(sanitizePersistedJson);
const startSseStreamMock = vi.mocked(startSseStream);
const discardCurrentResponseMock = vi.mocked(discardCurrentResponse);
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
  discardCurrentResponseMock.mockReset().mockResolvedValue(undefined);
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
    expect(screen.getByText("SSE connected", { selector: "span.sse-status" })).toBeTruthy();

    act(() => emitUpdate?.({
      sessionId: "browser-sse-1",
      kind: "closed",
      sequence: 0,
      dropped: 0,
    }));
    await waitFor(() => expect(stopMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText("SSE closed", { selector: "span.sse-status" })).toBeTruthy();
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
    await waitFor(() => expect(discardCurrentResponseMock).toHaveBeenCalledTimes(1));
  });

  it("releases a browser import after the native picker is cancelled", async () => {
    const { unmount } = await renderReady();
    const importButton = screen.getAllByRole("button", { name: "JSON 가져오기" })[0] as HTMLButtonElement;
    const input = screen.getByLabelText("JSON 파일 가져오기") as HTMLInputElement;

    fireEvent.click(importButton);
    await waitFor(() => expect(importButton.disabled).toBe(true));
    fireEvent(input, new Event("cancel", { bubbles: true }));
    await waitFor(() => expect(importButton.disabled).toBe(false));

    // A second attempt is accepted, proving the cancelled attempt did not
    // leave browserImportKind latched in the renderer.
    fireEvent.click(importButton);
    await waitFor(() => expect(importButton.disabled).toBe(true));
    unmount();
  });
});
