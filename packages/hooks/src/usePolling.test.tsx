import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePolling } from "./usePolling";

function hidden(value: boolean) {
  Object.defineProperty(document, "hidden", { configurable: true, value });
  document.dispatchEvent(new Event("visibilitychange"));
}
beforeEach(() => {
  vi.useFakeTimers();
  hidden(false);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("usePolling", () => {
  it("runs immediately and preserves the configured interval", async () => {
    const callback = vi.fn();
    renderHook(() => usePolling(callback, { intervalMs: 1000 }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(callback).toHaveBeenCalledTimes(4);
  });

  it("does not overlap slow calls, including manual refresh", async () => {
    let finish!: () => void;
    const callback = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const { result } = renderHook(() => usePolling(callback, { intervalMs: 1000 }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
      result.current.refresh();
      result.current.refresh();
    });
    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => finish());
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("pauses hidden polling and immediately refreshes on return", async () => {
    const callback = vi.fn();
    renderHook(() => usePolling(callback, { intervalMs: 1000 }));
    await act(async () => {});
    act(() => hidden(true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => hidden(false));
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("keeps in-flight exclusion through inactive/active transitions", async () => {
    let finish!: () => void;
    const callback = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const { rerender } = renderHook(({ active }) => usePolling(callback, { intervalMs: 1000, active }), {
      initialProps: { active: true },
    });
    rerender({ active: false });
    rerender({ active: true });
    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => finish());
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("supports inactive, delayed first calls and deliberate hidden polling", async () => {
    const callback = vi.fn();
    const { rerender } = renderHook(
      ({ active }) => usePolling(callback, { intervalMs: 1000, active, immediate: false, pauseWhenHidden: false }),
      { initialProps: { active: false } },
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(callback).not.toHaveBeenCalled();
    act(() => hidden(true));
    rerender({ active: true });
    expect(callback).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it("uses the latest callback and continues after failures", async () => {
    const first = vi.fn(async (): Promise<void> => {
      throw new Error("reported by owner");
    });
    const second = vi.fn(async () => {});
    const { rerender, unmount } = renderHook(({ callback }) => usePolling(callback, { intervalMs: 1000 }), {
      initialProps: { callback: first },
    });
    await act(async () => {});
    rerender({ callback: second });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
    unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("does not overlap or reschedule a retired StrictMode effect", async () => {
    let finish!: () => void;
    const callback = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const { unmount } = renderHook(() => usePolling(callback, { intervalMs: 1000 }), { wrapper: StrictMode });
    expect(callback).toHaveBeenCalledTimes(1);
    unmount();
    await act(async () => {
      finish();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(callback).toHaveBeenCalledTimes(1);
  });
});
