import { parentPort } from "node:worker_threads";
import { regexMatches } from "./regexEngine.ts";

parentPort.on("message", ({ pattern, flags, text, limit }) => {
  try {
    parentPort.postMessage({ result: regexMatches(pattern, flags, text, limit) });
  } catch {
    parentPort.postMessage({ error: true });
  }
});
