import { describe, expect, it, vi } from "vitest";
import { NativeEditorMirror } from "./nativeEditorMirror";

const document = { id: "text", path: "/work/notes.txt", nativeRevision: "native-1", text: "draft" };
function deferred() {
  let resolve!: (value: boolean) => void;
  const promise = new Promise<boolean>(done => { resolve = done; });
  return { promise, resolve };
}

describe("native editor buffer mirror", () => {
  it("includes unsupported and cross-language files, while standalone documents make no calls", async () => {
    const send = vi.fn().mockResolvedValue(true);
    const mirror = new NativeEditorMirror(send);
    await mirror.flush([document, { ...document, id: "python", path: "/work/a.py" },
      { ...document, id: "legacy", nativeRevision: undefined }]);
    expect(send.mock.calls).toEqual([[document.path, "native-1", "draft"], ["/work/a.py", "native-1", "draft"]]);
  });

  it("coalesces queued edits and waits for edits arriving during a flush", async () => {
    const first = deferred();
    const last = deferred();
    const send = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(last.promise);
    const mirror = new NativeEditorMirror(send);
    const flushing = mirror.flush([document]);
    await vi.waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    mirror.update([{ ...document, text: "intermediate" }]);
    mirror.update([{ ...document, text: "last" }]);
    first.resolve(true);
    await vi.waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    let complete = false;
    void flushing.then(() => { complete = true; });
    await Promise.resolve();
    expect(complete).toBe(false);
    last.resolve(true);
    await flushing;
    expect(send.mock.calls[1]).toEqual([document.path, "native-1", "last"]);
  });

  it("keeps a failed acknowledgement blocking apply until a new revision succeeds", async () => {
    const send = vi.fn().mockRejectedValueOnce(new Error("stale document")).mockResolvedValue(false);
    const mirror = new NativeEditorMirror(send);
    mirror.update([document]);
    await expect(mirror.flush([document])).rejects.toThrow("stale document");
    await expect(mirror.flush([{ ...document, nativeRevision: "native-2" }])).resolves.toBeUndefined();
  });

  it("discards queued work after close or a context change and rejects an old flush", async () => {
    const first = deferred();
    const send = vi.fn().mockReturnValue(first.promise);
    const mirror = new NativeEditorMirror(send);
    mirror.setContext("old");
    const flushing = mirror.flush([document]);
    const rejected = expect(flushing).rejects.toThrow("프로젝트가 변경");
    await vi.waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    mirror.update([{ ...document, text: "queued" }]);
    mirror.update([]);
    mirror.setContext("new");
    first.resolve(true);
    await rejected;
    expect(send).toHaveBeenCalledTimes(1);
  });
});
