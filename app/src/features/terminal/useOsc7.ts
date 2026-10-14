import { useCallback, useEffect, useRef } from "react";
import type { RpcClient } from "../../rpc/client";
import { useRpcQuery } from "../../rpc/query";
import { parseOsc7 } from "./osc";

interface OscParser {
  registerOscHandler(id: number, handler: (data: string) => boolean): { dispose: () => void };
}

export function useOsc7(client: RpcClient, id: string, onCwd?: (path: string) => void) {
  const info = useRpcQuery("system.info", {}, { enabled: onCwd !== undefined });
  const owner = useRef({ client, id, onCwd });
  owner.current = { client, id, onCwd };
  const hostname = info.data?.hostname;
  const ready = info.isSuccess || info.isError;
  const metadata = useRef({ hostname, ready });
  metadata.current = { hostname, ready };
  const active = useRef(new Set<(hostname: string | undefined) => void>());
  const register = useCallback((parser: OscParser) => {
    const lifetime = owner.current;
    let disposed = false;
    let pending: string | undefined;
    const current = () => !disposed && owner.current.client === lifetime.client && owner.current.id === lifetime.id;
    const receive = (data: string, hostname = metadata.current.hostname) => {
      // Every new OSC supersedes the pending candidate, including invalid and
      // remote values. Metadata must never revive an earlier local prompt.
      pending = undefined;
      if (!current()) return true;
      const path = parseOsc7(data, hostname);
      if (path !== null) owner.current.onCwd?.(path);
      else if (!metadata.current.ready) pending = data;
      return true;
    };
    const refresh = (hostname: string | undefined) => {
      if (!metadata.current.ready || pending === undefined) return;
      receive(pending, hostname);
    };
    const handler = parser.registerOscHandler(7, receive);
    active.current.add(refresh);
    return {
      dispose: () => {
        disposed = true;
        pending = undefined;
        active.current.delete(refresh);
        handler.dispose();
      },
    };
  }, []);
  useEffect(() => {
    if (ready) for (const refresh of active.current) refresh(hostname);
  }, [hostname, ready]);
  return register;
}
