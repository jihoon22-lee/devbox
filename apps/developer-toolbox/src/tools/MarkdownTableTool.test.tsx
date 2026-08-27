import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardText } from "../api";
import { MarkdownTableTool } from "./MarkdownTableTool";

vi.mock("../api", () => ({
  readClipboardText: vi.fn(),
}));

const readClipboardTextMock = vi.mocked(readClipboardText);
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();

beforeEach(() => {
  readClipboardTextMock.mockReset().mockResolvedValue("");
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:formatted-table");
  revokeObjectUrlMock.mockReset();
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
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function input(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "Markdown 표 입력" }) as HTMLTextAreaElement;
}

describe("MarkdownTableTool", () => {
  it("formats a table and exposes explicit copy/save actions", async () => {
    render(<MarkdownTableTool />);
    fireEvent.change(input(), {
      target: { value: "| name | value |\n| --- | ---: |\n| devbox | 5 |" },
    });

    const output = screen.getByLabelText("Markdown 표 출력");
    await waitFor(() => expect(output.textContent).toContain("| name   | value |"));
    expect(output.textContent).toContain("| ------ | ----: |");
    expect(output.textContent).toContain("| devbox |     5 |");

    fireEvent.click(screen.getByRole("button", { name: "복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith(output.textContent));
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(createObjectUrlMock).toHaveBeenCalledTimes(1);
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:formatted-table");
  });

  it("renders cell text without executing tag-like content", async () => {
    render(<MarkdownTableTool />);
    fireEvent.change(input(), {
      target: { value: "| syntax | text |\n| --- | --- |\n| `code` | <img src=x onerror=alert(1)> |" },
    });

    const output = screen.getByLabelText("Markdown 표 출력");
    await waitFor(() => expect(output.textContent).toContain("<img src=x onerror=alert(1)>"));
    expect(output.textContent).toContain("`code`");
    expect(output.querySelector("img")).toBeNull();
  });

  it("shows fixed errors and keeps malformed output actions disabled", async () => {
    render(<MarkdownTableTool />);
    fireEvent.change(input(), {
      target: { value: "| valid | row |\n| --- | -- |\n| credential=secret |" },
    });

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("MALFORMED_SEPARATOR");
    expect(alert.textContent).not.toContain("credential=secret");
    expect(screen.getByLabelText("Markdown 표 출력").textContent).toBe(" ");
    expect((screen.getByRole("button", { name: "복사" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("uses fixed messages for direct, context-menu, and input paste failures", async () => {
    render(<MarkdownTableTool />);
    fireEvent.change(input(), { target: { value: "| value |\n| --- |\n| safe |" } });
    const output = screen.getByLabelText("Markdown 표 출력");
    await waitFor(() => expect(output.textContent).toContain("safe"));

    writeTextMock.mockRejectedValueOnce(new Error("C:\\secret\\token"));
    fireEvent.click(screen.getByRole("button", { name: "복사" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "변환 결과를 clipboard에 복사하지 못했습니다.",
    );

    createObjectUrlMock.mockImplementationOnce(() => {
      throw new Error("/unsafe/path");
    });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));
    await waitFor(() => {
      expect(screen.getAllByRole("alert").some((entry) =>
        entry.textContent === "변환 결과 파일을 저장하지 못했습니다.",
      )).toBe(true);
    });

    writeTextMock.mockRejectedValueOnce(new Error("credential=secret"));
    fireEvent.contextMenu(output, { clientX: 14, clientY: 20 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy" }));
    await waitFor(() => {
      expect(screen.getAllByRole("alert").some((entry) =>
        entry.textContent === "변환 결과 작업을 완료하지 못했습니다.",
      )).toBe(true);
    });

    readClipboardTextMock.mockRejectedValueOnce(new Error("C:\\private\\credential"));
    fireEvent.contextMenu(input(), { clientX: 14, clientY: 20 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Paste" }));
    await waitFor(() => {
      expect(screen.getAllByRole("alert").some((entry) =>
        entry.textContent === "표 입력을 붙여넣지 못했습니다.",
      )).toBe(true);
    });
  });

  it("keeps only the newest transform result and exposes an accessible busy status", async () => {
    render(<MarkdownTableTool />);
    const tableInput = input();
    fireEvent.change(tableInput, { target: { value: "| old | value |" } });
    fireEvent.change(tableInput, { target: { value: "| newest | value |" } });

    const composingKey = new KeyboardEvent("keydown", {
      key: "Enter",
      isComposing: true,
      bubbles: true,
      cancelable: true,
    });
    tableInput.dispatchEvent(composingKey);
    expect(composingKey.defaultPrevented).toBe(false);

    await waitFor(() => {
      expect(screen.getByLabelText("Markdown 표 출력").textContent).toContain("newest");
    });
    expect(screen.getByLabelText("Markdown 표 출력").textContent).not.toContain("old");
  });

  it("clips an oversized explicit paste to the UTF-8 input bound", async () => {
    readClipboardTextMock.mockResolvedValueOnce("x".repeat(1_000_100));
    render(<MarkdownTableTool />);
    const tableInput = input();
    fireEvent.contextMenu(tableInput, { clientX: 14, clientY: 20 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Paste" }));

    await waitFor(() => expect(tableInput.value.length).toBe(1_000_000));
    expect(new TextEncoder().encode(tableInput.value).byteLength).toBeLessThanOrEqual(
      1_000_000,
    );
  });
});
