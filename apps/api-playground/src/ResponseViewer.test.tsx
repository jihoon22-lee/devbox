import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ResponseViewer } from "./ResponseViewer";
import type { ApiResponse } from "./types";

const writeTextMock = vi.fn<(text: string) => Promise<void>>();
const confirmMock = vi.fn<(message?: string) => boolean>();
const rawCopyMock = vi.fn<(kind: "headers" | "cookies", responseId: string) => Promise<string>>();
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

function renderViewer(value = response()) {
  return render(
    <ResponseViewer
      response={value}
      responseText={'{\n  "ok": true\n}'}
      pretty
      onPrettyChange={vi.fn()}
      onRawCopy={rawCopyMock}
      onError={errorMock}
    />,
  );
}

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  confirmMock.mockReset().mockReturnValue(false);
  rawCopyMock.mockReset().mockResolvedValue("set-cookie: session=raw-secret");
  errorMock.mockReset();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  Object.defineProperty(window, "confirm", { configurable: true, value: confirmMock });
});

afterEach(() => cleanup());

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
});
