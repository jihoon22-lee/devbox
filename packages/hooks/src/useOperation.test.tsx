import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { messageOf, useOperation } from "./useOperation";

afterEach(cleanup);

describe("useOperation", () => {
  it("aborts an older operation and keeps only the latest result", async () => {
    const { result } = renderHook(() => useOperation());
    let finish!: (value: string) => void;
    let oldSignal!: AbortSignal;
    let first!: Promise<string | undefined>;
    act(() => {
      first = result.current.run((signal) => {
        oldSignal = signal;
        return new Promise<string>((resolve) => {
          finish = resolve;
        });
      });
    });
    expect(result.current.busy).toBe(true);
    await act(async () => expect(await result.current.run(async () => "fresh")).toBe("fresh"));
    expect(oldSignal.aborted).toBe(true);
    await act(async () => finish("stale"));
    await expect(first).resolves.toBeUndefined();
    expect(result.current.busy).toBe(false);
  });

  it("shares an in-flight action when the same callback is submitted twice", async () => {
    let finish!: (value: number) => void;
    const task = vi.fn(
      () =>
        new Promise<number>((resolve) => {
          finish = resolve;
        }),
    );
    const { result } = renderHook(() => useOperation());
    let first!: Promise<number | undefined>;
    let second!: Promise<number | undefined>;
    act(() => {
      first = result.current.run(task);
      second = result.current.run(task);
    });
    expect(task).toHaveBeenCalledTimes(1);
    await act(async () => finish(7));
    await expect(first).resolves.toBe(7);
    await expect(second).resolves.toBe(7);
  });

  it("reports errors and clears them explicitly or on a new operation", async () => {
    const { result } = renderHook(() => useOperation());
    await act(async () => {
      await result.current.run(async () => {
        throw new Error("저장 실패");
      });
    });
    expect(result.current.issue).toBe("저장 실패");
    act(() => result.current.clearIssue());
    expect(result.current.issue).toBeNull();
    await act(async () => {
      await result.current.run(async () => {
        throw "not a public error";
      });
    });
    expect(result.current.issue).toBe(messageOf(null));
    await act(async () => {
      await result.current.run(async () => 1);
    });
    expect(result.current.issue).toBeNull();
  });

  it("cancel prevents late completion or failure from changing state", async () => {
    const { result } = renderHook(() => useOperation());
    let fail!: (error: Error) => void;
    let pending!: Promise<unknown>;
    act(() => {
      pending = result.current.run(
        () =>
          new Promise((_, reject) => {
            fail = reject;
          }),
      );
    });
    act(() => result.current.cancel());
    expect(result.current.busy).toBe(false);
    await act(async () => fail(new Error("late")));
    await expect(pending).resolves.toBeUndefined();
    expect(result.current.issue).toBeNull();
  });

  it("works after StrictMode replay and aborts work on unmount", async () => {
    const { result, unmount } = renderHook(() => useOperation(), { wrapper: StrictMode });
    await act(async () => expect(await result.current.run(async () => 2)).toBe(2));
    let finish!: (value: number) => void;
    let signal!: AbortSignal;
    let pending!: Promise<number | undefined>;
    act(() => {
      pending = result.current.run((value) => {
        signal = value;
        return new Promise<number>((resolve) => {
          finish = resolve;
        });
      });
    });
    unmount();
    expect(signal.aborted).toBe(true);
    finish(3);
    await expect(pending).resolves.toBeUndefined();
  });
});
