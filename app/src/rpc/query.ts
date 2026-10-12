import { type QueryKey, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import type { RpcError } from "./client";
import { METHOD_INFO, type Methods, type Topics } from "./gen/rpc";
import { useRpc, useRpcSession } from "./RpcProvider";
import { invalidateIfNewer, shareByRevision } from "./revisions";

export { compareRev, invalidateIfNewer, setIfNewer } from "./revisions";

type QueryMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "query" ? M : never }[keyof Methods];
type MutationMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "mutation" ? M : never }[keyof Methods];
export function useRpcQuery<M extends QueryMethod>(
  method: M,
  params: Methods[M]["params"],
  opts: { enabled?: boolean } = {},
) {
  const { client, state, generation } = useRpcSession();
  return useQuery<Methods[M]["result"], RpcError>({
    queryKey: [method, params],
    enabled: (opts.enabled ?? true) && state === "ready",
    retry: false,
    meta: { generation },
    structuralSharing: shareByRevision,
    queryFn: ({ signal }) => client.call(method, params, { signal }),
  });
}
export function useRpcMutation<M extends MutationMethod>(method: M) {
  const client = useRpc();
  return useMutation<Methods[M]["result"], RpcError, Methods[M]["params"]>({
    mutationFn: (params) => client.call(method, params, { clientRequestId: crypto.randomUUID() }),
    retry: false,
  });
}
export function confirmPolicy(method: keyof Methods) {
  return METHOD_INFO[method].confirm;
}
export function useTopicInvalidation<T extends keyof Topics>(topic: T, keys: readonly QueryKey[]) {
  const session = useRpcSession();
  const qc = useQueryClient();
  const keyText = JSON.stringify(keys);
  // biome-ignore lint/correctness/useExhaustiveDependencies: JSON query keys represent subscription identity; session snapshot is intentionally captured only for its live restoring getter.
  useEffect(
    () =>
      session.client.subscribe(
        topic,
        (rev) => {
          void invalidateIfNewer(qc, keys, rev);
        },
        () => {
          if (!session.restoring) void invalidateIfNewer(qc, keys, null);
        },
      ),
    [session.client, qc, topic, keyText],
  );
}
