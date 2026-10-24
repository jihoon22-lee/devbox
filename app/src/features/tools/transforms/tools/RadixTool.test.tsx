import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RadixTool } from "./RadixTool";

const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:radix-result");
  revokeObjectUrlMock.mockReset();
  clickedDownload = "";
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: createObjectUrlMock,
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    value: revokeObjectUrlMock,
  });
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
    clickedDownload = this.download;
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("RadixTool", () => {
  it("자동 prefix를 감지해 네 진법과 metadata를 표시하고 개별 결과를 복사한다", async () => {
    render(<RadixTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "진법 변환 입력" }), {
      target: { value: "0x2a" },
    });

    expect(screen.getByLabelText("BIN · 2진수 출력").textContent).toBe("0b101010");
    expect(screen.getByLabelText("OCT · 8진수 출력").textContent).toBe("0o52");
    expect(screen.getByLabelText("DEC · 10진수 출력").textContent).toBe("42");
    expect(screen.getByLabelText("HEX · 16진수 출력").textContent).toBe("0x2a");
    expect(screen.getByText("입력 16진수 · 2자리 · 6비트")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "HEX · 16진수 복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("0x2a"));
  });

  it("명시적 진법에서 prefix 없는 음수를 signed magnitude로 변환한다", () => {
    render(<RadixTool />);
    fireEvent.change(screen.getByLabelText("입력 진법"), { target: { value: "2" } });
    fireEvent.change(screen.getByRole("textbox", { name: "진법 변환 입력" }), {
      target: { value: "-1010" },
    });

    expect(screen.getByLabelText("DEC · 10진수 출력").textContent).toBe("-10");
    expect(screen.getByLabelText("HEX · 16진수 출력").textContent).toBe("-0xa");
    expect(screen.getByRole("note").textContent).toContain("two's complement 해석은 하지 않습니다");
    expect(screen.getByRole("note").textContent).toContain("자동으로 저장하거나 전송하지 않습니다");
  });

  it("invalid digit의 원문 위치를 표시하고 결과 action을 숨긴다", () => {
    render(<RadixTool />);
    fireEvent.change(screen.getByLabelText("입력 진법"), { target: { value: "8" } });
    fireEvent.change(screen.getByRole("textbox", { name: "진법 변환 입력" }), {
      target: { value: "19" },
    });

    expect(screen.getByRole("alert").textContent).toContain("2번째 문자");
    expect(screen.getByRole("alert").textContent).toContain("INVALID_DIGIT");
    expect(screen.queryByRole("button", { name: "전체 복사" })).toBeNull();
    expect((screen.getByRole("button", { name: "전체 결과 저장" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("전체 canonical 결과를 복사하고 text file로 저장한다", async () => {
    render(<RadixTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "진법 변환 입력" }), {
      target: { value: "42" },
    });

    const expected = "BIN 0b101010\nOCT 0o52\nDEC 42\nHEX 0x2a";
    fireEvent.click(screen.getByRole("button", { name: "전체 복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(expected));

    fireEvent.click(screen.getByRole("button", { name: "전체 결과 저장" }));
    expect(clickedDownload).toBe("radix-conversion.txt");
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:radix-result");
  });

  it("clipboard 실패 원문을 반향하지 않고 결과를 유지한다", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("DO_NOT_REFLECT_CLIPBOARD_SECRET"));
    render(<RadixTool />);
    fireEvent.change(screen.getByRole("textbox", { name: "진법 변환 입력" }), {
      target: { value: "42" },
    });

    fireEvent.click(screen.getByRole("button", { name: "DEC · 10진수 복사" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("변환 결과를 클립보드에 복사하지 못했습니다.");
    expect(alert.textContent).not.toContain("DO_NOT_REFLECT_CLIPBOARD_SECRET");
    expect(screen.getByLabelText("DEC · 10진수 출력").textContent).toBe("42");
  });
});
