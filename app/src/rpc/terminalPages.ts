import { type RpcClient, RpcError } from "./client";
import type { Methods } from "./gen/rpc";

/** Keep all DB rows at one revision; live tmux observations may change independently. */
export async function readTerminalList(
  client: RpcClient,
  params: Methods["terminal.list"]["params"],
  signal: AbortSignal,
) {
  for (let attempt = 0; attempt <= 2; attempt++) {
    const first = await client.call("terminal.list", params, { signal });
    const items = [...first.items];
    const cursors = new Set<string>();
    let page = first;
    let drift = false;
    while (page.nextAfterId) {
      if (cursors.has(page.nextAfterId)) throw new RpcError("terminal.list_changed");
      cursors.add(page.nextAfterId);
      page = await client.call("terminal.list", { ...params, afterId: page.nextAfterId }, { signal });
      if (page.rev !== first.rev) {
        drift = true;
        break;
      }
      items.push(...page.items);
    }
    if (!drift) return { ...first, items, nextAfterId: undefined };
  }
  throw new RpcError("terminal.list_changed");
}
