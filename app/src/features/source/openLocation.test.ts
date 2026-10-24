import { expect, test, vi } from "vitest";
import { resolveSourceLocation } from "./openLocation";

test("problem locations use the resolved file's registered project and then validate its key", async () => {
  const call = vi.fn(async (method: string, params: unknown) => {
    if (method === "observe.resolve_location") return { path: "/owned/other/src/가😀.rs", line: 10, column: 4 };
    if (method === "projects.list")
      return {
        rev: "1",
        items: [
          { id: "selected", path: "/owned/selected" },
          { id: "actual", path: "/owned/other" },
        ],
      };
    if (method === "files.stat")
      return { key: (params as { key: unknown }).key, kind: "file", rev: {}, readOnly: null };
    throw Error(method);
  });
  const result = await resolveSourceLocation({ call } as never, "selected", {
    path: "src/가😀.rs",
    line: 10,
    column: 4,
    source: { kind: "run", runId: "run-1" },
  });
  expect(result).toEqual({ key: { root: "project:actual", path: "src/가😀.rs" }, location: { line: 10, column: 4 } });
  expect(call).toHaveBeenCalledWith("files.stat", { key: result.key });
});
test("a similarly prefixed unregistered directory cannot become a project file", async () => {
  const call = vi.fn(async (method: string) =>
    method === "observe.resolve_location"
      ? { path: "/owned/project-host/x", line: 1, column: 1 }
      : { items: [{ id: "p", path: "/owned/project" }] },
  );
  await expect(
    resolveSourceLocation({ call } as never, "p", { path: "x", line: 1, source: { kind: "run", runId: "r" } }),
  ).rejects.toThrow();
  expect(call).not.toHaveBeenCalledWith("files.stat", expect.anything());
});
