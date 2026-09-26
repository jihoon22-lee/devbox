import { Channel } from "@tauri-apps/api/core";
import { subscribeOutput, terminalCall } from "../api-transport";
import { validateBatch, type OutputBatch } from "./terminalReplay";

/** One native batch in flight; acknowledgements follow the completed xterm write. */
export function streamTerminalOutput(
  sessionId: string,
  write: (text: string, truncated: boolean) => Promise<void>,
  closed: () => void,
  failed: () => void,
): () => void {
  let disposed = false;
  let subscriptionId: string | undefined;
  let released = false;
  let cursor = 0;
  let pending: OutputBatch | undefined;
  let phase: "idle" | "writing" | "acking" = "idle";
  const channel = new Channel<OutputBatch>();
  const release = () => {
    if (!subscriptionId || released) return;
    released = true;
    void terminalCall("unsubscribe_terminal_output", { subscriptionId }).catch(() => {});
  };
  const dispose = () => {
    disposed = true;
    pending = undefined;
    channel.onmessage = () => {};
    release();
  };
  const fail = () => {
    if (disposed) return;
    dispose();
    failed();
  };
  const drain = async () => {
    if (disposed || !subscriptionId || phase !== "idle") return;
    try {
      while (pending && !disposed) {
        const batch = pending;
        pending = undefined;
        phase = "writing";
        const text = validateBatch(batch, cursor);
        if (batch.frames.length || batch.truncated) await write(text, batch.truncated);
        if (disposed) return;
        cursor = batch.cursor;
        if (batch.closed) {
          dispose();
          closed();
          return;
        }
        // Native may send its next message before the ack command's reply arrives.
        // Allow one waiting batch only during this phase, never during the write.
        phase = "acking";
        await terminalCall("ack_terminal_output", { subscriptionId, cursor });
        phase = "idle";
      }
    } catch {
      fail();
    }
  };
  channel.onmessage = (batch) => {
    if (disposed) return;
    try {
      validateBatch(batch, cursor);
      if (pending || phase === "writing") throw new Error("output stream overrun");
      pending = batch;
      void drain();
    } catch {
      fail();
    }
  };
  try {
    void subscribeOutput(sessionId, 0, channel)
      .then((subscription) => {
        if (!subscription || typeof subscription.subscriptionId !== "string" || !subscription.subscriptionId) {
          fail();
          return;
        }
        subscriptionId = subscription.subscriptionId;
        if (disposed) release();
        else void drain();
      })
      .catch(fail);
  } catch {
    fail();
  }
  return dispose;
}
