import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { UndoProvider, useUndo } from "./undo";

function Harness({ undo }: { undo: () => Promise<void> }) {
  const { offer, toast } = useUndo();
  return (
    <>
      <button onClick={() => offer("노트를 만들었습니다.", undo)}>만들기</button>
      {toast}
    </>
  );
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useUndo", () => {
  it("offers undo for eight seconds", async () => {
    vi.useRealTimers(); // axe schedules its own timers
    const undo = vi.fn(async () => {});
    const { container } = render(<Harness undo={undo} />);
    fireEvent.click(screen.getByText("만들기"));
    expect(screen.getByRole("status").textContent).toContain("노트를 만들었습니다.");
    await assertNoA11yViolations(container);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
    });
    expect(undo).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  });

  it("expires and a newer action replaces the older offer", async () => {
    const first = vi.fn(async () => {});
    const { rerender } = render(<Harness undo={first} />);
    fireEvent.click(screen.getByText("만들기"));
    const second = vi.fn(async () => {});
    rerender(<Harness undo={second} />);
    fireEvent.click(screen.getByText("만들기"));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
    });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("만들기"));
    act(() => {
      vi.advanceTimersByTime(8_000);
    });
    expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  });
});

it("restores focus when the focused toast expires and keeps a pending undo from clearing a newer offer", async () => {
  let finish!: () => void;
  const first = () =>
    new Promise<void>((resolve) => {
      finish = resolve;
    });
  const { rerender } = render(<Harness undo={first} />);
  const opener = screen.getByText("만들기");
  opener.focus();
  fireEvent.click(opener);
  screen.getByRole("button", { name: "되돌리기" }).focus();
  act(() => {
    vi.advanceTimersByTime(8000);
  });
  expect(document.activeElement).toBe(opener);
  fireEvent.click(opener);
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  const second = vi.fn(async () => {});
  rerender(<Harness undo={second} />);
  fireEvent.click(opener);
  await act(async () => {
    finish();
  });
  expect(screen.getByRole("button", { name: "되돌리기" })).toBeTruthy();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  });
  expect(second).toHaveBeenCalledOnce();
});
it("shares one offer across feature hooks and reports one failed undo without a retry button", async () => {
  const undo = vi.fn(async () => {
    throw new Error("이미 수정되어 되돌리지 않았습니다.");
  });
  render(
    <UndoProvider>
      <Harness undo={undo} />
      <Harness undo={async () => {}} />
    </UndoProvider>,
  );
  fireEvent.click(screen.getAllByText("만들기")[1]);
  fireEvent.click(screen.getAllByText("만들기")[0]);
  expect(screen.getAllByRole("button", { name: "되돌리기" })).toHaveLength(1);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  });
  expect(undo).toHaveBeenCalledOnce();
  expect(screen.getByRole("status").textContent).toContain("이미 수정되어");
  expect(screen.queryByRole("button", { name: "되돌리기" })).toBeNull();
  act(() => {
    vi.advanceTimersByTime(3000);
  });
  expect(screen.queryByRole("status")).toBeNull();
});
