import { create } from "zustand";
import type { CloseChoice, Doc } from "./machine";
import type { ClosePrompt } from "./store";

interface Request {
  docs: Doc[];
  answer(choice?: CloseChoice): void;
}
export const useClosePrompt = create<{ request?: Request }>(() => ({}));
export const chooseDocumentClose: ClosePrompt = (docs, signal) =>
  new Promise((resolve) => {
    if (signal.aborted) return resolve(undefined);
    if (useClosePrompt.getState().request) return resolve(undefined);
    let done = false;
    const answer = (choice?: CloseChoice) => {
      if (done) return;
      done = true;
      signal.removeEventListener("abort", abort);
      useClosePrompt.setState({ request: undefined });
      resolve(choice ? Object.fromEntries(docs.map((doc) => [doc.id, choice])) : undefined);
    };
    const abort = () => answer();
    signal.addEventListener("abort", abort, { once: true });
    useClosePrompt.setState({ request: { docs, answer } });
  });
