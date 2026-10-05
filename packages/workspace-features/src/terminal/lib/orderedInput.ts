import { MAX_TERMINAL_PASTE_CHARACTERS } from "./terminalUx";

const MAX_PENDING_CHUNKS = 256;
// Includes the in-flight paste and bracketed-paste delimiters, plus following keys.
const MAX_PENDING_CHARACTERS = MAX_TERMINAL_PASTE_CHARACTERS * 2;
interface InputWrite {
  characters: number;
  send: () => Promise<void>;
}

/** A pane owns one queue for its lifetime. Never retry an ambiguously delivered write. */
export function createOrderedInput(onFailure: () => void) {
  const pending: InputWrite[] = [];
  let active = false;
  let stopped = false;
  let characters = 0;
  const dispose = () => {
    stopped = true;
    pending.length = 0;
    characters = 0;
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
    // Invoke immediately for the first key; only subsequent writes wait for its ACK.
    void (async () => {
      try {
        await next.send();
      } catch {
        fail();
        return;
      }
      if (stopped) return;
      characters -= next.characters;
      active = false;
      drain();
    })();
  };
  return {
    enqueue(data: string, send: () => Promise<void>) {
      if (stopped) return;
      if (pending.length + Number(active) >= MAX_PENDING_CHUNKS || characters + data.length > MAX_PENDING_CHARACTERS) {
        fail();
        return;
      }
      characters += data.length;
      pending.push({ characters: data.length, send });
      drain();
    },
    dispose,
  };
}
