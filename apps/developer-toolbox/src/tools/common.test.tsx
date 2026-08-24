import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClipboardText } from "../api";
import { ToolOutput, ToolTextArea } from "./common";

vi.mock("../api", () => ({
  readClipboardText: vi.fn(),
}));

const readClipboardTextMock = vi.mocked(readClipboardText);
const writeTextMock = vi.fn<(value: string) => Promise<void>>();
const createObjectUrlMock = vi.fn<(blob: Blob) => string>();
const revokeObjectUrlMock = vi.fn<(url: string) => void>();
let clickedDownload = "";
let clickedHref = "";

function InputHarness({ initial = "alpha beta" }: { initial?: string }) {
  const [value, setValue] = useState(initial);
  return (
    <ToolTextArea
      aria-label="Tool input"
      value={value}
      onValueChange={setValue}
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

    expect(screen.getByRole("menu", { name: "Input actions" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Paste" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Select all" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Clear" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "Paste" }));

    await waitFor(() => expect(input.value).toBe("alpha pasted"));
    expect(readClipboardTextMock).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(input.selectionStart).toBe(12));
    expect(input.selectionEnd).toBe(12);
  });

  it("selects all and clears through controlled state", async () => {
    render(<InputHarness />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "Select all" }));
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "Clear" }));
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
    expect(screen.queryByRole("menu", { name: "Input actions" })).toBeNull();
  });

  it("shows a recoverable error without changing input when clipboard read fails", async () => {
    readClipboardTextMock.mockRejectedValueOnce(new Error("permission denied"));
    render(<InputHarness />);
    const input = screen.getByRole("textbox", { name: "Tool input" }) as HTMLTextAreaElement;

    openMenu(input);
    fireEvent.click(screen.getByRole("menuitem", { name: "Paste" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Clipboard read failed: permission denied",
    );
    expect(input.value).toBe("alpha beta");
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
    const output = screen.getByLabelText("Output");

    openMenu(output);
    expect(screen.getByRole("menuitem", { name: "Copy" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Select all" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Save result file" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("result text"));

    openMenu(output);
    fireEvent.click(screen.getByRole("menuitem", { name: "Select all" }));
    expect(window.getSelection()?.toString()).toBe("result text");

    openMenu(output);
    fireEvent.click(screen.getByRole("menuitem", { name: "Save result file" }));
    await waitFor(() => expect(createObjectUrlMock).toHaveBeenCalledTimes(1));
    expect(clickedDownload).toBe("toolbox-result.txt");
    expect(clickedHref).toBe("blob:toolbox-result");
    expect(revokeObjectUrlMock).toHaveBeenCalledWith("blob:toolbox-result");
  });

  it("disables all output actions for an empty result", () => {
    render(<ToolOutput className="io-output" value="" />);
    openMenu(screen.getByLabelText("Output"));

    for (const label of ["Copy", "Select all", "Save result file"]) {
      expect(screen.getByRole("menuitem", { name: label }).getAttribute("aria-disabled")).toBe(
        "true",
      );
    }
  });

  it("reports clipboard write failures without leaking or replacing the result", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("clipboard busy"));
    render(<ToolOutput className="io-output" value="safe result" />);
    const output = screen.getByLabelText("Output");

    openMenu(output);
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Output action failed: clipboard busy",
    );
    expect(output.textContent).toBe("safe result");
  });
});
