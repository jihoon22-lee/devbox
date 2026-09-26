import { useCallback, useEffect, useRef, useState } from "react";

export function messageOf(error: unknown): string {
  return error instanceof Error && error.message ? error.message : "작업을 완료하지 못했습니다. 다시 시도해 주세요.";
}

type Operation = {
  task: (signal: AbortSignal) => Promise<unknown>;
  controller: AbortController;
  promise: Promise<unknown>;
};

/** A repeated callback shares its pending action; a different action supersedes it. */
export function useOperation() {
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState<string | null>(null);
  const mounted = useRef(false);
  const current = useRef<Operation | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      current.current?.controller.abort();
      current.current = null;
    };
  }, []);

  const run = useCallback(<T>(task: (signal: AbortSignal) => Promise<T>): Promise<T | undefined> => {
    if (!mounted.current) return Promise.resolve(undefined);
    const previous = current.current;
    // The same function has the same result type at this call boundary.
    if (previous?.task === task) return previous.promise as Promise<T | undefined>;
    previous?.controller.abort();
    const operation: Operation = { task, controller: new AbortController(), promise: Promise.resolve(undefined) };
    current.current = operation;
    setBusy(true);
    setIssue(null);
    const promise = (async () => {
      try {
        const value = await task(operation.controller.signal);
        return mounted.current && current.current === operation && !operation.controller.signal.aborted
          ? value
          : undefined;
      } catch (error) {
        if (mounted.current && current.current === operation && !operation.controller.signal.aborted)
          setIssue(messageOf(error));
        return undefined;
      } finally {
        if (mounted.current && current.current === operation) {
          current.current = null;
          setBusy(false);
        }
      }
    })();
    operation.promise = promise;
    return promise;
  }, []);
  const cancel = useCallback(() => {
    current.current?.controller.abort();
    current.current = null;
    if (mounted.current) setBusy(false);
  }, []);
  const clearIssue = useCallback(() => {
    if (mounted.current) setIssue(null);
  }, []);
  return { busy, issue, run, cancel, clearIssue };
}
