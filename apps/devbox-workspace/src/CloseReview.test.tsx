import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import CloseReview from "./CloseReview";
const showModal = vi.fn(function (this: HTMLDialogElement) {
  this.setAttribute("open", "");
});
const close = vi.fn(function (this: HTMLDialogElement) {
  this.removeAttribute("open");
});
beforeEach(() => {
  showModal.mockClear();
  close.mockClear();
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", { configurable: true, value: showModal });
  Object.defineProperty(HTMLDialogElement.prototype, "close", { configurable: true, value: close });
});
afterEach(() => {
  cleanup();
  document.body.replaceChildren();
});
it("owns the native modal layer and initially focuses cancel", () => {
  const origin = document.createElement("button");
  document.body.append(origin);
  origin.focus();
  const view = render(
    <CloseReview
      reasons={["Files 편집"]}
      filesDirty
      onSave={vi.fn()}
      onDiscard={vi.fn()}
      onCancel={vi.fn()}
      onReturn={vi.fn()}
    />,
  );
  const dialog = view.getByRole("dialog", { name: "Workspace 종료 검토" });
  expect(dialog).toBeInstanceOf(HTMLDialogElement);
  expect(showModal).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(view.getByRole("button", { name: "종료 취소" }));
  view.unmount();
  expect(close).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(origin);
  origin.remove();
});
it("Escape preserves an in-flight save and only cancels once it has settled", async () => {
  let rejectSave!: (error: Error) => void;
  const save = vi.fn(
    () =>
      new Promise<void>((_resolve, reject) => {
        rejectSave = reject;
      }),
  );
  const cancel = vi.fn(async () => {});
  const view = render(
    <CloseReview reasons={[]} filesDirty onSave={save} onDiscard={vi.fn()} onCancel={cancel} onReturn={vi.fn()} />,
  );
  const dialog = view.getByRole("dialog");
  fireEvent.click(view.getByRole("button", { name: "파일 저장 후 종료" }));
  const pendingEscape = new Event("cancel", { cancelable: true });
  fireEvent(dialog, pendingEscape);
  expect(pendingEscape.defaultPrevented).toBe(true);
  expect(cancel).not.toHaveBeenCalled();
  rejectSave(new Error("복구 기록 실패"));
  await waitFor(() => expect(view.getByRole("button", { name: "종료 취소" }).hasAttribute("disabled")).toBe(false));
  const settledEscape = new Event("cancel", { cancelable: true });
  fireEvent(dialog, settledEscape);
  await waitFor(() => expect(cancel).toHaveBeenCalledTimes(1));
  expect(settledEscape.defaultPrevented).toBe(true);
});
it("keeps the review visible and reports failed flush without approving close", async () => {
  const save = vi.fn(async () => {
    throw new Error("복구 기록 실패");
  });
  const cancel = vi.fn(async () => {});
  const view = render(
    <CloseReview
      reasons={["Files 편집"]}
      filesDirty
      onSave={save}
      onDiscard={vi.fn()}
      onCancel={cancel}
      onReturn={vi.fn()}
    />,
  );
  fireEvent.click(view.getByRole("button", { name: "파일 저장 후 종료" }));
  await waitFor(() => expect(view.getByRole("alert").textContent).toContain("복구 기록 실패"));
  expect(view.getByRole("dialog")).toBeDefined();
  expect(cancel).not.toHaveBeenCalled();
});

it("returning to the editor serializes native cancellation and keeps a rejected review visible", async () => {
  let rejectCancel!: (error: Error) => void;
  const pending = new Promise<void>((_resolve, reject) => {
    rejectCancel = reject;
  });
  const navigate = vi.fn();
  const returning = vi.fn(() => pending.then(navigate));
  const view = render(
    <CloseReview
      reasons={["Source 초안"]}
      filesDirty={false}
      onSave={vi.fn()}
      onDiscard={vi.fn()}
      onCancel={vi.fn()}
      onReturn={returning}
    />,
  );
  const button = view.getByRole("button", { name: "편집 화면으로 돌아가기" });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(returning).toHaveBeenCalledTimes(1);
  expect(button.hasAttribute("disabled")).toBe(true);
  rejectCancel(new Error("종료 취소 실패"));
  await waitFor(() => expect(view.getByRole("alert").textContent).toBe("종료 취소 실패"));
  expect(navigate).not.toHaveBeenCalled();
  expect(view.getByRole("dialog")).toBeDefined();
  expect(button.hasAttribute("disabled")).toBe(false);
});

it("composing Escape cannot trigger the browser's native dialog cancellation", () => {
  const cancel = vi.fn();
  const view = render(
    <CloseReview
      reasons={[]}
      filesDirty={false}
      onSave={vi.fn()}
      onDiscard={vi.fn()}
      onCancel={cancel}
      onReturn={vi.fn()}
    />,
  );
  const key = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true, isComposing: true });
  fireEvent(view.getByRole("dialog"), key);
  expect(key.defaultPrevented).toBe(true);
  expect(cancel).not.toHaveBeenCalled();
});
