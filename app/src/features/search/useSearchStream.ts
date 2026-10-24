import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { ClientStream } from "../../rpc/client-streams";
import type { Scope, SearchHit, SearchQuery } from "../../rpc/gen/rpc";
import { compareRev } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { hitKey, readSearchChunk } from "./searchChunk";

export type SearchGroups = Record<Scope, SearchHit[]>;
const groups = (): SearchGroups => ({ notes: [], files: [], code: [] });
interface Snapshot {
  groups: SearchGroups;
  rev?: string;
  truncated: Scope[];
}
const empty = (): Snapshot => ({ groups: groups(), truncated: [] });
export function useSearchStream(query: SearchQuery | undefined) {
  const { client, state, generation } = useRpcSession();
  const identity = JSON.stringify(query ?? null);
  const [result, setResult] = useState<Snapshot>(empty);
  const [selection, select] = useState<string>();
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<RpcError>();
  const restart = useRef<() => void>(() => {});
  // biome-ignore lint/correctness/useExhaustiveDependencies: canonical query JSON defines request identity.
  useEffect(() => {
    let live = true;
    let successful: Snapshot = empty();
    let wanted: string | undefined;
    let run: { stream?: ClientStream; busy: boolean; done: boolean } | undefined;
    setResult(empty());
    select(undefined);
    setError(undefined);
    setRefreshing(false);
    restart.current = () => {};
    if (!query || state !== "ready") return;
    const start = () => {
      run?.stream?.end();
      const own = { busy: true, done: false, stream: undefined as ClientStream | undefined };
      run = own;
      const accumulated: Snapshot = empty();
      let pending = "";
      const decoder = new TextDecoder("utf-8", { fatal: true });
      const current = () => live && run === own;
      const fail = (failure: unknown) => {
        if (!current()) return;
        own.busy = false;
        setRefreshing(false);
        setResult(successful);
        setError(failure instanceof RpcError ? failure : new RpcError("internal"));
        own.stream?.end();
      };
      setRefreshing(true);
      setError(undefined);
      void client
        .openStream("search.query", query, {
          onData(bytes, ack) {
            try {
              if (!current() || own.done || !own.busy) return;
              pending += decoder.decode(bytes, { stream: true });
              if (pending.length > 1024 * 1024) throw Error("search chunk too large");
              let newline = pending.indexOf("\n");
              while (newline >= 0) {
                const chunk = readSearchChunk(pending.slice(0, newline));
                pending = pending.slice(newline + 1);
                if (accumulated.rev && accumulated.rev !== chunk.rev) throw Error("search revision changed midstream");
                accumulated.rev = chunk.rev;
                const items = accumulated.groups[chunk.scope];
                if (items.length + chunk.items.length > query.limit) throw Error("search result limit exceeded");
                accumulated.groups[chunk.scope] = [...items, ...chunk.items];
                if (chunk.truncated && !accumulated.truncated.includes(chunk.scope))
                  accumulated.truncated.push(chunk.scope);
                const snapshot = {
                  ...accumulated,
                  groups: { ...accumulated.groups },
                  truncated: [...accumulated.truncated],
                };
                if (!successful.rev || chunk.done) setResult(snapshot);
                if (chunk.done) {
                  own.done = true;
                  own.busy = false;
                  successful = snapshot;
                  own.stream?.end();
                  setRefreshing(false);
                  select((previous) =>
                    Object.values(snapshot.groups)
                      .flat()
                      .some((h) => hitKey(h) === previous)
                      ? previous
                      : undefined,
                  );
                  if (wanted && compareRev(wanted, chunk.rev) > 0)
                    queueMicrotask(() => {
                      if (current()) start();
                    });
                  break;
                }
                newline = pending.indexOf("\n");
              }
            } catch (failure) {
              fail(failure);
            } finally {
              ack();
            }
          },
          onEnd() {
            if (current() && !own.done && own.busy) fail(new RpcError("connection_lost"));
          },
        })
        .then((stream) => {
          if (!current() || !own.busy) stream.end();
          else own.stream = stream;
        }, fail);
    };
    restart.current = start;
    const off = client.subscribe(
      "search.changed",
      (_rev, change) => {
        if (!/^\d+$/.test(change.rev)) return;
        if (successful.rev && compareRev(change.rev, successful.rev) <= 0) return;
        if (!wanted || compareRev(change.rev, wanted) > 0) wanted = change.rev;
        if (!run?.busy) start();
      },
      () => {
        if (!run?.busy) start();
      },
    );
    start();
    return () => {
      live = false;
      off();
      run?.stream?.end();
    };
  }, [client, state, generation, identity]);
  return { ...result, selection, select, refreshing, error, retry: () => restart.current() };
}
