import { afterEach, expect, it, vi } from "vitest";
import { matchNames } from "./regex";
const workers: Array<{ terminate: ReturnType<typeof vi.fn>; postMessage: ReturnType<typeof vi.fn>; onmessage?: (event: { data: unknown }) => void }> = [];
function installWorker() {
  vi.stubGlobal("Worker", class {
    terminate = vi.fn();
    postMessage = vi.fn();
    constructor() { workers.push(this); }
  });
}
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); workers.length = 0; });
it("terminates stuck regex execution and rejects without blocking another query", async () => {
  installWorker(); vi.useFakeTimers();
  const task = matchNames("(a+)+$", ["a".repeat(100) + "!"], new AbortController().signal);
  const rejected = expect(task).rejects.toThrow("제한 시간");
  await vi.advanceTimersByTimeAsync(500);
  await rejected;
  expect(workers[0].terminate).toHaveBeenCalledOnce();
  const next = matchNames("next", ["next.md"], new AbortController().signal);
  workers[1].onmessage?.({ data: { indices: [0] } });
  expect(await next).toEqual(new Set([0]));
});
it("cancellation terminates the worker and a stale reply cannot resolve the result", async () => {
  installWorker();
  const controller = new AbortController();
  const task = matchNames("note", ["note.md"], controller.signal);
  const rejected = expect(task).rejects.toThrow();
  controller.abort();
  workers[0].onmessage?.({ data: { indices: [0] } });
  await rejected;
  expect(workers[0].terminate).toHaveBeenCalled();
});
