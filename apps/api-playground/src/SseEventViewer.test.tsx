import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatSseEvents, SseEventViewer } from "./SseEventViewer";
import type { SseEvent } from "./lib/sse";

const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const errorMock = vi.fn<(message: string) => void>();

const events: SseEvent[] = [
  { event: "message", data: "<masked> & safe", id: "event-1", retryMs: 1_000 },
];

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  errorMock.mockReset();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

afterEach(() => cleanup());

describe("SseEventViewer", () => {
  it("renders event payload as text and exposes pause state", () => {
    const onPauseChange = vi.fn<(paused: boolean) => void>();
    const { container } = render(
      <SseEventViewer
        events={events}
        dropped={3}
        paused={false}
        onPauseChange={onPauseChange}
        onError={errorMock}
      />,
    );

    expect(screen.getByRole("log").textContent).toContain("<masked> & safe");
    expect(container.querySelector("script")).toBeNull();
    expect(screen.getByText(/3 evicted/u)).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "Pause SSE event rendering" }));
    expect(onPauseChange).toHaveBeenCalledWith(true);
  });

  it("copies only the explicit formatted masked range", async () => {
    render(
      <SseEventViewer
        events={events}
        dropped={0}
        paused={false}
        onPauseChange={vi.fn()}
        onError={errorMock}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy masked events" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(formatSseEvents(events)));
    expect(errorMock).not.toHaveBeenCalled();
  });

  it("reports clipboard failures without reflecting the underlying error", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("clipboard secret path"));
    render(
      <SseEventViewer
        events={events}
        dropped={0}
        paused={false}
        onPauseChange={vi.fn()}
        onError={errorMock}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy masked events" }));
    await waitFor(() => expect(errorMock).toHaveBeenCalledWith("SSE event를 클립보드에 복사하지 못했습니다."));
    expect(document.body.textContent).not.toContain("clipboard secret path");
  });
});
