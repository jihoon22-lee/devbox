import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ConfirmDialog from "./ConfirmDialog";

afterEach(() => cleanup());

function Harness({ onConfirm = vi.fn() }: { onConfirm?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>Open confirmation</button>
      {open ? (
        <ConfirmDialog
          title="Confirm action"
          summary={["Safe summary only"]}
          confirmLabel="Run action"
          onCancel={() => setOpen(false)}
          onConfirm={onConfirm}
        />
      ) : null}
    </>
  );
}

describe("ConfirmDialog", () => {
  it("focuses Cancel by default, traps Tab, and exposes an accessible dialog", () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open confirmation" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Confirm action" });
    const cancel = screen.getByRole("button", { name: "취소" });
    const confirm = screen.getByRole("button", { name: "Run action" });

    expect(document.activeElement).toBe(cancel);
    confirm.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement).toBe(cancel);
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(confirm);

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("runs the confirm callback without rendering untrusted values", () => {
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    fireEvent.click(screen.getByRole("button", { name: "Open confirmation" }));
    fireEvent.click(screen.getByRole("button", { name: "Run action" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Safe summary only")).toBeTruthy();
    expect(document.body.textContent).not.toContain("credential");
  });
});
