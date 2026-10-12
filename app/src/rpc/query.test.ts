import { QueryClient } from "@tanstack/react-query";
import { compareRev, invalidateIfNewer, setIfNewer } from "./query";

test("revisions use integer ordering and equal revision preserves observation-only data", async () => {
  expect(compareRev("10", "9")).toBe(1);
  expect(compareRev("9007199254740993", "9007199254740992")).toBe(1);
  expect(compareRev("5", "5")).toBe(0);
  const qc = new QueryClient();
  const key = ["projects.list", {}];
  qc.setQueryData(key, { rev: "12", observedMs: 1 });
  setIfNewer(qc, key, { rev: "11" });
  setIfNewer(qc, key, { rev: "12", observedMs: 2 });
  expect(qc.getQueryData(key)).toEqual({ rev: "12", observedMs: 1 });
  const invalidate = vi.spyOn(qc, "invalidateQueries");
  await invalidateIfNewer(qc, [key], "12");
  expect(invalidate).not.toHaveBeenCalled();
  await invalidateIfNewer(qc, [key], "13");
  expect(qc.getQueryState(key)?.isInvalidated).toBe(true);
  qc.clear();
});
