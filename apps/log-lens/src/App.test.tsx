import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const writeText = vi.fn<(value: string) => Promise<void>>();

beforeEach(() => {
  writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
});

afterEach(() => cleanup());

describe("Log Lens bounded UI", () => {
  it("offers an accessible log-line context menu and restores row focus", async () => {
    render(<App />);
    const row = screen.getAllByRole("row")[1] as HTMLDivElement;
    row.focus();

    fireEvent.contextMenu(row, { clientX: 18, clientY: 24 });
    expect(screen.getByRole("menu", { name: "Log line actions" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "Add bookmark" }));

    await waitFor(() => expect(document.activeElement).toBe(row));
    expect(within(row).getByRole("button", { name: "Remove bookmark" })).toBeTruthy();

    fireEvent.keyDown(row, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy log line" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(document.activeElement).toBe(row);
  });

  it("shows wrap controls and reports the source cap instead of dropping input", async () => {
    render(<App />);
    const add = screen.getByRole("button", { name: "Add source" });
    const form = add.closest("form");
    const path = screen.getByPlaceholderText("C:\\logs\\app.log");
    if (!form) throw new Error("source form missing");

    expect(screen.getByLabelText("Wrap lines")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Wrap lines"));
    expect(screen.getAllByRole("row")[1].querySelector(".message.nowrap")).toBeTruthy();

    for (let index = 0; index < 15; index += 1) {
      fireEvent.change(path, { target: { value: `fixture-${index}.log` } });
      fireEvent.submit(form);
      await waitFor(() => expect((add as HTMLButtonElement).disabled).toBe(false));
    }
    expect(screen.getByText(/16\/16 selected/)).toBeTruthy();

    fireEvent.change(path, { target: { value: "fixture-over-cap.log" } });
    fireEvent.submit(form);
    expect((await screen.findByRole("alert")).textContent).toContain(
      "A maximum of 16 sources can be loaded at once.",
    );
  });

  it("confirms saved-view updates and supports removal", async () => {
    render(<App />);
    fireEvent.change(screen.getByPlaceholderText("view name"), { target: { value: "Errors" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect((await screen.findByRole("status")).textContent).toContain("Saved view “Errors” saved.");

    const load = screen.getByLabelText("Load saved view");
    fireEvent.change(load, { target: { value: "Errors" } });
    expect((await screen.findByRole("status")).textContent).toContain("Loaded saved view “Errors”.");
    fireEvent.click(screen.getByRole("button", { name: "Remove view" }));
    expect((await screen.findByRole("status")).textContent).toContain("Saved view “Errors” removed.");
    expect(within(load).queryByRole("option", { name: "Errors" })).toBeNull();
  });
});
