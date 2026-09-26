import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
export type AgentState = "connected" | "starting" | "unavailable" | "unsupported";
export default function AgentStatus({ initial = "unsupported", native }: { initial?: AgentState; native: boolean }) {
  const [status, setStatus] = useState(initial);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let eventRevision = 0;
    let unlisten: (() => void) | undefined;
    void listen<string>("product-shell://agent-status", ({ payload }) => {
      if (!disposed && ["connected", "starting", "unavailable", "unsupported"].includes(payload)) {
        eventRevision++;
        setStatus(payload as AgentState);
      }
    })
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          const revision = eventRevision;
          void invoke<AgentState>("plugin:product-shell|agent_status")
            .then((value) => {
              if (
                !disposed &&
                revision === eventRevision &&
                ["connected", "starting", "unavailable", "unsupported"].includes(value)
              )
                setStatus(value);
            })
            .catch(() => {
              if (!disposed && revision === eventRevision) setStatus("unavailable");
            });
        }
      })
      .catch(() => {
        if (!disposed) setStatus("unavailable");
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [native]);
  return status === "unavailable" ? <span role="status">백그라운드 서비스 연결 안 됨</span> : null;
}
