import { useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button } from "../../ui";

/** The daemon chooses today's daily path and edits it through the existing lease/rewrite flow. */
export function InsertSummary({ onOpen }: { onOpen(path: string): void }) {
  const { client, state } = useRpcSession();
  const inFlight = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const insert = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(undefined);
    try {
      const result = await client.call("notes.insert_summary", { date: null });
      onOpen(result.path);
    } catch (failure) {
      const e = failure instanceof RpcError ? failure : new RpcError("internal");
      setError(messageFor(e.code, e.detail).title);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };
  return (
    <span>
      <Button disabled={busy || state !== "ready"} onClick={() => void insert()}>
        오늘 한 일 넣기
      </Button>
      {busy && <span role="status">오늘 한 일을 넣는 중입니다…</span>}
      {error && <span role="alert">{error}</span>}
    </span>
  );
}
