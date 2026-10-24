import { useQuery } from "@tanstack/react-query";
import { type RpcClient, RpcError } from "../../rpc/client";
import type { NoteTree, TreeItem } from "../../rpc/gen/rpc";
import { useTopicInvalidation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";

export interface WholeTree {
  rev: string;
  items: TreeItem[];
  truncated: boolean;
}

async function readOnce(client: RpcClient, signal: AbortSignal): Promise<WholeTree> {
  const items: TreeItem[] = [];
  let page: NoteTree = await client.call("notes.tree", { cursor: null }, { signal });
  items.push(...page.items);
  // Later pages come from the first page's snapshot, so the whole listing shares one rev.
  while (page.nextCursor) {
    page = await client.call("notes.tree", { cursor: page.nextCursor }, { signal });
    items.push(...page.items);
  }
  return { rev: page.rev, items, truncated: page.truncated };
}

/** Every page of `notes.tree`. An expired snapshot starts over once; a second expiry is an error. */
export async function readWholeTree(client: RpcClient, signal: AbortSignal): Promise<WholeTree> {
  try {
    return await readOnce(client, signal);
  } catch (error) {
    if (!(error instanceof RpcError) || error.code !== "notes.stale_cursor") throw error;
    return readOnce(client, signal);
  }
}

/**
 * The vault tree. `notes.tree` is paged (256 rows, up to 20,000), and `rpc/query.ts` has no pager
 * for it, so the feature reads all pages itself in the same query cache, keyed under `notes.tree`
 * so `notes.changed` refreshes it.
 */
export function useNoteTree() {
  const { client, state, generation } = useRpcSession();
  useTopicInvalidation("notes.changed", [["notes.tree"]]);
  return useQuery<WholeTree, RpcError>({
    queryKey: ["notes.tree", { all: true }],
    enabled: state === "ready",
    networkMode: "always",
    retry: false,
    meta: { generation },
    queryFn: ({ signal }) => readWholeTree(client, signal),
  });
}
