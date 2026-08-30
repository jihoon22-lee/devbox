import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardText } from "../api";
import { LoremTool } from "./LoremTool";

vi.mock("../api", () => ({
  readClipboardText: vi.fn(),
}));

const readClipboardTextMock = vi.mocked(readClipboardText);
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";

beforeEach(() => {
  readClipboardTextMock.mockReset().mockResolvedValue("");
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:lorem-result");
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

function countInput(): HTMLInputElement {
  return screen.getByRole("textbox", { name: "Lorem 수량" }) as HTMLInputElement;
}

describe("LoremTool", () => {
  it("generates deterministic sentences and exposes explicit copy/save", async () => {
    render(<LoremTool />);
    fireEvent.change(screen.getByLabelText("Lorem 분량 단위"), { target: { value: "sentences" } });
    fireEvent.change(countInput(), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "생성" }));

    const output = screen.getByLabelText("Lorem 출력");
    expect(output.textContent).toBe(
      "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
    );
    expect(screen.getByRole("status").textContent).toBe("2개 문장을 생성했습니다.");
    expect(screen.getByRole("note").textContent).toContain("네트워크 요청");

    fireEvent.click(screen.getByRole("button", { name: "복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(output.textContent));
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(clickedDownload).toBe("lorem-ipsum.txt");
    expect(createObjectUrlMock).toHaveBeenCalledTimes(1);
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:lorem-result");
  });

  it("clears stale output and reports a fixed invalid-count error", () => {
    render(<LoremTool />);
    fireEvent.click(screen.getByRole("button", { name: "생성" }));
    expect(screen.getByLabelText("Lorem 출력").textContent).toContain("Lorem ipsum");

    fireEvent.change(countInput(), { target: { value: "101" } });
    expect(screen.getByRole("alert").textContent).toContain("1에서 100");
    expect(screen.getByLabelText("Lorem 출력").textContent).toBe(" ");
    expect((screen.getByRole("button", { name: "생성" }) as HTMLButtonElement).disabled).toBe(true);
    expect(countInput().getAttribute("aria-invalid")).toBe("true");
  });

  it("does not generate during IME composition and restores the action afterward", () => {
    render(<LoremTool />);
    const count = countInput();
    const generate = screen.getByRole("button", { name: "생성" });

    fireEvent.compositionStart(count);
    expect((generate as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(generate);
    expect(screen.getByLabelText("Lorem 출력").textContent).toBe(" ");
    fireEvent.compositionEnd(count);
    expect((generate as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(generate);
    expect(screen.getByLabelText("Lorem 출력").textContent).toContain("Lorem ipsum");
  });

  it("clips count paste and replaces clipboard failures with a fixed message", async () => {
    readClipboardTextMock.mockResolvedValueOnce("1234");
    render(<LoremTool />);
    const count = countInput();
    count.focus();
    count.setSelectionRange(0, count.value.length);
    fireEvent.contextMenu(count, { clientX: 14, clientY: 20 });
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    await waitFor(() => expect(count.value).toBe("123"));

    fireEvent.change(count, { target: { value: "3" } });
    readClipboardTextMock.mockRejectedValueOnce(new Error("C:\\private\\credential"));
    fireEvent.contextMenu(count, { clientX: 14, clientY: 20 });
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    await waitFor(() => {
      expect(screen.getAllByRole("alert").some((entry) =>
        entry.textContent === "Lorem 입력을 붙여넣지 못했습니다.",
      )).toBe(true);
    });
  });

  it("blocks duplicate copy actions and keeps output after a fixed clipboard error", async () => {
    let resolveCopy: () => void = () => undefined;
    writeTextMock.mockReturnValueOnce(new Promise<void>((resolve) => {
      resolveCopy = resolve;
    }));
    render(<LoremTool />);
    fireEvent.click(screen.getByRole("button", { name: "생성" }));

    const copy = screen.getByRole("button", { name: "복사" }) as HTMLButtonElement;
    fireEvent.click(copy);
    fireEvent.click(copy);
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));
    expect(copy.disabled).toBe(true);
    resolveCopy();
    await waitFor(() => expect(copy.disabled).toBe(false));

    writeTextMock.mockRejectedValueOnce(new Error("DO_NOT_REFLECT_PLATFORM_DETAIL"));
    fireEvent.click(copy);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("Lorem 결과를 클립보드에 복사하지 못했습니다.");
    expect(alert.textContent).not.toContain("DO_NOT_REFLECT_PLATFORM_DETAIL");
    expect(screen.getByLabelText("Lorem 출력").textContent).toContain("Lorem ipsum");
  });

  it("uses a fixed output context-menu error", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("/unsafe/private/path"));
    render(<LoremTool />);
    fireEvent.click(screen.getByRole("button", { name: "생성" }));
    const output = screen.getByLabelText("Lorem 출력");

    fireEvent.contextMenu(output, { clientX: 14, clientY: 20 });
    fireEvent.click(screen.getByRole("menuitem", { name: "복사" }));
    await waitFor(() => {
      expect(screen.getAllByRole("alert").some((entry) =>
        entry.textContent === "Lorem 결과 작업을 완료하지 못했습니다.",
      )).toBe(true);
    });
    expect(output.textContent).toContain("Lorem ipsum");
  });
});
