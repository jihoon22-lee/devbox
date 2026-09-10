import { describe, expect, it, vi } from "vitest";
import { reconnectWsl } from "./reconnectWsl";
import type { Doc, OpenedFile } from "./types";

function fixture() {
  let docs: Doc[] = [{
    id: "a", path: "/project/a", text: "unsaved", dirty: true, revision: 1,
    nativeRevision: "old", encoding: { encodingKind: "utf8", bom: false },
    lineEnding: "lf", readOnly: false, size: 4, mtimeNanos: "1", contentHash: "hash",
    lossy: false, cursor: 3, bookmarks: [1],
  }];
  const opened: OpenedFile = { ...docs[0], text: "disk", nativeRevision: "new", mtimeNanos: "2" };
  const editor = {
    current: () => docs, active: vi.fn(() => true),
    replace: vi.fn((doc: Doc) => { docs = docs.map(old => old.id === doc.id ? doc : old); }),
    conflict: vi.fn(),
  };
  const connection = { reconnect: vi.fn(async () => {}), open: vi.fn(async () => opened), watch: vi.fn(async () => {}) };
  return { editor, connection, opened, change: (change: Partial<Doc>) => { docs = [{ ...docs[0], ...change }]; } };
}
describe("WSL file reconnection", () => {
  it("refreshes unchanged native snapshots while preserving edits, selection and bookmarks", async () => {
    const f = fixture();
    f.connection.open.mockImplementation(async () => { f.change({ text: "newer edit", revision: 2 }); return f.opened; });
    await reconnectWsl(f.editor, f.connection);
    expect(f.editor.current()[0]).toMatchObject({text: "newer edit", dirty: true, revision: 2, nativeRevision: "new", mtimeNanos: "2", cursor: 3, bookmarks: [1]});
    expect(f.editor.conflict).not.toHaveBeenCalled();
    expect(f.connection.watch).toHaveBeenCalledWith("/project/a");
  });
  it("keeps dirty buffers and old save tokens when disk changed", async () => {
    const f = fixture(); f.opened.contentHash = "changed";
    await reconnectWsl(f.editor, f.connection);
    expect(f.editor.current()[0]).toMatchObject({text: "unsaved", nativeRevision: "old", dirty: true});
    expect(f.editor.conflict).toHaveBeenCalledWith("/project/a");
  });
  it("reloads a clean buffer but preserves edits made during the read", async () => {
    const f = fixture(); f.change({ dirty: false }); f.opened.contentHash = "changed";
    await reconnectWsl(f.editor, f.connection);
    expect(f.editor.current()[0]).toMatchObject({text: "disk", nativeRevision: "new", dirty: false});
    const racing = fixture(); racing.change({ dirty: false }); racing.opened.contentHash = "changed";
    racing.connection.open.mockImplementation(async () => { racing.change({text: "typed", revision: 2, dirty: true}); return racing.opened; });
    await reconnectWsl(racing.editor, racing.connection);
    expect(racing.editor.current()[0].text).toBe("typed");
    expect(racing.editor.conflict).toHaveBeenCalled();
  });
  it("preserves unavailable files and stops publication after context changes", async () => {
    const f = fixture(); f.connection.open.mockRejectedValue(new Error("missing"));
    await expect(reconnectWsl(f.editor, f.connection)).rejects.toThrow("일부 WSL 파일");
    expect(f.editor.current()[0].text).toBe("unsaved");
    expect(f.connection.watch).not.toHaveBeenCalled();
    const racing = fixture();
    racing.connection.open.mockImplementation(async () => { racing.editor.active.mockReturnValue(false); return racing.opened; });
    await reconnectWsl(racing.editor, racing.connection);
    expect(racing.editor.replace).not.toHaveBeenCalled();
    expect(racing.connection.watch).not.toHaveBeenCalled();
  });
  it("does not read documents when retirement or native admission fails", async () => {
    const f = fixture(); f.connection.reconnect.mockRejectedValue(new Error("retirement unconfirmed"));
    await expect(reconnectWsl(f.editor, f.connection)).rejects.toThrow("retirement unconfirmed");
    expect(f.connection.open).not.toHaveBeenCalled();
    expect(f.editor.replace).not.toHaveBeenCalled();
  });
});
