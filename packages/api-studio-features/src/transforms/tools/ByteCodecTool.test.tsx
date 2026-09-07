import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ByteCodecTool } from "./ByteCodecTool";

const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";

beforeEach(() => {
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:byte-codec-result");
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

function input(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: /입력$/u }) as HTMLTextAreaElement;
}

describe("ByteCodecTool", () => {
  it("기본 UTF-8 → Base64 변환과 byte count, 복사·저장을 제공한다", async () => {
    render(<ByteCodecTool />);
    fireEvent.change(input(), { target: { value: "안녕" } });

    const output = screen.getByLabelText("Base64 출력");
    expect(output.textContent).toBe("7JWI64WV");
    expect(screen.getByText(/6바이트/u)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("7JWI64WV"));

    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(clickedDownload).toBe("converted.base64.txt");
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:byte-codec-result");
  });

  it("Hex raw byte를 Base64URL로 바꾸고 결과로 입출력을 교환한다", () => {
    render(<ByteCodecTool />);
    fireEvent.change(screen.getByLabelText("입력 형식"), { target: { value: "hex" } });
    fireEvent.change(screen.getByLabelText("출력 형식"), { target: { value: "base64url" } });
    fireEvent.change(input(), { target: { value: "fbff" } });

    expect(screen.getByLabelText("Base64URL 출력").textContent).toBe("-_8");
    fireEvent.click(screen.getByRole("button", { name: "결과로 입출력 교환" }));

    expect((screen.getByLabelText("입력 형식") as HTMLSelectElement).value).toBe("base64url");
    expect((screen.getByLabelText("출력 형식") as HTMLSelectElement).value).toBe("hex");
    expect(input().value).toBe("-_8");
    expect(screen.getByLabelText("Hex 원시 바이트 출력").textContent).toBe("fbff");
  });

  it("invalid encoded character의 원문 위치를 표시하고 action을 막는다", () => {
    render(<ByteCodecTool />);
    fireEvent.change(screen.getByLabelText("입력 형식"), { target: { value: "hex" } });
    fireEvent.change(input(), { target: { value: "de ad zg" } });

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("7번째 문자");
    expect(alert.textContent).toContain("INVALID_HEX_CHARACTER");
    expect((screen.getByRole("button", { name: "복사" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "저장" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("invalid UTF-8의 raw byte 위치와 text/raw byte 차이를 계속 안내한다", () => {
    render(<ByteCodecTool />);
    expect(screen.getByRole("note").textContent).toContain("유효하지 않은 UTF-8");
    expect(screen.getByRole("note").textContent).toContain("Base64는 암호화가 아니며");

    fireEvent.change(screen.getByLabelText("입력 형식"), { target: { value: "hex" } });
    fireEvent.change(screen.getByLabelText("출력 형식"), { target: { value: "utf8" } });
    fireEvent.change(input(), { target: { value: "e228a1" } });

    expect(screen.getByRole("alert").textContent).toContain("2번째 바이트");
    expect(screen.getByRole("alert").textContent).toContain("INVALID_UTF8_BYTES");
  });
});
