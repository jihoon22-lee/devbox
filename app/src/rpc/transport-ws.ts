import type { Transport, TransportState } from "./client";
import { decodeFrame } from "./frame";

const DELAYS = [200, 500, 1000, 2000, 5000] as const;
export function wsTransport(url: string): Transport {
  const frames = new Set<(f: Uint8Array) => void>(),
    states = new Set<(s: TransportState) => void>();
  let socket: WebSocket | undefined, timer: ReturnType<typeof setTimeout> | undefined;
  let active = false,
    attempt = 0;
  const emit = (s: TransportState) => {
    for (const cb of states) cb(s);
  };
  const retry = () => {
    if (active && timer === undefined)
      timer = setTimeout(
        () => {
          timer = undefined;
          open();
        },
        DELAYS[Math.min(attempt++, DELAYS.length - 1)],
      );
  };
  const open = () => {
    if (!active || socket) return;
    emit("connecting");
    if (!active) return;
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch {
      retry();
      emit("closed");
      return;
    }
    socket = ws;
    ws.binaryType = "arraybuffer";
    const current = () => active && socket === ws;
    const lost = () => {
      if (!current()) return;
      socket = undefined;
      retry();
      emit("closed");
      ws.close();
    };
    ws.onopen = () => {
      if (current()) {
        attempt = 0;
        emit("open");
      }
    };
    ws.onclose = lost;
    ws.onerror = lost;
    ws.onmessage = (e) => {
      if (!current()) return;
      let raw: Uint8Array;
      try {
        if (!(e.data instanceof ArrayBuffer)) throw new Error("Binary frame required");
        raw = new Uint8Array(e.data);
        decodeFrame(raw);
      } catch {
        lost();
        return;
      }
      for (const cb of frames) cb(raw);
    };
  };
  return {
    connect() {
      if (active) return;
      active = true;
      open();
    },
    reconnect() {
      const old = socket;
      socket = undefined;
      active = true;
      retry();
      emit("closed");
      old?.close();
    },
    close() {
      active = false;
      attempt = 0;
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
      const old = socket;
      socket = undefined;
      old?.close();
      if (old) emit("closed");
    },
    send(frame) {
      if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error("WebSocket is not open");
      socket.send(frame.slice());
    },
    onFrame(cb) {
      frames.add(cb);
      return () => {
        frames.delete(cb);
      };
    },
    onState(cb) {
      states.add(cb);
      return () => {
        states.delete(cb);
      };
    },
  };
}
