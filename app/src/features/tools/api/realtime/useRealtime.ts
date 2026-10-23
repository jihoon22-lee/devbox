/**
 * 실시간 연결 하나의 화면 상태. 새로 열면 이전 연결을 닫고, 화면을 떠나면(언마운트) 닫는다.
 * 늦게 온 이전 연결의 이벤트는 세대 번호로 버린다.
 */
import { useEffect, useRef, useState } from "react";
import type { SseOpen, WsInput, WsOpen } from "../../../../rpc/gen/rpc";
import { type RealtimeHandlers, type RealtimeSession, SendFailure, useApiBackend } from "../backend";
import { apply, emptyLog, type Log } from "./messages";

const live = (log: Log) => log.status === "connecting" || log.status === "open" || log.status === "reconnecting";

export function useRealtime() {
  const backend = useApiBackend();
  const [log, setLog] = useState<Log>(emptyLog);
  const session = useRef<RealtimeSession | null>(null);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
      session.current?.close();
      session.current = null;
    },
    [],
  );

  function handlers(gen: number): RealtimeHandlers {
    return {
      onEvent: (event) => {
        if (gen === generation.current) setLog((current) => apply(current, event));
      },
      onEnd: () => {
        if (gen !== generation.current) return;
        session.current = null;
        setLog((current) => (live(current) ? { ...current, status: "closed" } : current));
      },
    };
  }

  async function open(params: { ws: WsOpen } | { sse: SseOpen }) {
    session.current?.close();
    session.current = null;
    const gen = ++generation.current;
    setLog({ ...emptyLog(), status: "connecting" });
    try {
      const opened = await ("ws" in params
        ? backend.openWs(params.ws, handlers(gen))
        : backend.openSse(params.sse, handlers(gen)));
      if (gen !== generation.current) opened.close();
      else session.current = opened;
    } catch (error) {
      if (gen !== generation.current) return;
      const failure =
        error instanceof SendFailure ? { code: error.code, detail: error.detail } : { code: "internal" as const };
      setLog((current) => ({ ...current, status: "failed", failure }));
    }
  }

  return {
    log,
    open,
    send: (input: WsInput) => session.current?.send(input),
    close() {
      generation.current += 1;
      session.current?.close();
      session.current = null;
      setLog((current) => (live(current) ? { ...current, status: "closed" } : current));
    },
    clear: () => setLog((current) => ({ ...current, entries: [], bytes: 0, trimmed: 0, dropped: 0 })),
  };
}
