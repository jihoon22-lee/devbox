import type { RegexResult } from "./regexEngine";

export type { RegexRange, RegexResult } from "./regexEngine";
export { segments } from "./regexEngine";
export function runRegex(pattern: string, flags: string, text: string, signal?: AbortSignal): Promise<RegexResult> {
  if (signal?.aborted) return Promise.reject(new Error("정규식 실행을 취소했습니다."));
  return new Promise((resolve, reject) => {
    let worker: Worker;
    try {
      worker = new Worker(new URL("./regex.worker.ts", import.meta.url), { type: "module" });
    } catch {
      reject(new Error("정규식 실행 기능을 사용할 수 없습니다."));
      return;
    }
    let settled = false;
    const finish = (result?: RegexResult, error = "정규식을 처리하지 못했습니다. 입력과 옵션을 확인해 주세요.") => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      worker.terminate();
      if (result) resolve(result);
      else reject(new Error(error));
    };
    const abort = () => finish(undefined, "정규식 실행을 취소했습니다.");
    const timer = setTimeout(() => finish(undefined, "시간 초과: 정규식을 단순하게 바꿔 주세요."), 1000);
    signal?.addEventListener("abort", abort, { once: true });
    worker.onmessage = (event: MessageEvent<{ result?: RegexResult }>) => finish(event.data?.result);
    worker.onerror = () => finish();
    worker.onmessageerror = () => finish();
    try {
      worker.postMessage({ pattern, flags, text, limit: 10_000 });
    } catch {
      finish();
    }
  });
}
