import { describe, expect, it, vi } from "vitest";
import { MetadataRefresh } from "./metadataRefresh";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const turn = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

describe("metadata refresh ownership", () => {
  it.each([false, true])("coalesces a burst and suppresses stale completion (failure=%s)", async (failure) => {
    const first = deferred<string>();
    const last = deferred<string>();
    const read = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(last.promise);
    const apply = vi.fn();
    const fail = vi.fn();
    const refresh = new MetadataRefresh(read, apply, fail);
    refresh.start();
    const done = refresh.request();
    await turn();
    for (let i = 0; i < 100; i++) void refresh.request();
    expect(read).toHaveBeenCalledTimes(1);
    if (failure) first.reject(new Error("old failure"));
    else first.resolve("old tree and tags");
    await turn();
    expect(read).toHaveBeenCalledTimes(2);
    expect(apply).not.toHaveBeenCalled();
    expect(fail).not.toHaveBeenCalled();
    last.resolve("latest tree and tags");
    await done;
    expect(apply).toHaveBeenCalledExactlyOnceWith("latest tree and tags");
    expect(fail).not.toHaveBeenCalled();
  });

  it("discards an unmounted result and resumes exactly one trailing read after StrictMode restart", async () => {
    const old = deferred<string>();
    const next = deferred<string>();
    const read = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
    const apply = vi.fn();
    const fail = vi.fn();
    const refresh = new MetadataRefresh(read, apply, fail);
    refresh.start();
    const done = refresh.request();
    await turn();
    refresh.stop();
    refresh.start();
    void refresh.request();
    old.resolve("old mount");
    await turn();
    expect(apply).not.toHaveBeenCalled();
    expect(read).toHaveBeenCalledTimes(2);
    refresh.stop();
    next.reject(new Error("unmounted"));
    await done;
    expect(apply).not.toHaveBeenCalled();
    expect(fail).not.toHaveBeenCalled();
  });

  it("retains the last good value on a latest failure and does not strand reentrant refreshes", async () => {
    const values: string[] = [];
    const read = vi
      .fn()
      .mockResolvedValueOnce("first")
      .mockRejectedValueOnce(new Error("incomplete"))
      .mockResolvedValueOnce("recovered");
    const fail = vi.fn();
    const refresh = new MetadataRefresh<string>(
      read,
      (value) => {
        values.push(value);
        if (value === "first") void refresh.request();
      },
      fail,
    );
    refresh.start();
    await refresh.request();
    expect(values).toEqual(["first"]);
    expect(fail).toHaveBeenCalledTimes(1);
    await refresh.request();
    expect(values).toEqual(["first", "recovered"]);
    expect(read).toHaveBeenCalledTimes(3);
  });
});
