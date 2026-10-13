import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AgentGrid } from "./AgentGrid";
import { task } from "./test-fixtures";

const terminal = vi.fn();
vi.mock("../terminal/TerminalView", () => ({
  TerminalView: (props: unknown) => {
    terminal(props);
    return <div />;
  },
}));
beforeEach(() => {
  terminal.mockClear();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
test.each([
  [4, 2],
  [5, 3],
  [9, 3],
  [10, 4],
])("%i tasks use %i columns with read-only previews", (count, columns) => {
  const open = vi.fn();
  render(<AgentGrid tasks={Array.from({ length: count }, (_, i) => task(String(i)))} onOpen={open} />);
  expect(screen.getByRole("region", { name: "에이전트 터미널 격자" })).toHaveStyle({
    gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
  });
  expect(terminal).toHaveBeenCalledWith(expect.objectContaining({ readOnly: true, scale: expect.any(Number) }));
  fireEvent.click(screen.getByRole("button", { name: "작업 0 터미널 미리보기" }));
  expect(open).toHaveBeenCalledWith("0");
});
test("tiles retain title and state without a session and observe cell width", () => {
  const observers: ResizeObserverCallback[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        observers.push(callback);
      }
      observe() {}
      disconnect() {}
    },
  );
  render(
    <AgentGrid tasks={[{ ...task("a", "preparing"), terminalId: null }, task("b", "waiting")]} onOpen={() => {}} />,
  );
  expect(screen.getByRole("button", { name: "작업 a 터미널 미리보기" })).toHaveTextContent("준비 중");
  act(() => observers[1]!([{ contentRect: { width: 350 } } as ResizeObserverEntry], {} as ResizeObserver));
  expect(terminal).toHaveBeenLastCalledWith(expect.objectContaining({ id: "t", readOnly: true, scale: 0.35 }));
  vi.unstubAllGlobals();
});

test("closed work never appears in grid previews, column counts or the nonempty state", () => {
  const tasks = [task("a"), task("b"), task("c"), task("d"), task("closed", "closed")];
  const view = render(<AgentGrid tasks={tasks} onOpen={() => {}} />);
  expect(screen.queryByRole("button", { name: "작업 closed 터미널 미리보기" })).toBeNull();
  expect(screen.getByRole("region", { name: "에이전트 터미널 격자" })).toHaveStyle({
    gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
  });
  view.rerender(<AgentGrid tasks={[task("closed", "closed")]} onOpen={() => {}} />);
  expect(screen.queryByRole("region", { name: "에이전트 터미널 격자" })).toBeNull();
  expect(screen.getByText("에이전트 작업이 없습니다")).toBeInTheDocument();
});
