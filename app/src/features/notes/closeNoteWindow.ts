import type { CloseRequestedEvent } from "@tauri-apps/api/window";
import { useDocuments } from "../documents/store";

/** The native handler already blocks early requests before this listener is installed. */
export function noteCloseHandler(
  destroy: () => Promise<void>,
  blocked: () => void,
  prepare = () => useDocuments.getState().prepareWindowClose(),
) {
  let pending = false;
  return async (event: Pick<CloseRequestedEvent, "preventDefault">) => {
    event.preventDefault();
    if (pending) return;
    pending = true;
    try {
      if (!(await prepare())) {
        blocked();
        return;
      }
      await destroy();
    } catch {
      useDocuments.getState().cancelWindowClose();
      blocked();
    } finally {
      pending = false;
    }
  };
}
