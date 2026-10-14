import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Menu } from "../ui/Menu";
import { Select } from "../ui/Select";
import { useToasts } from "../ui/Toast";
import { useKeymap } from "./keymap";
import { useLayout } from "./layout";
import { restoreUiScale, setUiScale, uiBindings } from "./uiShortcuts";

beforeEach(() => {
  localStorage.clear();
  document.documentElement.style.zoom = "";
  useLayout.setState({ listCollapsed: false, bottomOpen: false });
});
afterEach(() => useToasts.getState().items.forEach((t) => useToasts.getState().dismiss(t.id)));
function Harness() {
  useKeymap(uiBindings());
  return (
    <>
      <input aria-label="입력" />
      <textarea aria-label="지시" />
      <select aria-label="선택">
        <option>하나</option>
      </select>
      <div contentEditable aria-label="편집" tabIndex={0} />
      <div data-keyscope="terminal">
        <input aria-label="터미널" />
      </div>
      <div data-keyscope="editor">
        <input aria-label="코드" />
      </div>
      <button type="button">본문</button>
    </>
  );
}
const key = (target: Element | Window, code: string, shiftKey = false) =>
  fireEvent.keyDown(target, { code, ctrlKey: true, shiftKey });
test("native undo stays available in every editable focus and absent toasts", () => {
  render(<Harness />);
  const undo = vi.fn();
  act(() => useToasts.getState().push({ tone: "info", title: "되돌릴 변경", undo }));
  for (const label of ["입력", "지시", "선택", "편집", "터미널", "코드"]) {
    const input = screen.getByLabelText(label);
    input.focus();
    expect(key(input, "KeyZ")).toBe(true);
  }
  expect(undo).not.toHaveBeenCalled();
  const body = screen.getByRole("button", { name: "본문" });
  body.focus();
  expect(key(body, "KeyZ")).toBe(false);
  expect(undo).toHaveBeenCalledOnce();
  expect(key(body, "KeyZ")).toBe(true);
});
test("UI scale persists and clamps while terminal/editor reset and bottom chord remain local", () => {
  render(<Harness />);
  const body = screen.getByRole("button", { name: "본문" });
  body.focus();
  for (let i = 0; i < 10; i++) key(body, "Equal");
  expect(document.documentElement.style.zoom).toBe("1.5");
  expect(localStorage.getItem("devbox.uiScale")).toBe("1.5");
  for (let i = 0; i < 10; i++) key(body, "Minus");
  expect(document.documentElement.style.zoom).toBe("0.9");
  key(body, "Digit0");
  expect(document.documentElement.style.zoom).toBe("1");
  for (const label of ["터미널", "코드"]) {
    const input = screen.getByLabelText(label);
    input.focus();
    setUiScale(1.2);
    expect(key(input, "Digit0")).toBe(true);
    expect(key(input, "Backquote")).toBe(true);
    expect(document.documentElement.style.zoom).toBe("1.2");
  }
  body.focus();
  key(body, "Backquote");
  key(body, "KeyB", true);
  expect(useLayout.getState()).toMatchObject({ bottomOpen: true, listCollapsed: true });
  localStorage.setItem("devbox.uiScale", "garbage");
  restoreUiScale();
  expect(document.documentElement.style.zoom).toBe("1");
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw Error();
  });
  expect(() => setUiScale(1.3)).not.toThrow();
});
test("Radix menus and selects own keys while open", async () => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
  function Popups() {
    useKeymap(uiBindings());
    return (
      <>
        <Menu trigger={<button type="button">더 보기</button>} items={[{ label: "열기", onSelect: vi.fn() }]} />
        <Select label="출처" value="a" onChange={() => {}} options={[{ value: "a", label: "터미널" }]} />
      </>
    );
  }
  render(<Popups />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "더 보기" }));
  await screen.findByRole("menu");
  expect(key(window, "KeyB", true)).toBe(true);
  expect(useLayout.getState().listCollapsed).toBe(false);
  await user.keyboard("{Escape}");
  screen.getByRole("combobox", { name: "출처" }).focus();
  await user.keyboard("{Enter}");
  await screen.findByRole("listbox");
  expect(key(window, "Backquote")).toBe(true);
  expect(useLayout.getState().bottomOpen).toBe(false);
  await user.keyboard("{Escape}");
});
