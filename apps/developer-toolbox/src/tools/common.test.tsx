import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardText } from "../api";
import { ToolOutput, ToolTextArea, TransformerTool } from "./common";

vi.mock("../api", () => ({
  createApiRequestHandoff: vi.fn(),
  readClipboardText: vi.fn(),
}));

const readClipboardTextMock = vi.mocked(readClipboardText);
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";
let clickedHref = "";

function InputHarness({
  initial = "alpha beta",
  maxPasteBytes,
}: {
  initial?: string;
  maxPasteBytes?: number;
}) {
  const [value, setValue] = useState(initial);
  return (
    <ToolTextArea
      aria-label="Tool input"
      value={value}
      onValueChange={setValue}
      maxPasteBytes={maxPasteBytes}
      rows={4}
    />
  );
}

function openMenu(target: HTMLElement): void {
  fireEvent.contextMenu(target, { clientX: 14, clientY: 20 });
}

beforeEach(() => {
  readClipboardTextMock.mockReset().mockResolvedValue("pasted");
  writeTextMock.mockReset().mockResolvedValue(undefined);
  createObjectUrlMock.mockReset().mockReturnValue("blob:toolbox-result");
  revokeObjectUrlMock.mockReset();
  clickedDownload = "";
  clickedHref = "";
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
    clickedHref = this.href;
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("ToolTextArea context menu", () => {
  it("provides the exact input actions and pastes over the captured selection", async () => {
    render(<InputHarness />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;
    input.focus();
    input.setSelectionRange(6, 10);

    openMenu(input);

    expect(screen.getByRole("menu", { name: "입력 작업" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "붙여넣기" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "모두 선택" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "지우기" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));

    await waitFor(() => expect(input.value).toBe("alpha pasted"));
    expect(readClipboardTextMock).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(input.selectionStart).toBe(12));
    expect(input.selectionEnd).toBe(12);
  });

  it("selects all and clears through controlled state", async () => {
    render(<InputHarness />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "모두 선택" }));
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "지우기" }));
    await waitFor(() => expect(input.value).toBe(""));
    await waitFor(() => expect(document.activeElement).toBe(input));
  });

  it("keeps cut/copy/paste/undo and IME composition keyboard events untouched", () => {
    render(<InputHarness />);
    const input = screen.getByRole("textbox", { name: "Tool input" });

    for (const key of ["x", "c", "v", "z"]) {
      const event = new KeyboardEvent("keydown", {
        key,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      });
      input.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }

    const compositionEvent = new KeyboardEvent("keydown", {
      key: "F10",
      shiftKey: true,
      bubbles: true,
      cancelable: true,
      isComposing: true,
    });
    input.dispatchEvent(compositionEvent);
    expect(compositionEvent.defaultPrevented).toBe(false);
    expect(screen.queryByRole("menu", { name: "입력 작업" })).toBeNull();
  });

  it("shows a recoverable error without changing input when clipboard read fails", async () => {
    readClipboardTextMock.mockRejectedValueOnce(new Error("permission denied"));
    render(<InputHarness />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "클립보드를 읽지 못했습니다: permission denied",
    );
    expect(input.value).toBe("alpha beta");
  });

  it("bounds the resulting UTF-8 value without splitting multibyte characters", async () => {
    readClipboardTextMock.mockResolvedValueOnce("😀z");
    render(<InputHarness initial="a가" maxPasteBytes={8} />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;
    input.focus();
    input.setSelectionRange(1, 1);

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));

    await waitFor(() => expect(input.value).toBe("a😀가"));
    expect(new TextEncoder().encode(input.value).byteLength).toBe(8);
  });

  it("stops before malformed Unicode and discards a paste after the value changes", async () => {
    readClipboardTextMock.mockResolvedValueOnce(`a${"\ud800"}b`);
    const view = render(<InputHarness initial="" maxPasteBytes={10} />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;
    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    await waitFor(() => expect(input.value).toBe("a"));

    let resolvePaste: (value: string) => void = () => undefined;
    readClipboardTextMock.mockReturnValueOnce(new Promise<string>((resolve) => {
      resolvePaste = resolve;
    }));
    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    fireEvent.change(input, { target: { value: "new" } });
    await act(async () => {
      resolvePaste("stale");
      await Promise.resolve();
    });
    expect(input.value).toBe("new");

    let resolveUnmountedPaste: (value: string) => void = () => undefined;
    readClipboardTextMock.mockReturnValueOnce(new Promise<string>((resolve) => {
      resolveUnmountedPaste = resolve;
    }));
    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    view.unmount();
    await act(async () => {
      resolveUnmountedPaste("after-unmount");
      await Promise.resolve();
    });
  });
});

describe("ToolOutput context menu", () => {
  it("copies, selects, and downloads the exact visible result", async () => {
    render(
      <ToolOutput
        className="io-output"
        value="result text"
        downloadName="toolbox-result.txt"
      />,
    );
    const output = screen.getByLabelText("출력");

    openMenu(output);
    expect(screen.getByRole("menuitem", { name: "복사" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "모두 선택" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "결과 파일 저장" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "복사" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("result text"));

    openMenu(output);
    fireEvent.click(screen.getByRole("menuitem", { name: "모두 선택" }));
    expect(window.getSelection()?.toString()).toBe("result text");

    openMenu(output);
    fireEvent.click(screen.getByRole("menuitem", { name: "결과 파일 저장" }));
    await waitFor(() => expect(createObjectUrlMock).toHaveBeenCalledTimes(1));
    expect(clickedDownload).toBe("toolbox-result.txt");
    expect(clickedHref).toBe("blob:toolbox-result");
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:toolbox-result");
  });

  it("disables all output actions for an empty result", () => {
    render(<ToolOutput className="io-output" value="" />);
    openMenu(screen.getByLabelText("출력"));

    for (const label of ["복사", "모두 선택", "결과 파일 저장"]) {
      expect(screen.getByRole("menuitem", { name: label }).getAttribute("aria-disabled")).toBe(
        "true",
      );
    }
  });

  it("reports clipboard write failures without leaking or replacing the result", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("clipboard busy"));
    render(<ToolOutput className="io-output" value="safe result" />);
    const output = screen.getByLabelText("출력");

    openMenu(output);
    fireEvent.click(screen.getByRole("menuitem", { name: "복사" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "출력 작업을 완료하지 못했습니다: clipboard busy",
    );
    expect(output.textContent).toBe("safe result");
  });
});

describe("useAsyncTransform cancellation", () => {
  it("aborts the superseded request and the unmounted request", () => {
    const signals: AbortSignal[] = [];
    const run = (_input: string, signal?: AbortSignal) => {
      if (signal) signals.push(signal);
      return new Promise<{ output: string }>(() => undefined);
    };
    const view = render(
      <TransformerTool
        placeholder="Text"
        run={run}
        clearOutputOnInput
      />,
    );
    const input = screen.getByRole("textbox", { name: "입력" });

    fireEvent.change(input, { target: { value: "first" } });
    expect(signals[signals.length - 1]?.aborted).toBe(false);
    fireEvent.change(input, { target: { value: "second" } });
    expect(signals[0]?.aborted).toBe(true);
    expect(signals[signals.length - 1]?.aborted).toBe(false);

    view.unmount();
    expect(signals[signals.length - 1]?.aborted).toBe(true);
  });
});
