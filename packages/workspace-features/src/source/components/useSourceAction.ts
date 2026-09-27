import { useCallback, useEffect, useRef } from "react";
import { useOperation } from "@devbox/hooks";
import { WorkspaceOperationError } from "../../transport";
import { repoLocalCancel, type RepoEntry } from "../api";
type Action<T> = (operationId: string, signal: AbortSignal) => Promise<T>;
/** Serialize panel actions and invalidate late responses/undo on repository changes. */
export function useSourceAction(repo: RepoEntry, onBusyChange?: (busy: boolean) => void) {
  const { busy, issue, run, cancel } = useOperation();
  const active = useRef<symbol | null>(null);
  const epoch = useRef(0);
  useEffect(() => {
    if (!repo.canonicalKey || !repo.path) return;
    epoch.current += 1;
    return () => {
      epoch.current += 1;
      active.current = null;
      cancel();
    };
  }, [repo.canonicalKey, repo.path, cancel]);
  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);
  useEffect(() => () => onBusyChange?.(false), [onBusyChange]);
  const execute = useCallback(
    async <T>(action: Action<T>): Promise<T | undefined> => {
      if (active.current) return undefined;
      const token = Symbol();
      active.current = token;
      try {
        return await run(async (signal) => {
          const id = crypto.randomUUID();
          const abort = () => {
            void repoLocalCancel(id).catch(() => undefined);
          };
          signal.addEventListener("abort", abort, { once: true });
          try {
            return await action(id, signal);
          } catch (cause) {
            throw cause instanceof WorkspaceOperationError
              ? cause
              : new Error("Git 작업을 완료하지 못했습니다. 상태를 다시 확인해 주세요.");
          } finally {
            signal.removeEventListener("abort", abort);
          }
        });
      } finally {
        if (active.current === token) active.current = null;
      }
    },
    [run],
  );
  const scopedUndo = useCallback(
    (action: Action<void>) => {
      const owner = epoch.current;
      return async () => {
        if (owner !== epoch.current) throw new Error("선택한 저장소가 바뀌어 되돌리지 않았습니다.");
        const completed = await execute(async (id, signal) => {
          await action(id, signal);
          return true;
        });
        if (!completed) throw new Error("되돌리기를 완료하지 못했습니다. 저장소 상태를 확인해 주세요.");
      };
    },
    [execute],
  );
  return { busy, issue, execute, scopedUndo };
}
