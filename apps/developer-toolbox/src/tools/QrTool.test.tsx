import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QrTool } from "./QrTool";
import type { QrResult } from "./qr";

const { generateQrMock } = vi.hoisted(() => ({
  generateQrMock: vi.fn(),
}));

vi.mock("../api", () => ({
  generateQr: generateQrMock,
}));

const RESULT: QrResult = {
  svg: "<svg xmlns=\"http://www.w3.org/2000/svg\"><path/></svg>",
  pngBase64: "cG5n",
  width: 232,
  version: 1,
  modules: 21,
  quietZone: 4,
  payloadBytes: 5,
};

beforeEach(() => {
  generateQrMock.mockReset().mockResolvedValue(RESULT);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: vi.fn().mockResolvedValue(undefined),
      write: vi.fn().mockResolvedValue(undefined),
    },
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("QrTool", () => {
  it("generates only on explicit action and exposes accessible SVG/PNG actions", async () => {
    render(<QrTool />);
    const input = screen.getByRole("textbox", { name: "텍스트 payload" });
    expect(generateQrMock).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "QR 생성" }));

    expect(await screen.findByRole("img", { name: "생성된 QR 코드 미리보기" })).toBeTruthy();
    expect(screen.getByLabelText("QR SVG 결과").textContent).toContain(RESULT.svg);
    expect(screen.getByRole("button", { name: "PNG 복사" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "PNG 저장" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("232px");
  });

  it("shows a safe file-save fallback when image clipboard is unavailable", async () => {
    vi.stubGlobal("ClipboardItem", undefined);
    render(<QrTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "텍스트 payload" }), { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "QR 생성" }));
    await screen.findByRole("img", { name: "생성된 QR 코드 미리보기" });

    fireEvent.click(screen.getByRole("button", { name: "PNG 복사" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "이 환경에서는 PNG clipboard를 사용할 수 없습니다. PNG 저장을 사용하세요.",
    );
  });

  it("ignores duplicate clicks while busy and drops a late response after unmount", async () => {
    let resolve: (value: QrResult) => void = () => undefined;
    generateQrMock.mockReturnValueOnce(new Promise<QrResult>((next) => { resolve = next; }));
    const rendered = render(<QrTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "텍스트 payload" }), { target: { value: "hello" } });
    const button = screen.getByRole("button", { name: "QR 생성" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(generateQrMock).toHaveBeenCalledTimes(1);
    rendered.unmount();
    resolve(RESULT);
    await waitFor(() => expect(generateQrMock).toHaveBeenCalledTimes(1));
  });
});
