import { vi } from "vitest";
import type { RpcClient } from "./client";
import type { DefinitionList } from "./gen/rpc";
import { shareRuntimeList } from "./revisions";
import { readRuntimeList } from "./runtimePages";

const page = (rev: string, more = false): DefinitionList => ({
  rev,
  defs: [],
  shadowed: [],
  runs: [],
  trust: "not_required",
  nextCursor: more ? { id: "last", shadowed: false } : null,
});
test("collects later definition pages without losing page-specific runs", async () => {
  const first = page("1", true),
    last = page("1");
  first.defs = [
    {
      source: "thisPc",
      def: { kind: "schedule", id: "first", task: "t", cron: "* * * * *", tz: null, missed: "skip" },
    },
  ];
  last.defs = [{ ...first.defs[0]!, def: { ...first.defs[0]!.def, id: "later" } }];
  const call = vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(last);
  const signal = new AbortController().signal;
  const result = await readRuntimeList({ call } as unknown as RpcClient, { projectId: "p", cursor: null }, signal);
  expect(result.defs.map((d) => d.def.id)).toEqual(["first", "later"]);
  expect(result.nextCursor).toBeNull();
  expect(call).toHaveBeenLastCalledWith("runtime.list", { projectId: "p", cursor: first.nextCursor }, { signal });
});
test("restarts a drifting snapshot and bounds unstable/repeated cursors", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(page("1", true))
    .mockResolvedValueOnce(page("2"))
    .mockResolvedValueOnce(page("3"));
  expect(
    (
      await readRuntimeList(
        { call } as unknown as RpcClient,
        { projectId: "p", cursor: null },
        new AbortController().signal,
      )
    ).rev,
  ).toBe("3");
  const loop = vi.fn().mockResolvedValue(page("1", true));
  await expect(
    readRuntimeList(
      { call: loop } as unknown as RpcClient,
      { projectId: "p", cursor: null },
      new AbortController().signal,
    ),
  ).rejects.toMatchObject({ code: "runtime.invalid_cursor" });
  expect(loop).toHaveBeenCalledTimes(2);
  const drifting = vi.fn().mockImplementation(async () => page(String(drifting.mock.calls.length), true));
  await expect(
    readRuntimeList(
      { call: drifting } as unknown as RpcClient,
      { projectId: "p", cursor: null },
      new AbortController().signal,
    ),
  ).rejects.toMatchObject({ code: "runtime.invalid_cursor" });
  expect(drifting).toHaveBeenCalledTimes(6);
});

test("authoritative runtime list at the saved revision replaces partial cache trust and runs, lower revision cannot", () => {
  const partial = { ...page("3"), trust: "approval_required" as const };
  const authoritative = { ...page("3"), trust: "approved" as const };
  expect(shareRuntimeList(partial, authoritative)).toEqual(authoritative);
  expect(shareRuntimeList(authoritative, page("2"))).toBe(authoritative);
});
