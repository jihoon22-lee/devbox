import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ResponseViewer, TOOLBOX_SELECTION_MESSAGES } from "./ResponseViewer";
import type { ApiResponse, ToolboxDispatch } from "./types";

const writeTextMock = vi.fn<(text: string) => Promise<void>>();
const confirmMock = vi.fn<(message?: string) => boolean>();
const rawCopyMock = vi.fn<(kind: "headers" | "cookies", responseId: string) => Promise<string>>();
const binarySaveMock = vi.fn<(responseId: string) => Promise<boolean>>();
const sendSelectionMock = vi.fn<(text: string) => Promise<ToolboxDispatch>>();
const errorMock = vi.fn<(message: string) => void>();

function response(overrides: Partial<ApiResponse> = {}): ApiResponse {
  return {
    status: 200,
    status_text: "OK",
    headers: [
      { key: "content-type", value: "application/json" },
      { key: "set-cookie", value: "[REDACTED]" },
    ],
    duration_ms: 12,
    size_bytes: 18,
    body: "{}",
    is_json: true,
    final_url: "https://api.example.com",
    redirects: [],
    cookies: [{
      name: "session",
      value: "[REDACTED]",
      attributes: [
        { key: "Path", value: "/" },
        { key: "HttpOnly", value: "" },
      ],
    }],
    response_id: "response-7",
    raw_headers_available: true,
    headers_truncated: false,
    ...overrides,
  };
}

function renderViewer(
  value = response(),
  options: { native?: boolean; responseText?: string } = {},
) {
  return render(
    <ResponseViewer
      response={value}
      responseText={options.responseText ?? '{\n  "ok": true\n}'}
      pretty
      onPrettyChange={vi.fn()}
      onRawCopy={rawCopyMock}
      onBinarySave={binarySaveMock}
      onSendSelection={sendSelectionMock}
      native={options.native ?? true}
      onError={errorMock}
    />,
  );
}

function selectBody(start: number, end: number): void {
  const body = screen.getByTestId("response-body");
  const text = body.firstChild;
  if (!(text instanceof Text)) throw new Error("response body text node missing");
  const range = document.createRange();
  range.setStart(text, start);
  range.setEnd(text, end);
  const selection = window.getSelection();
  if (!selection) throw new Error("selection unavailable");
  selection.removeAllRanges();
  selection.addRange(range);
  document.dispatchEvent(new Event("selectionchange"));
  fireEvent.mouseUp(body);
}

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  confirmMock.mockReset().mockReturnValue(false);
  rawCopyMock.mockReset().mockResolvedValue("set-cookie: session=raw-secret");
  binarySaveMock.mockReset().mockResolvedValue(true);
  sendSelectionMock.mockReset().mockResolvedValue({ handoffId: "handoff-1", redacted: false });
  errorMock.mockReset();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  Object.defineProperty(window, "confirm", { configurable: true, value: confirmMock });
});

afterEach(() => {
  window.getSelection()?.removeAllRanges();
  cleanup();
});

describe("ResponseViewer", () => {
  it("uses dedicated Body, Headers, and Cookies tabs and copies masked headers by default", async () => {
    renderViewer();
    const bodyTab = screen.getByRole("tab", { name: "Body" });
    expect(bodyTab.getAttribute("aria-selected")).toBe("true");
    bodyTab.focus();
    fireEvent.keyDown(bodyTab, { key: "ArrowRight" });
    const headersTab = screen.getByRole("tab", { name: "Headers (2)" });
    expect(headersTab.getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement).toBe(headersTab);

    expect(screen.getByText("set-cookie")).toBeTruthy();
    expect(screen.getByText("[REDACTED]")).toBeTruthy();
    expect(document.body.textContent).not.toContain("raw-secret");

    fireEvent.click(screen.getByRole("button", { name: "Copy masked headers" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(
      "content-type: application/json\nset-cookie: [REDACTED]",
    ));
    expect(confirmMock).not.toHaveBeenCalled();
    expect(rawCopyMock).not.toHaveBeenCalled();
  });

  it("does not request raw cookies until confirmation and copies only after approval", async () => {
    renderViewer();
    fireEvent.click(screen.getByRole("tab", { name: "Cookies (1)" }));
    const rawButton = screen.getByRole("button", { name: "Copy original cookies" });

    fireEvent.click(rawButton);
    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(rawCopyMock).not.toHaveBeenCalled();

    confirmMock.mockReturnValueOnce(true);
    fireEvent.click(rawButton);
    await waitFor(() => expect(rawCopyMock).toHaveBeenCalledWith("cookies", "response-7"));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(
      "set-cookie: session=raw-secret",
    ));
  });

  it("keeps raw copy disabled when native retention is unavailable or bounded", () => {
    renderViewer(response({
      response_id: null,
      raw_headers_available: false,
      headers_truncated: true,
    }));
    fireEvent.click(screen.getByRole("tab", { name: "Headers (2)" }));
    const rawHeaders = screen.getByRole("button", { name: "Copy original headers" }) as HTMLButtonElement;
    expect(rawHeaders.disabled).toBe(true);
    expect(screen.getByText(/100 rows \/ 64 KiB/u)).toBeTruthy();

    fireEvent.click(screen.getByRole("tab", { name: "Cookies (1)" }));
    const rawCookies = screen.getByRole("button", { name: "Copy original cookies" }) as HTMLButtonElement;
    expect(rawCookies.disabled).toBe(true);
  });

  it("reports a generic error without surfacing backend raw text", async () => {
    rawCopyMock.mockRejectedValueOnce(new Error("set-cookie: session=backend-secret"));
    confirmMock.mockReturnValueOnce(true);
    renderViewer();
    fireEvent.click(screen.getByRole("tab", { name: "Cookies (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy original cookies" }));

    await waitFor(() => expect(errorMock).toHaveBeenCalledWith(
      "원문 응답 Set-Cookie를 안전하게 복사하지 못했습니다.",
    ));
    expect(document.body.textContent).not.toContain("backend-secret");
  });

  it("keeps HTTP status errors distinct from projected GraphQL data and errors", () => {
    renderViewer(response({
      status: 400,
      status_text: "Bad Request",
      body: '{"data":{"viewer":null},"errors":[{"message":"field failed","path":["viewer"]}]}',
      graphql: {
        envelope: "valid",
        data: { viewer: null },
        errors: [{ message: "field failed", locations: [], path: ["viewer"] }],
        errors_truncated: false,
      },
    }));
    expect(screen.getByText("HTTP error (400)")).toBeTruthy();
    expect(screen.getByText("GraphQL envelope: valid")).toBeTruthy();
    expect(screen.getByRole("alert", { name: "GraphQL errors" }).textContent).toContain("field failed");
    expect(screen.getByText("GraphQL data")).toBeTruthy();
  });

  it("shows bounded binary type/size/hex previews and saves only after an explicit action", async () => {
    renderViewer(response({
      body: "",
      is_json: false,
      binary: {
        media_type: "application/octet-stream",
        size_bytes: 4,
        hex_preview: "89504e47",
        text_preview: null,
        hex_truncated: false,
        text_truncated: false,
        save_available: true,
      },
    }));

    expect(screen.getByText("application/octet-stream")).toBeTruthy();
    expect(screen.getByText(/4 bytes/u)).toBeTruthy();
    expect(screen.getByText("89504e47")).toBeTruthy();
    expect(binarySaveMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Save binary" }));
    await waitFor(() => expect(binarySaveMock).toHaveBeenCalledWith("response-7"));
  });

  it("disables binary save when native retention is unavailable", () => {
    renderViewer(response({
      binary: {
        media_type: "application/octet-stream",
        size_bytes: 3,
        hex_preview: "000102",
        text_preview: null,
        hex_truncated: false,
        text_truncated: false,
        save_available: false,
      },
    }));
    const save = screen.getByRole("button", { name: "Save binary" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    const send = screen.getByRole("button", { name: "Send selection to Developer Toolbox" }) as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    expect(document.body.textContent).not.toContain("raw-secret");
  });

  it("sends only a non-empty selection from the rendered response body", async () => {
    renderViewer(response({ headers: [{ key: "x-secret-path", value: "C:\\raw\\header" }] }), {
      responseText: "safe response body",
    });
    selectBody(0, 4);

    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));

    await waitFor(() => {
      expect(sendSelectionMock).toHaveBeenCalledWith("safe");
      expect(screen.getByRole("status").textContent).toBe(TOOLBOX_SELECTION_MESSAGES.success);
    });
    expect(sendSelectionMock).toHaveBeenCalledTimes(1);
    expect(document.body.textContent).not.toContain("C:\\raw\\header");
  });

  it("rejects an empty selection without invoking the native handoff", async () => {
    renderViewer(response(), { responseText: "safe body" });

    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));

    expect(sendSelectionMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("status")).textContent).toContain(TOOLBOX_SELECTION_MESSAGES.empty);
  });

  it("rejects a range that crosses outside the rendered response body", async () => {
    renderViewer(response(), { responseText: "safe body" });
    const body = screen.getByTestId("response-body");
    const outside = document.createElement("span");
    outside.textContent = "outside secret path";
    document.body.append(outside);
    const bodyText = body.firstChild;
    const outsideText = outside.firstChild;
    if (!(bodyText instanceof Text) || !(outsideText instanceof Text)) throw new Error("selection text missing");
    const range = document.createRange();
    range.setStart(bodyText, 0);
    range.setEnd(outsideText, outsideText.length);
    const selection = window.getSelection();
    if (!selection) throw new Error("selection unavailable");
    selection.removeAllRanges();
    selection.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));

    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));

    expect(sendSelectionMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("status")).textContent).toContain(TOOLBOX_SELECTION_MESSAGES.outside);
    outside.remove();
  });

  it("rejects a selection retained across a response/render revision", async () => {
    const view = renderViewer(response(), { responseText: "old body" });
    selectBody(0, 3);
    view.rerender(
      <ResponseViewer
        response={response({ status: 201, status_text: "Created" })}
        responseText="new body"
        pretty
        onPrettyChange={vi.fn()}
        onRawCopy={rawCopyMock}
        onBinarySave={binarySaveMock}
        onSendSelection={sendSelectionMock}
        native
        onError={errorMock}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));

    expect(sendSelectionMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("status")).textContent).toContain(TOOLBOX_SELECTION_MESSAGES.stale);
  });

  it("shows fixed native success and redaction feedback", async () => {
    renderViewer(response(), { responseText: "safe body" });
    selectBody(0, 4);
    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain(TOOLBOX_SELECTION_MESSAGES.success));

    cleanup();
    sendSelectionMock.mockResolvedValueOnce({ handoffId: "handoff-2", redacted: true });
    renderViewer(response(), { responseText: "safe body" });
    selectBody(0, 4);
    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain(TOOLBOX_SELECTION_MESSAGES.redacted));
  });

  it("maps native failures to fixed feedback and never renders the raw error", async () => {
    sendSelectionMock.mockRejectedValueOnce(new Error("C:\\Users\\private\\response-vault"));
    renderViewer(response(), { responseText: "safe body" });
    selectBody(0, 4);
    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toContain(TOOLBOX_SELECTION_MESSAGES.error));
    expect(document.body.textContent).not.toContain("response-vault");
    expect(document.body.textContent).not.toContain("C:\\Users\\private");
  });

  it("reports native-only availability in browser preview without clipboard fallback", async () => {
    renderViewer(response(), { native: false, responseText: "safe body" });
    selectBody(0, 4);
    fireEvent.click(screen.getByRole("button", { name: "Send selection to Developer Toolbox" }));

    expect(sendSelectionMock).not.toHaveBeenCalled();
    expect(writeTextMock).not.toHaveBeenCalled();
    expect((await screen.findByRole("status")).textContent).toContain(TOOLBOX_SELECTION_MESSAGES.nativeOnly);
  });
});
