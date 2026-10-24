// @vitest-environment node
import { Worker as Thread } from "node:worker_threads";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { runRegex } from "./regex";

const owned: Thread[] = [];
let terminated = 0;
class WorkerBridge {
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onmessageerror: (() => void) | null = null;
  thread: Thread;
  constructor() {
    this.thread = new Thread(new URL("./regex.worker.fixture.mjs", import.meta.url));
    owned.push(this.thread);
    this.thread.on("message", (data) => this.onmessage?.({ data }));
    this.thread.on("error", () => this.onerror?.());
    this.thread.on("messageerror", () => this.onmessageerror?.());
  }
  postMessage(value: unknown) {
    this.thread.postMessage(value);
  }
  terminate() {
    terminated++;
    void this.thread.terminate();
  }
}
beforeEach(() => {
  terminated = 0;
  vi.stubGlobal("Worker", WorkerBridge);
});
afterEach(async () => {
  await Promise.all(owned.splice(0).map((thread) => thread.terminate()));
  vi.unstubAllGlobals();
});
it("regex_worker_stops_catastrophic_patterns", async () => {
  let ticks = 0;
  const timer = setInterval(() => ticks++, 20);
  try {
    await expect(runRegex("(a+)+$", "u", "a".repeat(50) + "!")).rejects.toThrow("시간 초과");
    expect(terminated).toBe(1);
    expect(ticks).toBeGreaterThan(0);
  } finally {
    clearInterval(timer);
  }
}, 10000);
it("a real worker returns UTF16 ranges and is terminated after success", async () => {
  expect(await runRegex("😀", "u", "a😀b")).toEqual({ matches: [{ startUtf16: 1, endUtf16: 3 }], truncated: false });
  expect(terminated).toBe(1);
});
it("abort terminates the running worker and a pre-aborted input creates none", async () => {
  const controller = new AbortController();
  const pending = runRegex("(a+)+$", "", "a".repeat(50) + "!", controller.signal);
  controller.abort();
  await expect(pending).rejects.toThrow("취소");
  expect(terminated).toBe(1);
  await expect(runRegex("a", "", "a", controller.signal)).rejects.toThrow("취소");
  expect(terminated).toBe(1);
});
