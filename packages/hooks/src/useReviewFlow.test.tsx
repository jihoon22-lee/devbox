import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useReviewFlow } from "./useReviewFlow";

afterEach(cleanup);

describe("useReviewFlow", () => {
  it("previews, applies once and does not discard an applied preview", async () => {
    let finish!: (value: string) => void;
    const apply = vi.fn(
      () =>
        new Promise<string>((resolve) => {
          finish = resolve;
        }),
    );
    const discard = vi.fn(async () => {});
    const { result, unmount } = renderHook(() => useReviewFlow({ preview: async () => ({ id: "p" }), apply, discard }));
    await act(async () => result.current.start());
    expect(result.current.state).toBe("reviewed");
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.confirm();
      void result.current.confirm();
    });
    expect(apply).toHaveBeenCalledTimes(1);
    await act(async () => result.current.reset());
    expect(discard).not.toHaveBeenCalled();
    expect(result.current.state).toBe("applying");
    await act(async () => {
      finish("saved");
      await pending;
    });
    expect(result.current.result).toBe("saved");
    expect(result.current.state).toBe("done");
    unmount();
    expect(discard).not.toHaveBeenCalled();
  });

  it("discards an unapplied preview once on reset or unmount, including zero", async () => {
    const discard = vi.fn(async () => {});
    const { result, unmount } = renderHook(() =>
      useReviewFlow({ preview: async () => 0, apply: async (value: number) => value, discard }),
    );
    await act(async () => result.current.start());
    await act(async () => result.current.reset());
    expect(discard).toHaveBeenCalledWith(0);
    unmount();
    expect(discard).toHaveBeenCalledTimes(1);
  });

  it("aborts and disposes a preview that resolves after unmount", async () => {
    let finish!: (value: string) => void;
    let signal!: AbortSignal;
    const discard = vi.fn(async () => {});
    const preview = vi.fn((value: AbortSignal) => {
      signal = value;
      return new Promise<string>((resolve) => {
        finish = resolve;
      });
    });
    const { result, unmount } = renderHook(() =>
      useReviewFlow({ preview, apply: async (value: string) => value, discard }),
    );
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.start();
      void result.current.start();
    });
    expect(preview).toHaveBeenCalledTimes(1);
    unmount();
    expect(signal.aborted).toBe(true);
    finish("late");
    await pending;
    expect(discard).toHaveBeenCalledExactlyOnceWith("late");
  });

  it("does not let a reset preview overwrite a newer preview", async () => {
    let finish!: (value: string) => void;
    const preview = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            finish = resolve;
          }),
      )
      .mockResolvedValue("new");
    const discard = vi.fn(async () => {});
    const { result } = renderHook(() =>
      useReviewFlow<string, string>({ preview, apply: async (value) => value, discard }),
    );
    let old!: Promise<void>;
    act(() => {
      old = result.current.start();
    });
    await act(async () => result.current.reset());
    await act(async () => result.current.start());
    await act(async () => {
      finish("old");
      await old;
    });
    expect(result.current.preview).toBe("new");
    expect(result.current.state).toBe("reviewed");
    expect(discard).toHaveBeenCalledWith("old");
  });

  it("retains failed apply for retry and discards it on unmount", async () => {
    const discard = vi.fn(async () => {});
    const apply = vi.fn(async () => {
      throw new Error("충돌");
    });
    const { result, unmount } = renderHook(() => useReviewFlow({ preview: async () => 1, apply, discard }), {
      wrapper: StrictMode,
    });
    await act(async () => result.current.start());
    await act(async () => result.current.confirm());
    expect(result.current.state).toBe("failed");
    expect(result.current.issue).toBe("충돌");
    expect(result.current.preview).toBe(1);
    unmount();
    expect(discard).toHaveBeenCalledExactlyOnceWith(1);
  });

  it("uses the preview owner's cleanup and contains cleanup rejection", async () => {
    const original = vi.fn(async (): Promise<void> => {
      throw new Error("cleanup");
    });
    const replacement = vi.fn(async () => {});
    const { result, rerender, unmount } = renderHook(
      ({ discard }) => useReviewFlow({ preview: async () => "p", apply: async (p: string) => p, discard }),
      { initialProps: { discard: original } },
    );
    await act(async () => result.current.start());
    rerender({ discard: replacement });
    unmount();
    await act(async () => {});
    expect(original).toHaveBeenCalledWith("p");
    expect(replacement).not.toHaveBeenCalled();
  });
  it("retains the preview when explicit discard fails so cancellation can be retried", async () => {
    const discard = vi.fn().mockRejectedValueOnce(new Error("temporary")).mockResolvedValue(undefined);
    const { result } = renderHook(() =>
      useReviewFlow({ preview: async () => "p", apply: async (p: string) => p, discard }),
    );
    await act(async () => result.current.start());
    await act(async () => result.current.reset());
    expect(result.current.state).toBe("failed");
    expect(result.current.preview).toBe("p");
    expect(result.current.issue).toBe("temporary");
    await act(async () => result.current.reset());
    expect(result.current.state).toBe("idle");
    expect(discard).toHaveBeenCalledTimes(2);
  });
  it("retains the old preview when cleanup for its replacement fails", async () => {
    let reject!: (error: Error) => void;
    const discard = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<void>((_, fail) => {
            reject = fail;
          }),
      )
      .mockResolvedValue(undefined);
    const preview = vi.fn().mockResolvedValueOnce("old").mockResolvedValue("new");
    const { result } = renderHook(() =>
      useReviewFlow<string, string>({ preview, apply: async (value) => value, discard }),
    );
    await act(async () => result.current.start());
    let replacement!: Promise<void>;
    act(() => {
      replacement = result.current.start();
    });
    await act(async () => result.current.reset());
    await act(async () => {
      reject(new Error("cleanup failed"));
      await replacement;
    });
    expect(result.current.preview).toBe("old");
    expect(result.current.state).toBe("failed");
    expect(preview).toHaveBeenCalledTimes(1);
    await act(async () => result.current.start());
    expect(result.current.preview).toBe("new");
    expect(discard.mock.calls).toEqual([["old"], ["old"]]);
  });
});
