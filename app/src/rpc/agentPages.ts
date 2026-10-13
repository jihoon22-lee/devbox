import { type RpcClient, RpcError } from "./client";
import type { Methods } from "./gen/rpc";

/** Publish a single list snapshot only when every bounded page has the same revision. */
export async function readAgentList(client: RpcClient, params: Methods["agents.list"]["params"], signal: AbortSignal) {
  for (let attempt = 0; attempt <= 2; attempt++) {
    const first = await client.call("agents.list", params, { signal });
    const items = [...first.items];
    const cursors = new Set<string>();
    let page = first;
    let drift = false;
    while (page.nextAfterId) {
      if (cursors.has(page.nextAfterId)) throw new RpcError("agents.list_changed");
      cursors.add(page.nextAfterId);
      page = await client.call("agents.list", { ...params, afterId: page.nextAfterId }, { signal });
      if (page.rev !== first.rev) {
        drift = true;
        break;
      }
      items.push(...page.items);
    }
    if (!drift) return { ...first, items, nextAfterId: undefined };
  }
  throw new RpcError("agents.list_changed");
}
