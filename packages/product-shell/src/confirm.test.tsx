import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ConfirmationHost, confirmAction } from "./confirm";

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = vi.fn(function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  });
  HTMLDialogElement.prototype.close = vi.fn(function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  });
});
afterEach(cleanup);
it("fails closed without a host and cancels a pending request on unmount", async () => {
  expect(await confirmAction("delete")).toBe(false);
  const view = render(<ConfirmationHost />);
  let result!: Promise<boolean>;
  act(() => {
    result = confirmAction("delete");
  });
  expect(await confirmAction("another delete")).toBe(false);
  view.unmount();
  expect(await result).toBe(false);
});
it("requires an explicit decision, starts on Cancel, and restores focus after closing", async () => {
  const origin = document.createElement("button");
  document.body.append(origin);
  origin.focus();
  render(<ConfirmationHost />);
  let result!: Promise<boolean>;
  act(() => {
    result = confirmAction("delete the synthetic item?");
  });
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
  fireEvent.keyDown(screen.getByRole("dialog", { name: "작업 확인" }), { key: "Escape" });
  expect(await result).toBe(false);
  expect(document.activeElement).toBe(origin);
  act(() => {
    result = confirmAction("delete the synthetic item?");
  });
  fireEvent.click(screen.getByRole("button", { name: "확인" }));
  expect(await result).toBe(true);
  origin.remove();
});
it("a dialog opening error never authorizes the operation", async () => {
  HTMLDialogElement.prototype.showModal = () => {
    throw new Error("unavailable");
  };
  render(<ConfirmationHost />);
  let result!: Promise<boolean>;
  act(() => {
    result = confirmAction("delete");
  });
  expect(await result).toBe(false);
});
it("an old dialog decision cannot approve a newer review before React commits", async () => {
  render(<ConfirmationHost />);
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => {
    first = confirmAction("first synthetic deletion");
  });
  const oldAccept = screen.getByRole("button", { name: "확인" });
  let secondSettled = false;
  act(() => {
    fireEvent.click(oldAccept);
    second = confirmAction("second synthetic deletion");
    void second.then(() => {
      secondSettled = true;
    });
    fireEvent.click(oldAccept);
  });
  expect(await first).toBe(true);
  await Promise.resolve();
  expect(secondSettled).toBe(false);
  expect(screen.getByText("second synthetic deletion")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(await second).toBe(false);
});
