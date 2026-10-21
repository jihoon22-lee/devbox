import { type RpcClient, RpcError } from "./client";
import type { Methods } from "./gen/rpc";

/** The UI publishes all definition pages only at one revision, like agent lists. */
export async function readRuntimeList(
  client: RpcClient,
  params: Methods["runtime.list"]["params"],
  signal: AbortSignal,
) {
  for (let attempt = 0; attempt < 3; attempt++) {
    const first = await client.call("runtime.list", params, { signal });
    const defs = [...first.defs],
      shadowed = [...first.shadowed];
    const runs = new Map(first.runs.map((run) => [run.id, run]));
    const cursors = new Set<string>();
    let page = first,
      drift = false;
    while (page.nextCursor) {
      const key = JSON.stringify(page.nextCursor);
      if (cursors.has(key)) throw new RpcError("runtime.invalid_cursor");
      cursors.add(key);
      page = await client.call("runtime.list", { ...params, cursor: page.nextCursor }, { signal });
      if (page.rev !== first.rev) {
        drift = true;
        break;
      }
      defs.push(...page.defs);
      shadowed.push(...page.shadowed);
      for (const run of page.runs) runs.set(run.id, run);
    }
    if (!drift) return { ...first, defs, shadowed, runs: [...runs.values()], nextCursor: null };
  }
  throw new RpcError("runtime.invalid_cursor");
}
