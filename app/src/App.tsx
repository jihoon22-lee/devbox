import { useEffect, useState } from "react";
import { DocumentCloseDialog } from "./features/documents/DocumentCloseDialog";
import { RpcClient, type Transport } from "./rpc/client";
import { RpcProvider } from "./rpc/RpcProvider";
import { tauriTransport } from "./rpc/transport-tauri";
import { wsTransport } from "./rpc/transport-ws";
import { ConnectionGate } from "./shell/ConnectionGate";
import { NativeMainController } from "./shell/NativeMainController";
import { NativeQuitPreparation } from "./shell/NativeQuitPreparation";
import { Shell } from "./shell/Shell";
import { Button, ErrorState, TooltipProvider } from "./ui";

declare const __APP_VERSION__: string;
export async function pickTransport(
  search = location.search,
  fallback = import.meta.env.VITE_DEVBOX_WS,
): Promise<Transport> {
  if ("__TAURI_INTERNALS__" in window) return tauriTransport();
  const candidate = new URLSearchParams(search).get("gateway") ?? fallback;
  if (!candidate) throw new Error("gateway_missing");
  const url = new URL(candidate);
  if (!["ws:", "wss:"].includes(url.protocol)) throw new Error("gateway_invalid");
  return wsTransport(url.href);
}
export function App({ loadTransport = pickTransport }: { loadTransport?: () => Promise<Transport> }) {
  const native = "__TAURI_INTERNALS__" in window;
  const [ready, setReady] = useState(false);
  if (native)
    return (
      <>
        <NativeQuitPreparation />
        <DocumentCloseDialog />
        <NativeMainController ready={ready} />
        <ConnectionGate onReady={() => setReady(true)}>
          {(instance) => <ConnectedApp loadTransport={loadTransport} instance={instance} native />}
        </ConnectionGate>
      </>
    );
  return (
    <>
      <DocumentCloseDialog />
      <ConnectedApp
        loadTransport={loadTransport}
        instance={import.meta.env.VITE_DEVBOX_INSTANCE ?? "dev"}
        native={false}
      />
    </>
  );
}
function ConnectedApp({
  loadTransport,
  instance,
  native,
}: {
  loadTransport: () => Promise<Transport>;
  instance: string;
  native: boolean;
}) {
  const [attempt, setAttempt] = useState(0);
  const [client, setClient] = useState<RpcClient | null>(null);
  const [failed, setFailed] = useState(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: an explicit retry restarts initialization.
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
            instance,
            client: native ? "app" : "dev",
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
  }, [loadTransport, attempt, instance, native]);
  if (failed)
    return (
      <ErrorState
        title={
          native
            ? "연결을 준비하지 못했습니다. 다시 시도하세요."
            : "연결을 준비하지 못했습니다. 개발 연결 주소를 확인하세요."
        }
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
