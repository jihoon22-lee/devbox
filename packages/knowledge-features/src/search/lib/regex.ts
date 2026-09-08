const ERROR = "정규식 검색이 제한 시간을 넘었거나 사용할 수 없습니다. 표현식을 단순하게 바꿔 주세요.";
/** Preserve JavaScript regex syntax while bounding pathological backtracking. */
export function matchNames(expression: string, names: string[], signal: AbortSignal): Promise<Set<number>> {
  if (signal.aborted) return Promise.reject(new Error("검색을 취소했습니다."));
  if (expression.length > 4096 || names.length > 2000) return Promise.reject(new Error(ERROR));
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./regex.worker.ts", import.meta.url), { type: "module" });
    let settled = false;
    const finish = (indices?: number[]) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      worker.terminate();
      if (indices) resolve(new Set(indices)); else reject(new Error(ERROR));
    };
    const abort = () => finish();
    const timer = setTimeout(() => finish(), 500);
    signal.addEventListener("abort", abort, { once: true });
    worker.onmessage = event => {
      const indices: unknown = event.data?.indices;
      if (Array.isArray(indices) && indices.length <= names.length && indices.every(index => Number.isInteger(index) && index >= 0 && index < names.length)) finish(indices);
      else finish();
    };
    worker.onerror = () => finish();
    worker.postMessage({ expression, names });
  });
}
