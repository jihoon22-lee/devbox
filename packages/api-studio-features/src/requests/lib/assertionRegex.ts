const ERROR = "정규식 검증이 제한 시간을 넘었거나 사용할 수 없습니다. 표현식을 단순하게 바꿔 주세요.";
/** Untrusted collection expressions must never backtrack on the renderer thread. */
export function matchAssertionRegex(expression: string, text: string, signal?: AbortSignal): Promise<boolean> {
  if (signal?.aborted) return Promise.reject(new Error("검증을 취소했습니다"));
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./assertionRegex.worker.ts", import.meta.url), { type: "module" });
    let settled = false;
    const finish = (matched?: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      worker.terminate();
      if (typeof matched === "boolean") resolve(matched);
      else reject(new Error(ERROR));
    };
    const abort = () => finish();
    const timer = setTimeout(() => finish(), 500);
    signal?.addEventListener("abort", abort, { once: true });
    worker.onmessage = (event) => finish(typeof event.data?.matched === "boolean" ? event.data.matched : undefined);
    worker.onerror = () => finish();
    worker.onmessageerror = () => finish();
    try {
      worker.postMessage({ expression, text });
    } catch {
      finish();
    }
  });
}
