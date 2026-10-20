import { type RpcClient, RpcError } from "./client";
import type { Methods } from "./gen/rpc";

/** Publish a complete bounded problem snapshot; never mix cursor revisions. */
export async function readProblemPages(
  client: RpcClient,
  params: Methods["observe.problems"]["params"],
  signal: AbortSignal,
) {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const first = await client.call("observe.problems", { ...params, cursor: undefined }, { signal });
      const items = [...first.items];
      const cursors = new Set<string>();
      let page = first;
      while (page.nextCursor) {
        const key = JSON.stringify(page.nextCursor);
        if (cursors.has(key) || cursors.size >= 2048) throw new RpcError("observe.invalid_cursor");
        cursors.add(key);
        page = await client.call("observe.problems", { ...params, cursor: page.nextCursor }, { signal });
        if (page.rev !== first.rev) throw new RpcError("observe.invalid_cursor");
        items.push(...page.items);
        if (items.length > 2048) throw new RpcError("observe.limit_exceeded");
      }
      return { ...first, items, nextCursor: null, truncated: first.truncated || page.truncated };
    } catch (error) {
      if (!(error instanceof RpcError) || error.code !== "observe.invalid_cursor" || attempt === 2 || signal.aborted)
        throw error;
    }
  }
  throw new RpcError("observe.invalid_cursor");
}
