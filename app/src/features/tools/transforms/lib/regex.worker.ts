import { regexMatches } from "./regexEngine";

self.onmessage = (event: MessageEvent<{ pattern: string; flags: string; text: string; limit: number }>) => {
  try {
    self.postMessage({ result: regexMatches(event.data.pattern, event.data.flags, event.data.text, event.data.limit) });
  } catch {
    self.postMessage({ error: true });
  }
};
