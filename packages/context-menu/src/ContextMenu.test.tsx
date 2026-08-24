import { cleanup, createEvent, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { ContextMenu } from "./ContextMenu";
import type { ContextMenuEntry } from "./types";
import { useContextMenu, type UseContextMenuOptions } from "./useContextMenu";

afterEach(cleanup);

const ITEMS: readonly ContextMenuEntry[] = [
  { type: "item", id: "copy", label: "Copy", shortcut: "Ctrl+C" },
  { type: "item", id: "disabled", label: "Unavailable", disabled: true },
  { type: "separator", id: "main-separator" },
  {
    type: "submenu",
    id: "more",
    label: "More",
    items: [
      { type: "item", id: "child-one", label: "Child one" },
      { type: "item", id: "child-disabled", label: "Child disabled", disabled: true },
      { type: "item", id: "child-two", label: "Child two" },
    ],
  },
  { type: "item", id: "delete", label: "Delete", danger: true },
];

function Harness({ options }: { options?: UseContextMenuOptions }) {
  const controller = useContextMenu(options);
  const [selected, setSelected] = useState("");
  return (
    <>
      <button data-testid="trigger" {...controller.triggerProps}>
        Target
      </button>
      <output data-testid="selected">{selected}</output>
      <ContextMenu
        open={controller.open}
        anchor={controller.anchor}
        restoreFocusTo={controller.restoreFocusTo}
        items={ITEMS}
        onSelect={setSelected}
        onClose={controller.close}
        ariaLabel="Actions"
      />
    </>
  );
}

function InputHarness() {
  const controller = useContextMenu();
  return (
    <>
      <input data-testid="input" defaultValue="한글 text" {...controller.triggerProps} />
      <ContextMenu
        open={controller.open}
        anchor={controller.anchor}
        restoreFocusTo={controller.restoreFocusTo}
        items={ITEMS}
        onSelect={() => undefined}
        onClose={controller.close}
      />
    </>
  );
}

describe("context menu triggers", () => {
  it("opens at pointer coordinates and calls the app-owned pre-open hook first", () => {
    const onBeforeOpen = vi.fn();
    render(<Harness options={{ onBeforeOpen }} />);
    const trigger = screen.getByTestId("trigger");

    const dispatched = fireEvent.contextMenu(trigger, { clientX: 210, clientY: 140 });

    expect(dispatched).toBe(false);
    expect(onBeforeOpen).toHaveBeenCalledWith("pointer", trigger);
    const menu = screen.getByRole("menu", { name: "Actions" });
    expect(menu.style.left).toBe("210px");
    expect(menu.style.top).toBe("140px");
  });

  it("supports Shift+F10 and the Menu key and restores trigger focus", () => {
    render(<Harness />);
    const trigger = screen.getByTestId("trigger");
    trigger.focus();

    fireEvent.keyDown(trigger, { key: "F10", shiftKey: true });
    expect(screen.getByRole("menu", { name: "Actions" })).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Copy" }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(document.activeElement).toBe(trigger);

    fireEvent.keyDown(trigger, { key: "ContextMenu", code: "ContextMenu" });
    expect(screen.getByRole("menu", { name: "Actions" })).toBeTruthy();
  });

  it("does not consume clipboard shortcuts or composing IME keyboard events", () => {
    render(<InputHarness />);
    const input = screen.getByTestId("input") as HTMLInputElement;
    input.focus();
    input.setSelectionRange(1, 4);

    const copy = createEvent.keyDown(input, { key: "c", ctrlKey: true });
    fireEvent(input, copy);
    expect(copy.defaultPrevented).toBe(false);
    expect(input.selectionStart).toBe(1);
    expect(input.selectionEnd).toBe(4);

    const composingMenu = createEvent.keyDown(input, {
      key: "F10",
      shiftKey: true,
      keyCode: 229,
    });
    fireEvent(input, composingMenu);
    expect(composingMenu.defaultPrevented).toBe(false);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(input.value).toBe("한글 text");
  });
});

describe("ContextMenu keyboard and item semantics", () => {
  it("skips disabled/separator rows, wraps, and traps Tab focus", () => {
    render(<Harness />);
    fireEvent.contextMenu(screen.getByTestId("trigger"), { clientX: 10, clientY: 10 });
    const copy = screen.getByRole("menuitem", { name: "Copy" });
    const more = screen.getByRole("menuitem", { name: "More" });
    const danger = screen.getByRole("menuitem", { name: "Delete" });

    expect(document.activeElement).toBe(copy);
    fireEvent.keyDown(copy, { key: "ArrowDown" });
    expect(document.activeElement).toBe(more);
    fireEvent.keyDown(more, { key: "ArrowDown" });
    expect(document.activeElement).toBe(danger);
    fireEvent.keyDown(danger, { key: "ArrowDown" });
    expect(document.activeElement).toBe(copy);
    fireEvent.keyDown(copy, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(danger);
    fireEvent.keyDown(danger, { key: "Tab" });
    expect(document.activeElement).toBe(copy);
  });

  it("renders separator, disabled, shortcut, and danger states without invoking disabled rows", () => {
    render(<Harness />);
    fireEvent.contextMenu(screen.getByTestId("trigger"));

    expect(screen.getByRole("separator")).toBeTruthy();
    expect(screen.getByText("Ctrl+C")).toBeTruthy();
    const disabled = screen.getByRole("menuitem", { name: "Unavailable" });
    expect(disabled.getAttribute("aria-disabled")).toBe("true");
    const focusedBeforeHover = document.activeElement;
    fireEvent.mouseEnter(disabled);
    expect(document.activeElement).toBe(focusedBeforeHover);
    expect(disabled.tabIndex).toBe(-1);
    fireEvent.click(disabled);
    expect(screen.getByRole("menu", { name: "Actions" })).toBeTruthy();
    expect(screen.getByTestId("selected").textContent).toBe("");
    expect(screen.getByRole("menuitem", { name: "Delete" }).classList.contains("danger")).toBe(true);
  });

  it("keeps pointer hover and keyboard activation on the same item", () => {
    render(<Harness />);
    fireEvent.contextMenu(screen.getByTestId("trigger"));
    const danger = screen.getByRole("menuitem", { name: "Delete" });

    fireEvent.mouseEnter(danger);
    expect(document.activeElement).toBe(danger);
    fireEvent.keyDown(danger, { key: "Enter" });

    expect(screen.getByTestId("selected").textContent).toBe("delete");
    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
  });

  it("opens a submenu with ArrowRight, returns with ArrowLeft, and selects with Enter", () => {
    render(<Harness />);
    fireEvent.contextMenu(screen.getByTestId("trigger"));
    const copy = screen.getByRole("menuitem", { name: "Copy" });
    fireEvent.keyDown(copy, { key: "ArrowDown" });
    const more = screen.getByRole("menuitem", { name: "More" });

    fireEvent.keyDown(more, { key: "ArrowRight" });
    const childOne = screen.getByRole("menuitem", { name: "Child one" });
    expect(document.activeElement).toBe(childOne);
    fireEvent.keyDown(childOne, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Child two" }));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(more);

    fireEvent.keyDown(more, { key: "ArrowRight" });
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "Child one" }), { key: "Enter" });
    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(screen.getByTestId("selected").textContent).toBe("child-one");
  });

  it("closes on outside pointer input and restores focus", () => {
    render(<Harness />);
    const trigger = screen.getByTestId("trigger");
    trigger.focus();
    fireEvent.contextMenu(trigger);

    fireEvent.pointerDown(document.body);

    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("closes when the underlying viewport scrolls", () => {
    render(<Harness />);
    const trigger = screen.getByTestId("trigger");
    trigger.focus();
    fireEvent.contextMenu(trigger);

    fireEvent.scroll(document.body);

    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("keeps the menu open while its own bounded list scrolls", () => {
    render(<Harness />);
    fireEvent.contextMenu(screen.getByTestId("trigger"));

    fireEvent.scroll(screen.getByRole("menu", { name: "Actions" }));

    expect(screen.getByRole("menu", { name: "Actions" })).toBeTruthy();
  });
});
