import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { Description } from "@devbox/product-shell/api";
import type { RuntimeLogOpenRequest } from "@devbox/workspace-features/logs";
import { componentCall } from "./native";
import { terminalLogRequest } from "./runtimeNavigation";

/** The cold shell consumes only native intent metadata; it starts no collector. */
export default function TerminalLogBridge({ description, consumedId, onOpen }: { description: Description; consumedId: string | null; onOpen: (request: RuntimeLogOpenRequest) => void }) {
  const latest = useRef({ description, onOpen });
  latest.current = { description, onOpen };
  const refresh = useRef<() => Promise<void>>(async () => undefined);
  const acknowledged = useRef<string | null>(null);
  const [issue, setIssue] = useState("");
  useEffect(() => {
    let disposed = false;
    let pending = false;
    let again = false;
    let unlisten: (() => void) | undefined;
    const read = async () => {
      if (pending) { again = true; return; }
      pending = true;
      try {
        do {
          again = false;
          const value = await componentCall<unknown>(latest.current.description, "workspace.terminal", "read_terminal_log", {}, "terminal");
          if (disposed) return;
          const request = terminalLogRequest(value, latest.current.description.context);
          if (request) latest.current.onOpen(request);
        } while (again && !disposed);
      } catch { if (!disposed) setIssue("터미널의 로그 요청을 확인하지 못했습니다."); }
      finally { pending = false; }
    };
    refresh.current = read;
    void listen("workspace://terminal-log-ready", () => void read()).then(stop => { if (disposed) stop(); else { unlisten = stop; void read(); } }).catch(() => { if (!disposed) setIssue("터미널 로그 연결을 시작하지 못했습니다."); });
    return () => { disposed = true; unlisten?.(); };
  }, [description.context]);
  useEffect(() => {
    if (!consumedId || acknowledged.current === consumedId) return;
    let disposed=false;
    void (async()=>{
      for(let attempt=0;attempt<3&&!disposed;attempt++) {
        try {
          await componentCall(latest.current.description,"workspace.terminal","ack_terminal_log",{id:consumedId},"terminal");
          acknowledged.current=consumedId;
          await refresh.current();return;
        } catch { if(attempt===2&&!disposed)setIssue("로그 요청의 완료 상태를 확인하지 못했습니다."); }
        await new Promise(resolve=>setTimeout(resolve,100));
      }
    })();
    return()=>{disposed=true;};
  }, [consumedId, description.context]);
  return issue ? <p role="status">{issue}</p> : null;
}
