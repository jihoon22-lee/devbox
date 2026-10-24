import { useEffect, useRef, useState } from "react";

export function useToolAction(value: string, fixedError?: string, onBusyChange?: (busy: boolean) => void) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const revision = useRef(0);
  const mounted = useRef(true);
  const currentValue = useRef(value);
  currentValue.current = value;
  const busyChanged = useRef(onBusyChange);
  busyChanged.current = onBusyChange;
  // biome-ignore lint/correctness/useExhaustiveDependencies: changing the value invalidates pending clipboard/output actions.
  useEffect(() => {
    revision.current += 1;
    pending.current = false;
    setBusy(false);
    setError(null);
    busyChanged.current?.(false);
  }, [value]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      revision.current += 1;
    };
  }, []);
  const run = async (action: () => Promise<unknown> | void) => {
    if (pending.current) return;
    const token = ++revision.current;
    const snapshot = value;
    pending.current = true;
    setBusy(true);
    setError(null);
    busyChanged.current?.(true);
    try {
      await Promise.resolve();
      if (mounted.current && revision.current === token) await action();
    } catch (caught) {
      if (mounted.current && revision.current === token && currentValue.current === snapshot)
        setError(
          fixedError ?? `출력 작업을 완료하지 못했습니다: ${caught instanceof Error ? caught.message : String(caught)}`,
        );
    } finally {
      if (mounted.current && revision.current === token) {
        pending.current = false;
        setBusy(false);
        busyChanged.current?.(false);
      }
    }
  };
  return { error, busy, run };
}

export function useAsyncTransform(
  input: string,
  run: (input: string, signal?: AbortSignal) => Promise<{ output: string; error?: string }>,
  options: { clearOutputOnStart?: boolean } = {},
) {
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const seq = useRef(0);
  const clearOutputOnStart = options.clearOutputOnStart ?? false;
  useEffect(() => {
    const current = ++seq.current;
    const controller = new AbortController();
    if (clearOutputOnStart) {
      setOutput("");
      setError(null);
    }
    setRunning(true);
    let request: Promise<{ output: string; error?: string }>;
    try {
      request =
        input.length > 1024 * 1024 || new TextEncoder().encode(input).length > 1024 * 1024
          ? Promise.resolve({ output: "", error: "입력은 1MiB까지 처리할 수 있습니다." })
          : Promise.resolve(run(input, controller.signal));
    } catch (error) {
      request = Promise.reject(error);
    }
    request
      .then((res) => {
        if (!controller.signal.aborted && seq.current === current) {
          setOutput(res.output);
          setError(res.error ?? null);
        }
      })
      .catch((error) => {
        if (!controller.signal.aborted && seq.current === current)
          setError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!controller.signal.aborted && seq.current === current) setRunning(false);
      });
    return () => {
      controller.abort();
      if (seq.current === current) seq.current += 1;
    };
  }, [input, run, clearOutputOnStart]);
  return { output, error, running };
}
