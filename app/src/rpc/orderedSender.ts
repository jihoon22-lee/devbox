import { decodeFrame, FRAME, jsonFrame } from "./frame";

const MAX_PENDING = 256;
const MAX_PENDING_BYTES = 8 * 1024 * 1024;
const STREAM_WINDOW = 256 * 1024;
interface Credit {
  stream: number;
  bytes: number;
}
interface Write {
  frame: Uint8Array;
  credit?: Credit;
  send: (frame: Uint8Array) => Promise<void>;
}
function creditOf(frame: Uint8Array): Credit | undefined {
  // Other control messages and every input/request frame are ordering barriers.
  if (frame[4] !== FRAME.control) return;
  try {
    const decoded = decodeFrame(frame);
    if (decoded.channel !== 0) return;
    const value = JSON.parse(new TextDecoder().decode(decoded.body));
    if (
      value?.type === "streamCredit" &&
      Object.keys(value).length === 3 &&
      Number.isInteger(value.stream) &&
      value.stream >= 0 &&
      value.stream <= 0xffffffff &&
      Number.isInteger(value.bytes) &&
      value.bytes > 0 &&
      value.bytes <= STREAM_WINDOW
    )
      return { stream: value.stream, bytes: value.bytes };
  } catch {
    /* Preserve opaque frames without changing their payload. */
  }
  return;
}

/** Invocation completion is ambiguous on failure: discard all later frames, never retry. */
export function createOrderedSender(onFailure: () => void) {
  const pending: Write[] = [];
  const credits = new Map<number, Write>();
  let active = false;
  let stopped = false;
  let count = 0;
  let bytes = 0;
  const dispose = () => {
    stopped = true;
    pending.length = 0;
    credits.clear();
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
    if (next.credit && credits.get(next.credit.stream) === next) credits.delete(next.credit.stream);
    active = true;
    void (async () => {
      try {
        await next.send(next.frame);
      } catch {
        fail();
        return;
      }
      if (stopped) return;
      count--;
      bytes -= next.frame.byteLength;
      active = false;
      drain();
    })();
  };
  return {
    /** False means the frame was dropped; capacity includes the active invocation. */
    enqueue(frame: Uint8Array, send: (frame: Uint8Array) => Promise<void>): boolean {
      if (stopped) return false;
      const credit = creditOf(frame);
      const previous = credit && credits.get(credit.stream);
      if (credit && previous?.credit && previous.credit.bytes + credit.bytes <= STREAM_WINDOW) {
        const total = previous.credit.bytes + credit.bytes;
        const merged = jsonFrame({ type: "streamCredit", stream: credit.stream, bytes: total }, FRAME.control);
        const nextBytes = bytes - previous.frame.byteLength + merged.byteLength;
        if (nextBytes > MAX_PENDING_BYTES) {
          fail();
          return false;
        }
        // Only a waiting invocation is mutable; an active native send is never changed.
        previous.frame = merged;
        previous.credit.bytes = total;
        bytes = nextBytes;
        return true;
      }
      if (count >= MAX_PENDING || bytes + frame.byteLength > MAX_PENDING_BYTES) {
        fail();
        return false;
      }
      const write = { frame, credit, send };
      if (credit) credits.set(credit.stream, write);
      else credits.clear();
      pending.push(write);
      count++;
      bytes += frame.byteLength;
      drain();
      return true;
    },
    dispose,
  };
}
