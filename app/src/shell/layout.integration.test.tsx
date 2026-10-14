import { act, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { TooltipProvider } from "../ui";
import { BottomPanel, registerBottomTab, useBottomTabs } from "./BottomPanel";
import { useLayout } from "./layout";
import { SectionLayout } from "./SectionLayout";
import { useAutoCollapse } from "./useAutoCollapse";

beforeEach(() => {
  document.documentElement.style.zoom = "";
  useLayout.setState({ listWidth: 260, listCollapsed: false, bottomOpen: false, bottomHeight: 260 });
  useBottomTabs.setState({ tabs: [] });
});
afterEach(() => vi.unstubAllGlobals());
function pointer(target: Element | Window, type: string, x: number, y: number) {
  const event = new MouseEvent(type, { bubbles: true, button: 0, clientX: x, clientY: y });
  Object.defineProperty(event, "pointerId", { value: 1 });
  fireEvent(target, event);
}
test("list resizing clamps, cleans cancelled drag and retains list state through collapse", () => {
  function List() {
    const [value, setValue] = useState(0);
    return (
      <button type="button" onClick={() => setValue(value + 1)}>
        목록 {value}
      </button>
    );
  }
  const view = render(
    <SectionLayout list={<List />}>
      <p>본문</p>
    </SectionLayout>,
  );
  fireEvent.click(screen.getByRole("button", { name: "목록 0" }));
  const edge = screen.getByRole("separator", { name: "목록 너비" });
  pointer(edge, "pointerdown", 100, 0);
  pointer(window, "pointermove", 200, 0);
  pointer(window, "pointermove", 250, 0);
  expect(useLayout.getState().listWidth).toBe(410);
  pointer(window, "pointercancel", 250, 0);
  pointer(window, "pointermove", 300, 0);
  expect(useLayout.getState().listWidth).toBe(410);
  expect(fireEvent.keyDown(edge, { key: "End" })).toBe(false);
  expect(useLayout.getState().listWidth).toBe(480);
  act(() => useLayout.getState().toggleList());
  expect(screen.queryByRole("button", { name: "목록 1" })).toBeNull();
  act(() => useLayout.getState().toggleList());
  expect(screen.getByRole("button", { name: "목록 1" })).toBeInTheDocument();
  document.documentElement.style.zoom = "1.5";
  pointer(edge, "pointerdown", 100, 0);
  pointer(window, "pointermove", -50, 0);
  expect(useLayout.getState().listWidth).toBe(380);
  view.unmount();
  pointer(window, "pointermove", 0, 0);
  expect(useLayout.getState().listWidth).toBe(380);
});
test("bottom tab replacements survive older disposers and selection falls back on removal", () => {
  const first = registerBottomTab({ id: "terminal", label: "터미널", render: () => <p>이전</p> });
  const newer = registerBottomTab({ id: "terminal", label: "터미널", render: () => <p>현재</p> });
  first();
  const extra = registerBottomTab({ id: "extra", label: "검토", render: () => <p>검토 본문</p> });
  useLayout.setState({ bottomOpen: true });
  render(
    <TooltipProvider>
      <BottomPanel />
    </TooltipProvider>,
  );
  expect(screen.getByText("현재")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "검토" }));
  expect(screen.getByText("검토 본문")).toBeInTheDocument();
  act(extra);
  expect(screen.getByText("현재")).toBeInTheDocument();
  const edge = screen.getByRole("separator", { name: "하단 패널 높이" });
  pointer(edge, "pointerdown", 0, 300);
  pointer(window, "pointermove", 0, 100);
  expect(useLayout.getState().bottomHeight).toBe(460);
  pointer(window, "pointerup", 0, 100);
  expect(fireEvent.keyDown(edge, { key: "ArrowDown" })).toBe(false);
  expect(useLayout.getState().bottomHeight).toBe(440);
  act(newer);
  expect(screen.queryByRole("region", { name: "하단 패널" })).toBeNull();
});
test("auto collapse only applies initially or on breakpoint entry and observes the outer frame", () => {
  vi.stubGlobal("innerWidth", 1280);
  vi.stubGlobal("innerHeight", 800);
  let notify: ResizeObserverCallback = () => {};
  const observe = vi.fn(),
    disconnect = vi.fn();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        notify = callback;
      }
      observe = observe;
      disconnect = disconnect;
    },
  );
  function Frame() {
    const ref = useRef<HTMLDivElement>(null);
    useAutoCollapse(ref);
    return <div ref={ref} data-testid="frame" />;
  }
  useLayout.setState({ bottomOpen: true });
  const view = render(<Frame />);
  expect(observe).toHaveBeenCalledWith(screen.getByTestId("frame"));
  const size = (width: number, height: number) =>
    act(() => notify([{ contentRect: { width, height } } as ResizeObserverEntry], {} as ResizeObserver));
  size(1000, 700);
  expect(useLayout.getState()).toMatchObject({ listCollapsed: true, bottomOpen: false });
  act(() => {
    useLayout.getState().toggleList();
    useLayout.getState().toggleBottom();
  });
  fireEvent(window, new Event("resize"));
  size(1000, 710);
  expect(useLayout.getState()).toMatchObject({ listCollapsed: false, bottomOpen: true });
  size(1200, 800);
  size(1000, 700);
  expect(useLayout.getState()).toMatchObject({ listCollapsed: true, bottomOpen: false });
  view.unmount();
  expect(disconnect).toHaveBeenCalledOnce();
  vi.stubGlobal("innerWidth", 900);
  vi.stubGlobal("innerHeight", 700);
  useLayout.setState({ listCollapsed: false, bottomOpen: true });
  render(<Frame />);
  expect(useLayout.getState()).toMatchObject({ listCollapsed: true, bottomOpen: false });
});

test("bottom resize starts at the displayed height after a viewport clamp", () => {
  document.documentElement.style.zoom = "1.5";
  const unregister = registerBottomTab({ id: "terminal", label: "터미널", render: () => <p>현재</p> });
  useLayout.setState({ bottomOpen: true, bottomHeight: 600 });
  const view = render(
    <TooltipProvider>
      <BottomPanel />
    </TooltipProvider>,
  );
  const panel = screen.getByRole("region", { name: "하단 패널" });
  // In a 1280x800 viewport at 150%, max-height constrains the 600px preference to 353.33 CSS px.
  const displayed = vi.spyOn(panel, "getBoundingClientRect").mockReturnValue({
    x: 72,
    y: 234,
    top: 234,
    left: 72,
    right: 1280,
    bottom: 764,
    width: 1208,
    height: 530,
    toJSON: () => ({}),
  });
  const edge = screen.getByRole("separator", { name: "하단 패널 높이" });
  expect(useLayout.getState().bottomHeight).toBe(600);
  expect(fireEvent.keyDown(edge, { key: "ArrowDown" })).toBe(false);
  expect(useLayout.getState().bottomHeight).toBeCloseTo(530 / 1.5 - 20);
  act(() => useLayout.setState({ bottomHeight: 600 }));
  pointer(edge, "pointerdown", 100, 234);
  pointer(window, "pointermove", 100, 264);
  expect(useLayout.getState().bottomHeight).toBeCloseTo((530 - 30) / 1.5);
  const originalRect = panel.getBoundingClientRect();
  displayed.mockReturnValue({ ...originalRect, height: 500 });
  pointer(window, "pointermove", 100, 294);
  pointer(window, "pointerup", 100, 294);
  const height = useLayout.getState().bottomHeight;
  view.unmount();
  unregister();
  expect(height).toBeCloseTo((530 - 60) / 1.5);
});
