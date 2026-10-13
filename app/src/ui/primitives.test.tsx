import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createRef, useState } from "react";
import { Menu } from "./Menu";
import { Select } from "./Select";
import { SplitPane } from "./SplitPane";
import { Tabs } from "./Tabs";
import { TextArea } from "./TextArea";

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});

function pointer(target: Element | Window, type: string, props: { x?: number; y?: number; id?: number } = {}) {
  const event = new MouseEvent(type, { bubbles: true, button: 0, clientX: props.x ?? 0, clientY: props.y ?? 0 });
  Object.defineProperty(event, "pointerId", { value: props.id ?? 1 });
  fireEvent(target, event);
}

test("tabs move focus and selection with arrows/Home/End and close without selecting", async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  function Example() {
    const [active, setActive] = useState("a");
    return (
      <Tabs
        items={[
          { id: "a", label: "터미널", closable: true },
          { id: "b", label: "로그" },
          { id: "c", label: "문제" },
        ]}
        active={active}
        onSelect={setActive}
        onClose={close}
      />
    );
  }
  render(<Example />);
  await user.tab();
  await user.keyboard("{ArrowRight}");
  expect(screen.getByRole("tab", { name: "로그" })).toHaveFocus();
  expect(screen.getByRole("tab", { name: "로그" })).toHaveAttribute("aria-selected", "true");
  await user.keyboard("{End}");
  expect(screen.getByRole("tab", { name: "문제" })).toHaveFocus();
  await user.keyboard("{Home}");
  expect(screen.getByRole("tab", { name: "터미널" })).toHaveFocus();
  await user.click(screen.getByRole("button", { name: "터미널 닫기" }));
  fireEvent(screen.getByRole("tab", { name: "터미널" }), new MouseEvent("auxclick", { bubbles: true, button: 1 }));
  expect(close).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("tab", { name: "터미널" })).toHaveAttribute("aria-selected", "true");
});

test("split keyboard uses only its axis, clamps values and prevents scrolling", () => {
  const change = vi.fn();
  render(
    <SplitPane dir="row" ratio={0.16} onRatio={change}>
      {[<div key="a">왼쪽</div>, <div key="b">오른쪽</div>]}
    </SplitPane>,
  );
  const handle = screen.getByRole("separator");
  expect(fireEvent.keyDown(handle, { key: "ArrowLeft" })).toBe(false);
  expect(change).toHaveBeenLastCalledWith(0.15);
  fireEvent.keyDown(handle, { key: "ArrowUp" });
  expect(change).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(handle, { key: "End" });
  expect(change).toHaveBeenLastCalledWith(0.85);
});

test("split ignores unusable dimensions and removes drag listeners on cancel/unmount", () => {
  const change = vi.fn();
  const view = render(
    <SplitPane dir="column" ratio={Number.NaN} min={Number.NaN} onRatio={change}>
      {[<div key="a" />, <div key="b" />]}
    </SplitPane>,
  );
  const handle = screen.getByRole("separator");
  expect(handle).toHaveAttribute("aria-valuenow", "50");
  pointer(handle, "pointerdown");
  pointer(window, "pointermove", { y: 100 });
  expect(change).not.toHaveBeenCalled();
  pointer(window, "pointercancel");
  vi.spyOn(handle.parentElement as HTMLElement, "getBoundingClientRect").mockReturnValue({
    top: 10,
    left: 0,
    width: 200,
    height: 200,
    right: 200,
    bottom: 210,
    x: 0,
    y: 10,
    toJSON: () => ({}),
  });
  pointer(handle, "pointerdown");
  pointer(window, "pointermove", { y: 110, id: 2 });
  expect(change).not.toHaveBeenCalled();
  pointer(window, "pointermove", { y: 110 });
  expect(change).toHaveBeenLastCalledWith(0.5);
  pointer(window, "pointercancel");
  pointer(window, "pointermove", { y: 170 });
  expect(change).toHaveBeenCalledTimes(1);
  pointer(handle, "pointerdown");
  view.unmount();
  pointer(window, "pointermove", { y: 200 });
  expect(change).toHaveBeenCalledTimes(1);
});

test("textarea resizes uncontrolled input and controlled updates while preserving refs/handlers", () => {
  const ref = createRef<HTMLTextAreaElement>();
  const input = vi.fn();
  const view = render(<TextArea aria-label="지시" ref={ref} onInput={input} maxRows={2} />);
  const area = screen.getByRole("textbox");
  Object.defineProperty(area, "scrollHeight", { configurable: true, value: 100 });
  fireEvent.input(area, { target: { value: "긴 지시" } });
  expect(ref.current).toBe(area);
  expect(input).toHaveBeenCalledOnce();
  expect(area.style.height).toBe("52px");
  Object.defineProperty(area, "scrollHeight", { configurable: true, value: 32 });
  view.rerender(<TextArea aria-label="지시" ref={ref} value="짧게" onChange={() => {}} maxRows={2} />);
  expect(area.style.height).toBe("32px");
});

test("menu keyboard skips disabled actions and restores trigger focus", async () => {
  const user = userEvent.setup();
  const action = vi.fn();
  render(
    <Menu
      trigger={<button type="button">더 보기</button>}
      items={[
        { label: "삭제", disabled: true, onSelect: action },
        { label: "열기", onSelect: action },
      ]}
    />,
  );
  await user.tab();
  await user.keyboard("{Enter}");
  expect(await screen.findByRole("menu")).toBeInTheDocument();
  await user.keyboard("{Home}{Enter}");
  expect(action).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "더 보기" })).toHaveFocus();
});

test("select supports keyboard selection and disabled options", async () => {
  const user = userEvent.setup();
  const change = vi.fn();
  render(
    <Select
      label="출처"
      defaultValue="a"
      onValueChange={change}
      options={[
        { value: "a", label: "터미널" },
        { value: "b", label: "닫힘", disabled: true },
        { value: "c", label: "로그" },
      ]}
    />,
  );
  await user.tab();
  await user.keyboard("{Enter}");
  expect(await screen.findByRole("listbox")).toBeInTheDocument();
  await user.keyboard("{End}{Enter}");
  expect(change).toHaveBeenCalledWith("c");
});

test("split accepts custom minimum ratios below the default", () => {
  const change = vi.fn();
  render(
    <SplitPane dir="row" ratio={0.08} min={0.05} onRatio={change}>
      {[<div key="a" />, <div key="b" />]}
    </SplitPane>,
  );
  fireEvent.keyDown(screen.getByRole("separator"), { key: "ArrowLeft" });
  expect(change).toHaveBeenLastCalledWith(0.05);
});
