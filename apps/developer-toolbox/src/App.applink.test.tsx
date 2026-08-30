import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  acceptToolboxText,
  discardToolboxText,
  onOpenRequest,
  previewToolboxText,
  renewToolboxText,
  takePendingOpen,
} from "./api";
import type { OpenRequest, ToolboxTextHandoffPreview } from "./types";

const mocks = vi.hoisted(() => ({
  wakeup: null as (() => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  acceptToolboxText: vi.fn(),
  createApiRequestHandoff: vi.fn(),
  discardToolboxText: vi.fn(),
  onOpenRequest: vi.fn(),
  previewToolboxText: vi.fn(),
  readClipboardText: vi.fn(),
  renewToolboxText: vi.fn(),
  takePendingOpen: vi.fn(),
  TOOLBOX_TEXT_HANDOFF_KIND: "toolbox-text/v1",
}));

const acceptToolboxTextMock = vi.mocked(acceptToolboxText);
const discardToolboxTextMock = vi.mocked(discardToolboxText);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const previewToolboxTextMock = vi.mocked(previewToolboxText);
const renewToolboxTextMock = vi.mocked(renewToolboxText);
const takePendingOpenMock = vi.mocked(takePendingOpen);

const firstId = "0123456789abcdef0123456789abcdef";
const secondId = "fedcba9876543210fedcba9876543210";

function request(id = firstId): OpenRequest {
  return {
    target: { kind: "handoff", handoffKind: "toolbox-text/v1", id },
    from: "api-playground",
  };
}

function preview(id = firstId, producerId = "api-playground"): ToolboxTextHandoffPreview {
  return {
    handoffId: id,
    producerId,
    expiresAtMs: Date.now() + 600_000,
    text: "accepted handoff text",
    redacted: false,
  };
}

async function waitForListener(): Promise<void> {
  await waitFor(() => expect(mocks.wakeup).not.toBeNull());
}

async function openPreview(id = firstId): Promise<void> {
  takePendingOpenMock.mockResolvedValueOnce(request(id));
  await act(async () => mocks.wakeup?.());
  await screen.findByRole("dialog", { name: "Toolbox 텍스트 미리보기" });
}

beforeEach(() => {
  mocks.wakeup = null;
  mocks.order.length = 0;
  onOpenRequestMock.mockReset().mockImplementation(async (handler) => {
    mocks.order.push("listen");
    mocks.wakeup = handler;
    return () => undefined;
  });
  takePendingOpenMock.mockReset().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  });
  previewToolboxTextMock.mockReset().mockImplementation(async (id) => preview(id));
  renewToolboxTextMock.mockReset().mockResolvedValue({ leaseUntilMs: Date.now() + 60_000 });
  acceptToolboxTextMock.mockReset().mockResolvedValue("accepted handoff text");
  discardToolboxTextMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("Developer Toolbox toolbox-text/v1 receiver", () => {
  it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
    const { container } = render(<App />);
    await screen.findByRole("textbox", { name: "스마트 워크플로 입력" });
    await assertNoA11yViolations(container);
  });

  it("registers before the cold pull and previews a cold request", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      mocks.order.push("take");
      return request();
    });
    render(<App />);

    await screen.findByRole("dialog", { name: "Toolbox 텍스트 미리보기" });
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);
    expect(previewToolboxTextMock).toHaveBeenCalledWith(firstId);
    expect(screen.getByText("API Playground")).toBeTruthy();
    expect(screen.getByText("toolbox-text/v1")).toBeTruthy();
  });

  it("treats a hot event as a wakeup and re-takes the pending request", async () => {
    render(<App />);
    await waitForListener();
    const initialTakeCount = takePendingOpenMock.mock.calls.length;

    await openPreview(secondId);

    expect(takePendingOpenMock.mock.calls.length).toBe(initialTakeCount + 1);
    expect(previewToolboxTextMock).toHaveBeenCalledWith(secondId);
    expect(document.body.textContent).not.toContain("stale event payload");
  });

  it("localizes an allowlisted native handoff error without changing its contract", async () => {
    previewToolboxTextMock.mockRejectedValueOnce(
      new Error("다른 텍스트 handoff를 먼저 처리하세요"),
    );
    render(<App />);
    await waitForListener();
    takePendingOpenMock.mockResolvedValueOnce(request());

    await act(async () => mocks.wakeup?.());

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(
      "다른 텍스트 전달을 먼저 처리하세요",
    ));
    expect(document.body.textContent).not.toContain("다른 텍스트 handoff를 먼저 처리하세요");
  });

  it("focuses Cancel by default and applies only after ack into Smart input", async () => {
    render(<App />);
    await waitForListener();
    await openPreview();

    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" })));
    const smartInput = screen.getByRole("textbox", { name: "스마트 워크플로 입력" }) as HTMLTextAreaElement;
    fireEvent.change(smartInput, { target: { value: '{"name":"Ada"}' } });
    fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
    fireEvent.click(screen.getByRole("button", { name: "파이프라인 실행" }));
    expect(screen.getByLabelText("파이프라인 결과").textContent).toContain('"name": "Ada"');

    let resolveAccept!: (value: string) => void;
    acceptToolboxTextMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolveAccept = resolve;
    }));
    fireEvent.click(screen.getByRole("button", { name: "적용" }));
    expect(smartInput.value).toBe('{"name":"Ada"}');
    expect(acceptToolboxTextMock).toHaveBeenCalledWith(firstId);

    resolveAccept("injected text");
    await waitFor(() => expect(smartInput.value).toBe("injected text"));
    expect(screen.getByLabelText("파이프라인 결과").textContent?.trim()).toBe("");
    expect(screen.queryByRole("dialog", { name: "Toolbox 텍스트 미리보기" })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(discardToolboxTextMock).not.toHaveBeenCalled();
  });

  it("restores a cancelled preview without copying or auto-running", async () => {
    render(<App />);
    await waitForListener();
    await openPreview();
    const smartInput = screen.getByRole("textbox", { name: "스마트 워크플로 입력" }) as HTMLTextAreaElement;
    fireEvent.change(smartInput, { target: { value: "existing input" } });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });

    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(discardToolboxTextMock).toHaveBeenCalledWith(firstId));
    expect(screen.queryByRole("dialog", { name: "Toolbox 텍스트 미리보기" })).toBeNull();
    expect(smartInput.value).toBe("existing input");
    expect(writeText).not.toHaveBeenCalled();
    expect(acceptToolboxTextMock).not.toHaveBeenCalled();
  });

  it("renews the exact claim while its one preview is open", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<App />);
      await waitForListener();
      await openPreview();
      await act(async () => {
        vi.advanceTimersByTime(30_001);
        await Promise.resolve();
      });
      expect(renewToolboxTextMock).toHaveBeenCalledWith(firstId);
      expect(screen.getByRole("dialog", { name: "Toolbox 텍스트 미리보기" })).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("restores a late claim when the renderer unmounts", async () => {
    let resolvePreview!: (value: ToolboxTextHandoffPreview) => void;
    previewToolboxTextMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    const view = render(<App />);
    await waitForListener();
    takePendingOpenMock.mockResolvedValueOnce(request());
    await act(async () => mocks.wakeup?.());
    expect(previewToolboxTextMock).toHaveBeenCalledWith(firstId);
    view.unmount();

    resolvePreview(preview());
    await waitFor(() => expect(discardToolboxTextMock).toHaveBeenCalledWith(firstId));
    expect(screen.queryByRole("dialog", { name: "Toolbox 텍스트 미리보기" })).toBeNull();
  });

  it("does not allow a stale preview response to open a modal", async () => {
    let resolvePreview!: (value: ToolboxTextHandoffPreview) => void;
    previewToolboxTextMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    const view = render(<App />);
    await waitForListener();
    takePendingOpenMock.mockResolvedValueOnce(request());
    await act(async () => mocks.wakeup?.());
    view.unmount();

    resolvePreview({ ...preview(), producerId: "unknown-app" });
    await act(async () => Promise.resolve());
    expect(screen.queryByRole("dialog", { name: "Toolbox 텍스트 미리보기" })).toBeNull();
    expect(discardToolboxTextMock).toHaveBeenCalledWith(firstId);
  });

  it("rejects an unallowlisted source label without echoing its identity", async () => {
    previewToolboxTextMock.mockResolvedValueOnce(preview(firstId, "unknown-app"));
    render(<App />);
    await waitForListener();
    takePendingOpenMock.mockResolvedValueOnce(request());
    await act(async () => mocks.wakeup?.());

    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getByRole("alert").textContent).toContain("텍스트 전달을 사용할 수 없습니다");
    expect(document.body.textContent).not.toContain("unknown-app");
    expect(screen.queryByRole("dialog", { name: "Toolbox 텍스트 미리보기" })).toBeNull();
  });
});
