import { fireEvent, render, screen } from "@testing-library/react";
import { VirtualList } from "./VirtualList";

test("renders a bounded window of a large list and reports the scroll boundary", () => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(240);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(800);
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: vi.fn() });
  const boundary = vi.fn();
  render(
    <VirtualList
      items={Array.from({ length: 10_000 }, (_, i) => i)}
      onAtBottomChange={boundary}
      renderItem={(value) => <span data-testid="row">{value}</span>}
    />,
  );
  expect(screen.getAllByTestId("row").length).toBeLessThan(40);
  const list = screen.getByRole("log");
  Object.defineProperties(list, {
    scrollHeight: { configurable: true, value: 240_000 },
    clientHeight: { configurable: true, value: 240 },
    scrollTop: { configurable: true, writable: true, value: 100 },
  });
  fireEvent.scroll(list);
  expect(boundary).toHaveBeenLastCalledWith(false);
  list.scrollTop = 240_000 - 240;
  fireEvent.scroll(list);
  expect(boundary).toHaveBeenLastCalledWith(true);
});
