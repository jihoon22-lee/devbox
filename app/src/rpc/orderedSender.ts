const MAX_PENDING = 256;
const MAX_PENDING_BYTES = 8 * 1024 * 1024;
interface Write {
  bytes: number;
  send: () => Promise<void>;
}

/** Invocation completion is ambiguous on failure: discard all later frames, never retry. */
export function createOrderedSender(onFailure: () => void) {
  const pending: Write[] = [];
  let active = false;
  let stopped = false;
  let count = 0;
  let bytes = 0;
  const dispose = () => {
    stopped = true;
    pending.length = 0;
    count = 0;
    bytes = 0;
  };
  const fail = () => {
    if (stopped) return;
    dispose();
    onFailure();
  };
  const drain = () => {
    if (active || stopped) return;
    const next = pending.shift();
    if (!next) return;
    active = true;
    void (async () => {
      try {
        await next.send();
      } catch {
        fail();
        return;
      }
      if (stopped) return;
      count--;
      bytes -= next.bytes;
      active = false;
      drain();
    })();
  };
  return {
    /** False means the frame was dropped; capacity includes the active invocation. */
    enqueue(frame: Uint8Array, send: () => Promise<void>): boolean {
      if (stopped) return false;
      if (count >= MAX_PENDING || bytes + frame.byteLength > MAX_PENDING_BYTES) {
        fail();
        return false;
      }
      pending.push({ bytes: frame.byteLength, send });
      count++;
      bytes += frame.byteLength;
      drain();
      return true;
    },
    dispose,
  };
}
