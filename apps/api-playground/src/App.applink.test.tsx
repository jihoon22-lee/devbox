import { act, cleanup, render, screen, waitFor, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  ackApiRequest,
  claimApiRequest,
  onOpenRequest,
  restoreApiRequest,
  takePendingOpen,
} from "./api";
import type { ApiRequestHandoffPreview, OpenRequest, RequestTemplate } from "./types";

const mocks = vi.hoisted(() => ({
  openHandler: null as ((request: OpenRequest) => void) | null,
  order: [] as string[],
}));

vi.mock("./api", () => ({
  ackApiRequest: vi.fn(),
  buildRevealedCurl: vi.fn(),
  claimApiRequest: vi.fn(),
  copyRawResponseCookies: vi.fn(),
  copyRawResponseHeaders: vi.fn(),
  onOpenRequest: vi.fn().mockImplementation(async (handler: (request: OpenRequest) => void) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  }),
  pickMultipartFile: vi.fn(),
  restoreApiRequest: vi.fn(),
  sanitizePersistedJson: vi.fn(async (serialized: string) => serialized),
  sealSecret: vi.fn(),
  sendRequest: vi.fn(),
  takePendingOpen: vi.fn().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  }),
}));

const ackApiRequestMock = vi.mocked(ackApiRequest);
const claimApiRequestMock = vi.mocked(claimApiRequest);
const onOpenRequestMock = vi.mocked(onOpenRequest);
const restoreApiRequestMock = vi.mocked(restoreApiRequest);
const takePendingOpenMock = vi.mocked(takePendingOpen);

const request: RequestTemplate = {
  method: "POST",
  url: "/hooks/push?event=push",
  headers: [{ key: "Authorization", value: "${WEBHOOK_SECRET}", enabled: true }],
  cookies: [],
  multipart: [],
  params: [],
  body_kind: "json",
  body: '{"event":"push"}',
  auth: { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" },
  timeout_ms: 10_000,
  graphql: null,
};

const preview: ApiRequestHandoffPreview = {
  handoffId: "0123456789abcdef0123456789abcdef",
  kind: "api-request/v1",
  producerId: "webhook-lab",
  consumerId: "api-playground",
  expiresAtMs: 1_700_000_600_000,
  request,
};

function handoffRequest(id = preview.handoffId): OpenRequest {
  return {
    target: { kind: "handoff", handoffKind: "api-request/v1", id },
    from: "webhook-lab",
  };
}

beforeEach(() => {
  localStorage.clear();
  mocks.openHandler = null;
  mocks.order.length = 0;
  onOpenRequestMock.mockReset().mockImplementation(async (handler) => {
    mocks.order.push("listen");
    mocks.openHandler = handler;
    return () => undefined;
  });
  takePendingOpenMock.mockReset().mockImplementation(async () => {
    mocks.order.push("take");
    return null;
  });
  claimApiRequestMock.mockReset().mockResolvedValue(preview);
  ackApiRequestMock.mockReset().mockResolvedValue(request);
  restoreApiRequestMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("API Playground api-request/v1 receiver", () => {
  it("registers the listener before the cold pending pull", async () => {
    render(<App />);

    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));
    expect(mocks.order.slice(0, 2)).toEqual(["listen", "take"]);
  });

  it("claims a cold request for preview and applies it only after ack", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      mocks.order.push("take");
      return handoffRequest();
    });
    render(<App />);

    const dialog = await screen.findByRole("dialog", { name: "Webhook 요청 미리보기" });
    expect(dialog.textContent).toContain("producerwebhook-lab");
    expect(dialog.textContent).toContain("consumerapi-playground");
    expect(dialog.textContent).toContain(preview.handoffId);
    expect(claimApiRequestMock).toHaveBeenCalledWith(preview.handoffId);
    expect(ackApiRequestMock).not.toHaveBeenCalled();
    expect((screen.getByPlaceholderText("https://api.example.com/users") as HTMLInputElement).value).toBe("");

    fireEvent.click(screen.getByRole("button", { name: "적용" }));
    await waitFor(() => expect(ackApiRequestMock).toHaveBeenCalledWith(preview.handoffId));
    expect((screen.getByPlaceholderText("https://api.example.com/users") as HTMLInputElement).value)
      .toBe(request.url);
    expect(screen.queryByRole("dialog", { name: "Webhook 요청 미리보기" })).toBeNull();
  });

  it("uses the pending slot rather than a stale hot-event payload", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.openHandler).not.toBeNull());
    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(1));

    takePendingOpenMock.mockResolvedValueOnce(handoffRequest());
    await act(async () => {
      mocks.openHandler?.({
        target: { kind: "handoff", handoffKind: "api-request/v1", id: "fedcba9876543210fedcba9876543210" },
        from: "stale-producer",
      });
    });

    await waitFor(() => expect(takePendingOpenMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(claimApiRequestMock).toHaveBeenCalledWith(preview.handoffId));
    expect(document.body.textContent).not.toContain("stale-producer");
  });

  it("restores a cancelled preview and never writes to the clipboard", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      mocks.order.push("take");
      return handoffRequest();
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<App />);

    await screen.findByRole("dialog", { name: "Webhook 요청 미리보기" });
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(restoreApiRequestMock).toHaveBeenCalledWith(preview.handoffId));
    expect(screen.queryByRole("dialog", { name: "Webhook 요청 미리보기" })).toBeNull();
    expect(writeText).not.toHaveBeenCalled();
  });

  it("returns a native claim if the renderer unmounts before claim resolves", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => handoffRequest());
    let resolveClaim!: (value: ApiRequestHandoffPreview) => void;
    claimApiRequestMock.mockImplementationOnce(
      () => new Promise<ApiRequestHandoffPreview>((resolve) => { resolveClaim = resolve; }),
    );
    render(<App />);

    await waitFor(() => expect(claimApiRequestMock).toHaveBeenCalledWith(preview.handoffId));
    cleanup();
    await act(async () => resolveClaim(preview));

    await waitFor(() => expect(restoreApiRequestMock).toHaveBeenCalledWith(preview.handoffId));
  });

  it("focuses the preview dialog and Escape restores the handoff", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => handoffRequest());
    render(<App />);

    const dialog = await screen.findByRole("dialog", { name: "Webhook 요청 미리보기" });
    const cancel = screen.getByRole("button", { name: "취소" });
    expect(document.activeElement).toBe(cancel);
    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => expect(restoreApiRequestMock).toHaveBeenCalledWith(preview.handoffId));
    expect(screen.queryByRole("dialog", { name: "Webhook 요청 미리보기" })).toBeNull();
  });

  it("surfaces a fixed expiry error without clipboard fallback", async () => {
    takePendingOpenMock.mockImplementationOnce(async () => {
      mocks.order.push("take");
      return handoffRequest();
    });
    claimApiRequestMock.mockRejectedValueOnce(new Error("handoff 요청이 만료되었거나 더 이상 사용할 수 없습니다"));
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<App />);

    expect(await screen.findByText("handoff 요청이 만료되었거나 더 이상 사용할 수 없습니다")).toBeTruthy();
    expect(screen.queryByRole("dialog", { name: "Webhook 요청 미리보기" })).toBeNull();
    expect(writeText).not.toHaveBeenCalled();
  });
});
