import { hashKey } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { type RpcClient, RpcError } from "../../rpc/client";
import type { Methods } from "../../rpc/gen/rpc";
import { useRpc } from "../../rpc/RpcProvider";

type SecretMutation = "secrets.set" | "secrets.import_key" | "secrets.delete" | "secrets.reset";
type Intent = {
  fingerprint: string;
  requestId: string;
  client: RpcClient;
  daemonId?: string;
  ambiguous: boolean;
};

// Preserve manual-retry identity without retaining plaintext in TanStack or refs.
export function useSecretMutation() {
  const client = useRpc();
  const unresolved = useRef<Intent | null>(null);
  useEffect(
    () => () => {
      unresolved.current = null;
    },
    [],
  );
  return async <M extends SecretMutation>(method: M, params: Methods[M]["params"]) => {
    const serialized = hashKey([method, params]);
    const bytes = new TextEncoder().encode(serialized);
    let fingerprint: string;
    try {
      const digest = await crypto.subtle.digest("SHA-256", bytes);
      fingerprint = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
    } finally {
      bytes.fill(0);
    }
    const previous = unresolved.current;
    const retry =
      previous?.ambiguous &&
      previous.fingerprint === fingerprint &&
      previous.client === client &&
      previous.daemonId === client.daemonId;
    const intent: Intent = {
      fingerprint,
      requestId: retry ? previous.requestId : crypto.randomUUID(),
      client,
      daemonId: client.daemonId,
      ambiguous: false,
    };
    unresolved.current = intent;
    try {
      const result = await client.call(method, JSON.parse(serialized)[1] as Methods[M]["params"], {
        clientRequestId: intent.requestId,
      });
      if (unresolved.current === intent) unresolved.current = null;
      return result;
    } catch (error) {
      if (unresolved.current === intent) {
        if (error instanceof RpcError && error.code === "connection_lost") intent.ambiguous = true;
        else unresolved.current = null;
      }
      throw error;
    }
  };
}
