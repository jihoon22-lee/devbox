import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { SearchCommands } from "./SearchCommands";

afterEach(cleanup);
test("Ctrl+Shift+F yields to a focused terminal and ignores IME", () => {
  const navigate = vi.fn();
  render(
    <>
      <SearchCommands navigate={navigate} />
      <div data-keyscope="terminal">
        <textarea aria-label="terminal" />
      </div>
      <input aria-label="plain" />
    </>,
  );
  document.querySelector<HTMLTextAreaElement>("textarea")!.focus();
  fireEvent.keyDown(window, { code: "KeyF", ctrlKey: true, shiftKey: true });
  expect(navigate).not.toHaveBeenCalled();
  document.querySelector<HTMLInputElement>("input")!.focus();
  fireEvent.keyDown(window, { code: "KeyF", ctrlKey: true, shiftKey: true, isComposing: true });
  expect(navigate).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { code: "KeyF", ctrlKey: true, shiftKey: true });
  expect(navigate).toHaveBeenCalledTimes(1);
});
