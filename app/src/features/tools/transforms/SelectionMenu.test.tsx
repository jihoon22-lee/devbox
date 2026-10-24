import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { SelectionMenu } from "./SelectionMenu";

it("opens by Shift+F10, focuses the action, sends the captured selection and closes", async () => {
  const send = vi.fn();
  const get = vi.fn(() => "selected");
  render(
    <SelectionMenu label="선택 영역" getSelection={get} onSend={send}>
      <span>content</span>
    </SelectionMenu>,
  );
  const host = screen.getByRole("group", { name: "선택 영역" });
  host.focus();
  fireEvent.keyDown(host, { key: "F10", shiftKey: true });
  expect(screen.getByRole("menuitem")).toHaveFocus();
  get.mockReturnValue("");
  await userEvent.keyboard("{Enter}");
  expect(send).toHaveBeenCalledWith("selected");
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});
it("supports ContextMenu and Escape with focus restored, and ignores empty selections", () => {
  const get = vi.fn(() => "selected");
  render(
    <SelectionMenu label="선택 영역" getSelection={get} onSend={vi.fn()}>
      <span>content</span>
    </SelectionMenu>,
  );
  const host = screen.getByRole("group");
  host.focus();
  fireEvent.keyDown(host, { key: "ContextMenu" });
  fireEvent.keyDown(screen.getByRole("menuitem"), { key: "Escape" });
  expect(host).toHaveFocus();
  expect(screen.queryByRole("menu")).toBeNull();
  get.mockReturnValue("");
  fireEvent.contextMenu(host);
  expect(screen.queryByRole("menu")).toBeNull();
});
it("dismisses on outside pointer input", () => {
  render(
    <SelectionMenu label="선택 영역" getSelection={() => "selected"} onSend={vi.fn()}>
      <span>content</span>
    </SelectionMenu>,
  );
  fireEvent.contextMenu(screen.getByRole("group"));
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("menu")).toBeNull();
});
