import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { getWindowsBuildNumber, onOpenRequest, startSession, takePendingOpen } from "./api";
import type { OpenRequest } from "./types";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
  paneCanvasProps: null as unknown,
}));

vi.mock("./components/PaneCanvas", () => ({
  default: (props: unknown) => {
    mocks.paneCanvasProps = props;
    return <div data-testid="pane-canvas" />;
  },
}));
vi.mock("./api", () => ({
  listDistros: vi.fn().mockResolvedValue([
    { name: "Ubuntu", version: 2, default: true, state: "Running" },
  ]),
  dockerPs: vi.fn().mockResolvedValue([]),
  dockerAction: vi.fn().mockResolvedValue(undefined),
  startSession: vi.fn().mockResolvedValue("session-1"),
  closeSession: vi.fn().mockResolvedValue(undefined),
  onTerminalClosed: vi.fn().mockResolvedValue(() => undefined),
  onTerminalOutput: vi.fn().mockResolvedValue(() => undefined),
  getWindowsBuildNumber: vi.fn().mockResolvedValue(null),
  takePendingOpen: vi.fn().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  }),
  onOpenRequest: vi.fn().mockImplementation(async (handler: (request: OpenRequest) => void) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  }),
}));

const startSessionMock = vi.mocked(startSession);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const takePendingOpenMock = vi.mocked(takePendingOpen);
const getWindowsBuildNumberMock = vi.mocked(getWindowsBuildNumber);

beforeEach(() => {
  mocks.openHandler = null;
  mocks.order.length = 0;
  mocks.paneCanvasProps = null;
  startSessionMock.mockClear();
  onOpenRequestMock.mockClear();
  takePendingOpenMock.mockClear();
  getWindowsBuildNumberMock.mockClear();
});

afterEach(() => cleanup());

describe("App app-link delivery", () => {
  it("waits for the Windows build lookup before mounting PaneCanvas", async () => {
    let resolveBuild: ((value: number | null) => void) | undefined;
    getWindowsBuildNumberMock.mockImplementationOnce(
      () => new Promise<number | null>((resolve) => {
        resolveBuild = resolve;
      }),
    );

    render(<App />);
    expect(mocks.paneCanvasProps).toBeNull();

    await act(async () => resolveBuild?.(22631));
    await waitFor(() => expect(mocks.paneCanvasProps).not.toBeNull());
    expect((mocks.paneCanvasProps as { windowsBuildNumber: number }).windowsBuildNumber).toBe(22631);
  });

  it("listens before the initial take and consumes the pending slot on relaunch", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);

    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/mnt/e/projects/devbox", line: null, column: null },
      from: "workbench",
    });
    await act(async () => {
      mocks.openHandler?.({ target: { kind: "query", text: "stale" }, from: "test" });
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(startSessionMock).toHaveBeenCalledWith("Ubuntu", "/mnt/e/projects/devbox"),
    );
  });

  it("consumes a cold-start request when app-link listener registration fails", async () => {
    onOpenRequestMock.mockRejectedValueOnce(new Error("listener unavailable"));
    takePendingOpenMock.mockResolvedValueOnce({
      target: { kind: "path", path: "/mnt/e/projects/devbox", line: null, column: null },
      from: "workbench",
    });

    render(<App />);

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(startSessionMock).toHaveBeenCalledWith("Ubuntu", "/mnt/e/projects/devbox"),
    );
  });
});
