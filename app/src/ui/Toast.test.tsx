import { act, fireEvent, render, screen } from "@testing-library/react";
import { ToastHost, useToasts } from "./Toast";

beforeEach(() => {
  vi.useFakeTimers();
  useToasts.getState().items.forEach((t) => useToasts.getState().dismiss(t.id));
});
afterEach(() => {
  useToasts.getState().items.forEach((t) => useToasts.getState().dismiss(t.id));
  vi.useRealTimers();
});
test("expiry is five seconds or ten seconds with undo", () => {
  render(<ToastHost />);
  const undo = vi.fn();
  act(() => {
    useToasts.getState().push({ tone: "info", title: "알림" });
    useToasts.getState().push({ tone: "ok", title: "완료", undo });
  });
  act(() => vi.advanceTimersByTime(5000));
  expect(screen.queryByText("알림")).toBeNull();
  expect(screen.getByText("완료")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(5000));
  expect(screen.queryByText("완료")).toBeNull();
  expect(undo).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});
test("undo removes atomically before callback and rejects stale activation", () => {
  render(<ToastHost />);
  let id = 0;
  const undo = vi.fn(() => {
    expect(useToasts.getState().items).toHaveLength(0);
    useToasts.getState().undo(id);
  });
  act(() => {
    id = useToasts.getState().push({ tone: "info", title: "중지했습니다", undo });
  });
  const button = screen.getByRole("button", { name: "되돌리기" });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(undo).toHaveBeenCalledOnce();
  expect(screen.queryByText("중지했습니다")).toBeNull();
  expect(vi.getTimerCount()).toBe(0);
});
test("latest undo skips ordinary toast; eviction and dismiss clear timers", () => {
  const first = vi.fn(),
    second = vi.fn();
  const store = useToasts.getState();
  store.push({ tone: "info", title: "하나", undo: first });
  store.push({ tone: "info", title: "둘", undo: second });
  store.push({ tone: "info", title: "알림" });
  store.undoLatest();
  expect(second).toHaveBeenCalledOnce();
  expect(first).not.toHaveBeenCalled();
  for (let i = 0; i < 6; i++) store.push({ tone: "info", title: String(i) });
  expect(useToasts.getState().items).toHaveLength(5);
  expect(vi.getTimerCount()).toBe(5);
  store.undoLatest();
  expect(first).not.toHaveBeenCalled();
  useToasts.getState().items.forEach((t) => store.dismiss(t.id));
  expect(vi.getTimerCount()).toBe(0);
});
