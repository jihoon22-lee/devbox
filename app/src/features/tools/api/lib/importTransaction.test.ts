// v0.9.0 importTransaction.test.ts 2개 적응: localStorage 문서 대신 컬렉션 파일(digest CAS)로,
// 중간 실패 때 이번에 만든 파일만 그 digest일 때 지운다.
import { describe, expect, it } from "vitest";
import { FakeBackend } from "../test-backend";
import { rollbackImport, writeImport } from "./importTransaction";

const root = { kind: "personal" } as const;

describe("import transaction", () => {
  it("rollback removes only files that still have the written digest", async () => {
    const backend = new FakeBackend();
    const result = await writeImport(backend, root, [
      { path: "a.request.json", text: "a" },
      { path: "b.request.json", text: "b" },
    ]);
    expect(result.failed).toBeNull();
    expect(result.written.map((w) => w.path)).toEqual(["a.request.json", "b.request.json"]);
    // 가져온 뒤 b를 밖에서 고쳤다.
    backend.seed(root, "b.request.json", "edited");
    const undo = await rollbackImport(backend, root, result.written);
    expect(undo).toEqual({ removed: ["a.request.json"], kept: ["b.request.json"] });
    expect(backend.text(root, "b.request.json")).toBe("edited");
    expect(backend.text(root, "a.request.json")).toBeUndefined();
  });

  it("a later write failure stops and reports what was created so far", async () => {
    const backend = new FakeBackend();
    backend.seed(root, "existing.request.json", "keep");
    backend.failWrite.add("c.request.json");
    const result = await writeImport(backend, root, [
      { path: "a.request.json", text: "a" },
      { path: "c.request.json", text: "c" },
      { path: "d.request.json", text: "d" },
    ]);
    expect(result.failed).toMatchObject({ path: "c.request.json" });
    expect(result.written.map((w) => w.path)).toEqual(["a.request.json"]);
    expect(backend.write).toHaveBeenCalledTimes(2);
    await rollbackImport(backend, root, result.written);
    expect(backend.text(root, "a.request.json")).toBeUndefined();
    expect(backend.text(root, "existing.request.json")).toBe("keep");
    // 이미 있는 경로는 덮지 않는다(expect absent).
    const clash = await writeImport(backend, root, [{ path: "existing.request.json", text: "x" }]);
    expect(clash.failed).toMatchObject({ path: "existing.request.json" });
    expect(backend.text(root, "existing.request.json")).toBe("keep");
  });
});
