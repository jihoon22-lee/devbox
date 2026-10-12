import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Transport, TransportState } from "./client";
import { decodeFrame } from "./frame";

interface TransportNotice {
  attachmentId: string;
  state: "open" | "closed";
  revision: string;
}
interface Attachment {
  id: string;
  off?: () => void;
  attaching: boolean;
  open: boolean;
  revision: bigint;
  queue: Uint8Array[];
  bytes: number;
  writing: boolean;
  epoch: number;
}
const DELAYS = [200, 500, 1000, 2000, 5000] as const;
export function tauriTransport(): Transport {
  const frames = new Set<(frame: Uint8Array) => void>();
  const states = new Set<(state: TransportState) => void>();
  let active = false;
  let attachment: Attachment | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let attempt = 0;
  const emit = (state: TransportState) => {
    for (const cb of states) cb(state);
  };
  const current = (a: Attachment) => active && attachment === a;
  const detach = (a: Attachment) => {
    void invoke("rpc_detach", { attachmentId: a.id }).catch(() => {});
  };
  const clearWrites = (a: Attachment) => {
    a.queue = [];
    a.bytes = 0;
    a.epoch++;
  };
  const release = () => {
    const old = attachment;
    attachment = undefined;
    if (!old) return;
    old.open = false;
    clearWrites(old);
    old.off?.();
    if (old.attaching) detach(old);
  };
  const retry = () => {
    if (!active || timer !== undefined) return;
    timer = setTimeout(
      () => {
        timer = undefined;
        open();
      },
      DELAYS[Math.min(attempt++, DELAYS.length - 1)],
    );
  };
  const failed = (a: Attachment) => {
    if (!current(a)) return;
    release();
    retry();
    emit("closed");
  };
  const notice = (a: Attachment, value: TransportNotice) => {
    if (!current(a) || value.attachmentId !== a.id) return;
    if (!/^\d+$/.test(value.revision) || !["open", "closed"].includes(value.state)) {
      failed(a);
      return;
    }
    const revision = BigInt(value.revision);
    if (revision <= a.revision) return;
    a.revision = revision;
    const isOpen = value.state === "open";
    if (a.open === isOpen) return;
    a.open = isOpen;
    if (!isOpen) clearWrites(a);
    emit(value.state);
  };
  const pump = async (a: Attachment) => {
    if (a.writing) return;
    a.writing = true;
    try {
      while (current(a) && a.open && a.queue.length) {
        const frame = a.queue.shift();
        if (!frame) break;
        const epoch = a.epoch;
        try {
          await invoke("rpc_send", frame, { headers: { "x-devbox-attachment": a.id } });
        } catch {
          if (epoch === a.epoch) failed(a);
          return;
        }
        if (epoch === a.epoch) a.bytes -= frame.byteLength;
      }
    } finally {
      a.writing = false;
      if (current(a) && a.open && a.queue.length) void pump(a);
    }
  };
  const open = () => {
    if (!active || attachment) return;
    const a: Attachment = {
      id: crypto.randomUUID(),
      attaching: false,
      open: false,
      revision: -1n,
      queue: [],
      bytes: 0,
      writing: false,
      epoch: 0,
    };
    attachment = a;
    emit("connecting");
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = (buffer) => {
      if (!current(a) || !a.open) return;
      let frame: Uint8Array;
      try {
        if (!(buffer instanceof ArrayBuffer)) throw new Error("Binary frame required");
        frame = new Uint8Array(buffer);
        decodeFrame(frame);
      } catch {
        failed(a);
        return;
      }
      attempt = 0;
      for (const cb of frames) cb(frame);
    };
    void (async () => {
      try {
        const off = await listen<TransportNotice>("devbox://transport", (event) => notice(a, event.payload));
        if (!current(a)) {
          off();
          return;
        }
        a.off = off;
        a.attaching = true;
        const snapshot = await invoke<TransportNotice>("rpc_attach", { onFrame: channel, attachmentId: a.id });
        if (!current(a)) {
          detach(a);
          return;
        }
        notice(a, snapshot);
      } catch {
        if (!current(a)) {
          if (a.attaching) detach(a);
          return;
        }
        failed(a);
      }
    })();
  };
  return {
    connect() {
      if (!active) {
        active = true;
        open();
      }
    },
    reconnect() {
      if (!active) return;
      release();
      retry();
      emit("closed");
    },
    close() {
      active = false;
      attempt = 0;
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
      release();
      emit("closed");
    },
    send(frame) {
      const a = attachment;
      if (!active || !a?.open) throw new Error("Tauri transport is not open");
      decodeFrame(frame);
      if (a.queue.length >= 64 || a.bytes + frame.byteLength > 32 * 1024 * 1024) {
        failed(a);
        throw new Error("Tauri transport queue is full");
      }
      a.queue.push(frame.slice());
      a.bytes += frame.byteLength;
      void pump(a);
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
