import { useOperation } from "@devbox/hooks";
import { useCallback, useRef } from "react";

/** Form owners still receive native failures; one mutation owns the task UI. */
export function useTaskOperations() {
  const { busy, run } = useOperation();
  const pending = useRef(false);
  const perform = useCallback(
    async <T>(task: () => Promise<T>): Promise<T | undefined> => {
      if (pending.current) return undefined;
      pending.current = true;
      const outcome: { failed: boolean; error?: unknown } = { failed: false };
      try {
        const value = await run(async () => {
          try {
            return await task();
          } catch (error) {
            outcome.failed = true;
            outcome.error = error;
            throw error;
          }
        });
        if (outcome.failed) throw outcome.error;
        return value;
      } finally {
        pending.current = false;
      }
    },
    [run],
  );
  return { busy, run: perform };
}
