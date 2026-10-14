import type { QueryClient } from "@tanstack/react-query";
import { RpcError } from "../../../rpc/client";
import type { AgentPatch } from "../../../rpc/gen/rpc";

type Read = { snapshot: string; pending?: Promise<AgentPatch> };
const reads = new WeakMap<QueryClient, Map<string, Read>>();
export function reviewSnapshot(qc: QueryClient, agentId: string, generation: number) {
  return JSON.stringify([
    agentId,
    generation,
    qc.getQueryState(["agents.changes", { id: agentId }])?.dataUpdateCount ?? 0,
  ]);
}

/** ChangesTab and confirmation must await the same post-receipt patch read. */
export function freshReviewPatch(
  qc: QueryClient,
  agentId: string,
  path: string,
  snapshot: string,
): Promise<AgentPatch> {
  let owned = reads.get(qc);
  if (!owned) {
    owned = new Map();
    reads.set(qc, owned);
  }
  const id = JSON.stringify([agentId, path]);
  const previous = owned.get(id);
  if (previous?.snapshot === snapshot && previous.pending) return previous.pending;
  const read: Read = { snapshot };
  owned.set(id, read);
  const key = ["agents.patch", { id: agentId, path }];
  read.pending = (async () => {
    if (previous && previous.snapshot !== snapshot) await qc.cancelQueries({ queryKey: key, exact: true });
    if (owned.get(id) !== read) throw new RpcError("connection_lost");
    const before = qc.getQueryState(key)?.dataUpdateCount ?? 0;
    await qc.refetchQueries({ queryKey: key, exact: true }, { cancelRefetch: false, throwOnError: true });
    if (owned.get(id) !== read) throw new RpcError("connection_lost");
    const state = qc.getQueryState<AgentPatch>(key);
    if (
      !state ||
      state.dataUpdateCount <= before ||
      state.fetchStatus !== "idle" ||
      state.status !== "success" ||
      state.isInvalidated
    )
      throw new RpcError("connection_lost");
    const data = state.data;
    if (!data) throw new RpcError("internal");
    return data;
  })();
  void read.pending.then(
    () => {
      read.pending = undefined;
    },
    () => {
      read.pending = undefined;
    },
  );
  return read.pending;
}
