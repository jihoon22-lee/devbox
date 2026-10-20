import { hashKey, type QueryKey, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { readAgentList } from "./agentPages";
import { type RpcClient, RpcError } from "./client";
import { METHOD_INFO, type Methods, type Topics } from "./gen/rpc";
import { useRpc, useRpcSession } from "./RpcProvider";
import { invalidateIfNewer, shareByRevision, shareRuntimeList, shareTerminalLive } from "./revisions";
import { readRuntimeList } from "./runtimePages";
import { readTerminalList } from "./terminalPages";

export { compareRev, invalidateIfNewer, setIfNewer } from "./revisions";

type QueryMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "query" ? M : never }[keyof Methods];
type MutationMethod = { [M in keyof Methods]: Methods[M]["kind"] extends "mutation" ? M : never }[keyof Methods];
export function useRpcQuery<M extends QueryMethod>(
  method: M,
  params: Methods[M]["params"],
  opts: {
    enabled?: boolean;
    staleTime?: number;
    refetchInterval?: number | false;
    refetchIntervalInBackground?: boolean;
    refetchOnWindowFocus?: boolean | "always";
  } = {},
) {
  const { client, state, generation } = useRpcSession();
  return useQuery<Methods[M]["result"], RpcError>({
    queryKey: [method, params],
    enabled: (opts.enabled ?? true) && state === "ready",
    // Local RPC readiness is independent of the browser's internet connection.
    networkMode: "always",
    retry: false,
    ...(opts.staleTime === undefined ? {} : { staleTime: opts.staleTime }),
    refetchInterval: opts.refetchInterval,
    refetchIntervalInBackground: opts.refetchIntervalInBackground,
    ...(opts.refetchOnWindowFocus === undefined ? {} : { refetchOnWindowFocus: opts.refetchOnWindowFocus }),
    meta: { generation },
    structuralSharing:
      method === "terminal.list" ? shareTerminalLive : method === "runtime.list" ? shareRuntimeList : shareByRevision,
    queryFn: ({ signal }) =>
      method === "agents.list"
        ? (readAgentList(client, params as Methods["agents.list"]["params"], signal) as Promise<Methods[M]["result"]>)
        : method === "terminal.list"
          ? (readTerminalList(client, params as Methods["terminal.list"]["params"], signal) as Promise<
              Methods[M]["result"]
            >)
          : method === "runtime.list"
            ? (readRuntimeList(client, params as Methods["runtime.list"]["params"], signal) as Promise<
                Methods[M]["result"]
              >)
            : client.call(method, params, { signal }),
  });
}
export function useRpcMutation<M extends MutationMethod>(method: M) {
  const client = useRpc();
  type Params = Methods[M]["params"];
  type Operation = {
    key: string;
    id: string;
    method: M;
    client: RpcClient;
    daemonId?: string;
    ambiguous: boolean;
  };
  // Only the latest ambiguous invocation can be retried by submitting identical
  // params. reset() declares a new user intent, even when its params are identical.
  // Closing/unmounting discards this identity; server dedupe is not durable across
  // daemon restarts, so a new daemon always starts a fresh request.
  const unresolved = useRef<Operation | null>(null);
  const invocations = useRef(new WeakMap<Params, Operation>());
  useEffect(
    () => () => {
      unresolved.current = null;
    },
    [],
  );
  const prepare = (params: Params): Params => {
    const key = hashKey([method, params]);
    const previous = unresolved.current;
    const retry =
      previous?.ambiguous &&
      previous.key === key &&
      previous.client === client &&
      previous.daemonId === client.daemonId;
    const operation: Operation = {
      key,
      id: retry ? previous.id : crypto.randomUUID(),
      method,
      client,
      daemonId: client.daemonId,
      ambiguous: false,
    };
    // Snapshot at invocation, before TanStack schedules mutationFn. Both callback
    // variables and wire params use this copy, never the mutable caller reference.
    // Canonical JSON also matches the daemon's raw-JSON deduplication comparison.
    const snapshot = JSON.parse(key)[1] as Params;
    invocations.current.set(snapshot, operation);
    unresolved.current = operation;
    return snapshot;
  };
  const mutation = useMutation<Methods[M]["result"], RpcError, Params>({
    // Never queue local side effects for a later browser-online event.
    networkMode: "always",
    mutationFn: async (params) => {
      const operation = invocations.current.get(params)!;
      try {
        const result = await operation.client.call(operation.method, params, { clientRequestId: operation.id });
        if (unresolved.current === operation) unresolved.current = null;
        return result;
      } catch (error) {
        if (unresolved.current === operation) {
          if (error instanceof RpcError && error.code === "connection_lost") operation.ambiguous = true;
          else unresolved.current = null;
        }
        throw error;
      } finally {
        invocations.current.delete(params);
      }
    },
    retry: false,
  });
  return {
    ...mutation,
    mutate: (params: Params, options?: Parameters<typeof mutation.mutate>[1]) =>
      mutation.mutate(prepare(params), options),
    mutateAsync: (params: Params, options?: Parameters<typeof mutation.mutateAsync>[1]) =>
      mutation.mutateAsync(prepare(params), options),
    reset: () => {
      unresolved.current = null;
      mutation.reset();
    },
  };
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
