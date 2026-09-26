import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { UndoProvider } from "@devbox/product-shell/undo";
import { useState } from "react";
import Daily from "./Daily";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/notes/api", () => ({ notesCall: invoke }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => invoke }));
afterEach(cleanup);
beforeEach(() => invoke.mockReset());
const created = { path: "Journal/2024-02-29.md", created: true, revision: "r1", indexed: true };
function show() {
  const onOpen = vi.fn();
  const onDateChange = vi.fn();
  const view = render(<Daily date="2024-02-29" onDateChange={onDateChange} onOpen={onOpen} onActivity={vi.fn()} />);
  return { ...view, onOpen, onDateChange };
}
it("creates and opens with one action then offers conditional undo", async () => {
  invoke.mockImplementation(async (method: string) => (method === "open_daily" ? created : { removed: true }));
  const { onOpen, container } = show();
  expect(invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 열기" }));
  await waitFor(() => expect(onOpen).toHaveBeenCalledWith(created.path));
  expect(invoke).toHaveBeenCalledWith("open_daily", { date: "2024-02-29" });
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("undo_created_note", { path: created.path, revision: created.revision }),
  );
});
it("keeps undo available after opening unmounts the daily screen", async () => {
  invoke.mockImplementation(async (method: string) => (method === "open_daily" ? created : { removed: true }));
  function Harness() {
    const [open, setOpen] = useState(false);
    return (
      <UndoProvider>
        {open ? (
          <p>노트 편집기</p>
        ) : (
          <Daily date="2024-02-29" onDateChange={vi.fn()} onOpen={() => setOpen(true)} onActivity={vi.fn()} />
        )}
      </UndoProvider>
    );
  }
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 열기" }));
  await screen.findByText("노트 편집기");
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("undo_created_note", { path: created.path, revision: created.revision }),
  );
});
it("opens existing notes without undo and date selection has no native side effect", async () => {
  invoke.mockResolvedValue({ ...created, created: false, revision: "" });
  const { onDateChange, onOpen } = show();
  fireEvent.change(screen.getByLabelText("일일 기록 날짜"), { target: { value: "2024-03-01" } });
  expect(onDateChange).toHaveBeenCalledWith("2024-03-01");
  expect(invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "일일 노트 열기" }));
  await waitFor(() => expect(onOpen).toHaveBeenCalledWith(created.path));
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
});
