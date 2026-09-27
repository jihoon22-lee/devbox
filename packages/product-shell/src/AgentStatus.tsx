import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
export type AgentState = "connected" | "starting" | "restarting" | "unavailable" | "unsupported";
const states = ["connected", "starting", "restarting", "unavailable", "unsupported"];
function valid(value: unknown): value is AgentState {
  return typeof value === "string" && states.includes(value);
}
export default function AgentStatus({ initial = "unsupported", native }: { initial?: AgentState; native: boolean }) {
  const [status, setStatus] = useState(initial);
  const [retrying, setRetrying] = useState(false);
  const eventRevision = useRef(0);
  const lifetime = useRef(0);
  useEffect(() => {
    const generation = ++lifetime.current;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if (native) {
      void listen<string>("product-shell://agent-status", ({ payload }) => {
        if (!disposed && valid(payload)) {
          eventRevision.current++;
          setStatus(payload);
        }
      })
        .then((stop) => {
          if (disposed) stop();
          else {
            unlisten = stop;
            const revision = eventRevision.current;
            void invoke<AgentState>("plugin:product-shell|agent_status")
              .then((value) => {
                if (!disposed && revision === eventRevision.current && valid(value)) setStatus(value);
              })
              .catch(() => {
                if (!disposed && revision === eventRevision.current) setStatus("unavailable");
              });
          }
        })
        .catch(() => {
          if (!disposed) setStatus("unavailable");
        });
    }
    return () => {
      disposed = true;
      if (lifetime.current === generation) lifetime.current++;
      unlisten?.();
    };
  }, [native]);
  const reconnect = async () => {
    if (!native || retrying) return;
    const generation = lifetime.current,
      revision = ++eventRevision.current;
    setRetrying(true);
    try {
      const value = await invoke<AgentState>("plugin:product-shell|agent_reconnect");
      if (generation === lifetime.current && revision === eventRevision.current && valid(value)) {
        eventRevision.current++;
        setStatus(value);
      }
    } catch {
      if (generation === lifetime.current && revision === eventRevision.current) {
        eventRevision.current++;
        setStatus("unavailable");
      }
    } finally {
      if (generation === lifetime.current) setRetrying(false);
    }
  };
  if (status === "restarting") return <span role="status">백그라운드 서비스를 다시 시작하고 있습니다.</span>;
  return status === "unavailable" ? (
    <span role="status">
      백그라운드 서비스 연결 안 됨{" "}
      {native && (
        <button
          type="button"
          aria-label="백그라운드 서비스 다시 연결"
          disabled={retrying}
          onClick={() => void reconnect()}
        >
          다시 연결
        </button>
      )}
    </span>
  ) : null;
}
