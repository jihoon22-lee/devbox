import { beforeEach, expect, it, vi } from "vitest";
import { openFile, revealFile, searchSource, type SourceSnapshot } from "./api";
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../transport", () => ({ componentInvoke: () => mocks.invoke, isProductHosted: () => true }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));
beforeEach(() => mocks.invoke.mockReset());
it("revokes a completed generation when its consumer is later cancelled", async () => {
  const snapshot: SourceSnapshot = { generation: "native-query", storeGeneration: "store", source: "files", state: "complete", partial: false, rows: [{ source: "files", rootIdentity: "files:9", reference: "issued-reference", availability: "available", value: { id: 1, path: "C:/fixture/shared.md", name: "shared.md", ext: "md", size: 1, modified_ts: 1, snippet: "" } }] };
  mocks.invoke.mockImplementation(async method => method === "source_query" ? snapshot : undefined);
  const controller = new AbortController();
  const update = vi.fn();
  const rows = await searchSource("files", "shared", "name", 200, {}, controller.signal, update);
  expect(rows[0].reference).toBe("issued-reference");
  expect(update).toHaveBeenCalledWith(snapshot);
  controller.abort();
  expect(mocks.invoke).toHaveBeenLastCalledWith("source_cancel", { generation: "native-query" });
});
it("sends only an opaque reference to product openers", async () => {
  mocks.invoke.mockResolvedValue(undefined);
  await openFile("C:/renderer/substituted.exe", "issued-reference");
  expect(mocks.invoke).toHaveBeenLastCalledWith("open_file", { reference: "issued-reference" });
  await revealFile("C:/renderer/substituted.exe", "issued-reference");
  expect(mocks.invoke).toHaveBeenLastCalledWith("reveal_file", { reference: "issued-reference" });
});
