import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import type { AgentChanges, AgentPatch } from "../../../rpc/gen/rpc";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { reviewSnapshot } from "./reviewPatch";

export function useReviewSnapshot(agentId: string) {
  const { qc, generation } = useRpcSession();
  const subscribe = useCallback(
    (notify: () => void) =>
      qc.getQueryCache().subscribe((event) => {
        const [method, params] = event.query.queryKey;
        if (
          method === "agents.changes" &&
          typeof params === "object" &&
          params &&
          "id" in params &&
          params.id === agentId
        )
          notify();
      }),
    [qc, agentId],
  );
  return useSyncExternalStore(subscribe, () => reviewSnapshot(qc, agentId, generation));
}

export type ReviewFile = AgentChanges["files"][number];
export function useReviewSignature(
  changes: AgentChanges | undefined,
  file: ReviewFile | undefined,
  patch: AgentPatch | undefined,
) {
  const content =
    changes && file && patch && !patch.truncated
      ? JSON.stringify([changes.headOid, changes.baseOid, file, patch.patch])
      : undefined;
  const [result, setResult] = useState<{ content: string; signature: string }>();
  useEffect(() => {
    if (content === undefined) return;
    let current = true;
    void (async () => {
      try {
        const hash = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(content));
        const signature = `sha256:${Array.from(new Uint8Array(hash), (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
        if (current) setResult({ content, signature });
      } catch {
        // An unavailable digest must never turn unverified content into a seen mark.
        if (current) setResult(undefined);
      }
    })();
    return () => {
      current = false;
    };
  }, [content]);
  return result && result.content === content ? result.signature : undefined;
}
