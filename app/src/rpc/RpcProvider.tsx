import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import type { ClientState, RpcClient } from "./client";
import { useConnection } from "./connection";

interface Session {
  client: RpcClient;
  qc: QueryClient;
  state: ClientState;
  generation: number;
  restoring: boolean;
}
const Context = createContext<Session | null>(null);
export function useRpcSession(): Session {
  const session = useContext(Context);
  if (!session) throw new Error("RpcProvider missing");
  return session;
}
export function useRpc(): RpcClient {
  return useRpcSession().client;
}

// A changed client is a different server session and must not inherit any observers.
export function RpcProvider({ client, children }: { client: RpcClient; children: ReactNode }) {
  const [identity, setIdentity] = useState({ client, key: 0 });
  if (identity.client !== client) setIdentity({ client, key: identity.key + 1 });
  return (
    <SessionProvider key={identity.key} client={client}>
      {children}
    </SessionProvider>
  );
}
function SessionProvider({ client, children }: { client: RpcClient; children: ReactNode }) {
  const [runtime] = useState<Session>(() => ({
    client,
    qc: new QueryClient({
      defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: false } },
    }),
    state: client.state,
    generation: 0,
    restoring: false,
  }));
  const [snapshot, setSnapshot] = useState<Session>({ ...runtime });
  useEffect(() => {
    const connection = useConnection.getState();
    connection.reset();
    const publish = () => setSnapshot({ ...runtime });
    const onState = (state: ClientState) => {
      runtime.state = state;
      connection.set(state, client.daemonVersion);
      if (state === "ready") {
        // RpcClient emits ready only after all restored subscription ACKs. The
        // immediately following onResync callbacks belong to this same refresh.
        runtime.restoring = true;
        void runtime.qc.invalidateQueries({}, { cancelRefetch: false });
        queueMicrotask(() => {
          runtime.restoring = false;
        });
      } else {
        void runtime.qc.cancelQueries();
      }
      publish();
    };
    const offState = client.onState(onState);
    const offReset = client.onReset(() => {
      runtime.qc.clear();
      runtime.generation++;
      // Changing context rerenders query hooks. Their observers rebuild against
      // the emptied cache in setOptions, preserving mounted component state.
      publish();
    });
    onState(client.state);
    client.start();
    return () => {
      offState();
      offReset();
      void runtime.qc.cancelQueries();
      client.stop();
    };
  }, [client, runtime]);
  return (
    <Context.Provider
      value={{
        ...snapshot,
        get restoring() {
          return runtime.restoring;
        },
      }}
    >
      <QueryClientProvider client={runtime.qc}>{children}</QueryClientProvider>
    </Context.Provider>
  );
}
