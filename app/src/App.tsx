import { useEffect, useState } from "react";
import { RpcClient, type Transport } from "./rpc/client";
import { RpcProvider } from "./rpc/RpcProvider";
import { wsTransport } from "./rpc/transport-ws";
import { Shell } from "./shell/Shell";
import { Button, ErrorState, TooltipProvider } from "./ui";
declare const __APP_VERSION__: string;
export async function pickTransport(
  search = location.search,
  fallback = import.meta.env.VITE_DEVBOX_WS,
): Promise<Transport> {
  const candidate = new URLSearchParams(search).get("gateway") ?? fallback;
  if (!candidate) throw new Error("gateway_missing");
  const url = new URL(candidate);
  if (!["ws:", "wss:"].includes(url.protocol)) throw new Error("gateway_invalid");
  return wsTransport(url.href);
}
export function App({ loadTransport = pickTransport }: { loadTransport?: () => Promise<Transport> }) {
  const [attempt, setAttempt] = useState(0);
  const [client, setClient] = useState<RpcClient | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    let owned: Transport | undefined;
    setClient(null);
    setFailed(false);
    void Promise.resolve()
      .then(() => loadTransport())
      .then((transport) => {
        if (cancelled) {
          transport.close();
          return;
        }
        owned = transport;
        setClient(
          new RpcClient(transport, {
            version: __APP_VERSION__,
            instance: import.meta.env.VITE_DEVBOX_INSTANCE ?? "dev",
            client: "dev",
          }),
        );
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      owned?.close();
    };
  }, [loadTransport, attempt]);
  if (failed)
    return (
      <ErrorState
        title="연결을 준비하지 못했습니다. 개발 연결 주소를 확인하세요."
        diagnosticId="app.transport_startup"
        action={<Button onClick={() => setAttempt((value) => value + 1)}>다시 시도</Button>}
      />
    );
  if (!client) return <p role="status">연결을 준비하고 있습니다</p>;
  return (
    <TooltipProvider>
      <RpcProvider client={client}>
        <Shell />
      </RpcProvider>
    </TooltipProvider>
  );
}
