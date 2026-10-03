import * as api from "../api";
import { SessionVariables } from "./runner";

export function createRequestSessionVariables(): SessionVariables {
  return new SessionVariables({
    reveal: (reference) => api.revealCapture(reference),
    discard: (references) => api.discardCaptures(references),
    restore: (references) => api.restoreCaptures(references),
  });
}
