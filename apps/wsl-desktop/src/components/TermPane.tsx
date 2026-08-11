import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { broadcast, writeSession } from "../api";

interface TermPaneProps {
  sessionId: string;
  title: string;
  broadcastOn: boolean;
  registerWrite: (id: string, fn: (data: string) => void) => void;
  unregisterWrite: (id: string) => void;
  onClose: () => void;
}

const THEME = {
  background: "#111418",
  foreground: "#e6e9ef",
  cursor: "#4f8cff",
  selectionBackground: "#264f78",
};

export default function TermPane({ sessionId, title, broadcastOn, registerWrite, unregisterWrite, onClose }: TermPaneProps) {
  const ref = useRef<HTMLDivElement>(null);
  const broadcastRef = useRef(broadcastOn);
  broadcastRef.current = broadcastOn;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const term = new Terminal({
      fontSize: 13,
      fontFamily: '"Cascadia Code", Consolas, monospace',
      theme: THEME,
      cursorBlink: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    fit.fit();

    const dataDisposable = term.onData((data) => {
      if (broadcastRef.current) {
        void broadcast(data);
      } else {
        void writeSession(sessionId, data);
      }
    });

    registerWrite(sessionId, (data: string) => term.write(data));

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* noop */
      }
    });
    ro.observe(el);

    // 초기 프롬프트
    term.write("\x1b[2J\x1b[H");

    return () => {
      ro.disconnect();
      dataDisposable.dispose();
      unregisterWrite(sessionId);
      term.dispose();
    };
  }, [sessionId, registerWrite, unregisterWrite]);

  return (
    <div className="pane">
      <div className="pane-head">
        <span className="pane-title">{title}</span>
        <button className="pane-close" title="Close terminal" onClick={onClose}>
          ✕
        </button>
      </div>
      <div className="term-wrap" ref={ref} />
    </div>
  );
}
