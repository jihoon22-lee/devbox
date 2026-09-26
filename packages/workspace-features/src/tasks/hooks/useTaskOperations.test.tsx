import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useTaskOperations } from "./useTaskOperations";

afterEach(cleanup);

it("admits one task mutation until its completion", async () => {
  let finish!: (value: number) => void;
  const task = vi.fn(
    () =>
      new Promise<number>((resolve) => {
        finish = resolve;
      }),
  );
  const { result } = renderHook(() => useTaskOperations());
  let first!: Promise<number | undefined>;
  let second!: Promise<number | undefined>;
  act(() => {
    first = result.current.run(task);
    second = result.current.run(task);
  });
  expect(result.current.busy).toBe(true);
  expect(task).toHaveBeenCalledTimes(1);
  await expect(second).resolves.toBeUndefined();
  await act(async () => finish(7));
  await expect(first).resolves.toBe(7);
  expect(result.current.busy).toBe(false);
});

it("returns save failures to the editor and releases the next mutation", async () => {
  const { result } = renderHook(() => useTaskOperations());
  const failure = new Error("synthetic save failure");
  await act(async () => {
    await expect(
      result.current.run(async () => {
        throw failure;
      }),
    ).rejects.toBe(failure);
  });
  expect(result.current.busy).toBe(false);
  await act(async () => expect(await result.current.run(async () => "saved")).toBe("saved"));
});
