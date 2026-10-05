import { describe, expect, it, vi } from "vitest";
import { createOrderedInput } from "./orderedInput";
import { MAX_TERMINAL_PASTE_CHARACTERS } from "./terminalUx";

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

describe("ordered terminal input", () => {
  it("drops queued Enter after an ambiguous failed write and never retries", async () => {
    const failure = vi.fn();
    const input = createOrderedInput(failure);
    const first = deferred();
    const send = vi.fn(() => first.promise);
    const enter = vi.fn().mockResolvedValue(undefined);
    input.enqueue("command", send);
    input.enqueue("\r", enter);
    first.reject(new Error("unknown delivery"));
    await Promise.resolve();
    input.enqueue("later", enter);
    expect(send).toHaveBeenCalledTimes(1);
    expect(enter).not.toHaveBeenCalled();
    expect(failure).toHaveBeenCalledTimes(1);
  });

  it("discards writes waiting at pane disposal and ignores late completion", async () => {
    const failure = vi.fn();
    const input = createOrderedInput(failure);
    const first = deferred();
    const later = vi.fn().mockResolvedValue(undefined);
    input.enqueue("first", () => first.promise);
    input.enqueue("\r", later);
    input.dispose();
    first.reject(new Error("closed"));
    await Promise.resolve();
    expect(later).not.toHaveBeenCalled();
    expect(failure).not.toHaveBeenCalled();
  });

  it("allows a maximum paste and following Enter but caps retained characters", async () => {
    const failure = vi.fn();
    const input = createOrderedInput(failure);
    const first = deferred();
    const later = vi.fn().mockResolvedValue(undefined);
    input.enqueue(`\x1b[200~${"x".repeat(MAX_TERMINAL_PASTE_CHARACTERS)}\x1b[201~`, () => first.promise);
    input.enqueue("\r", later);
    expect(failure).not.toHaveBeenCalled();
    input.enqueue("x".repeat(MAX_TERMINAL_PASTE_CHARACTERS), later);
    expect(failure).toHaveBeenCalledTimes(1);
    first.resolve();
    await Promise.resolve();
    expect(later).not.toHaveBeenCalled();
  });

  it("bounds tiny pending keystrokes while an ACK stalls", async () => {
    const failure = vi.fn();
    const input = createOrderedInput(failure);
    const first = deferred();
    const later = vi.fn().mockResolvedValue(undefined);
    input.enqueue("x", () => first.promise);
    for (let i = 0; i < 255; i++) input.enqueue("x", later);
    expect(failure).not.toHaveBeenCalled();
    input.enqueue("\r", later);
    expect(failure).toHaveBeenCalledTimes(1);
    first.resolve();
    await Promise.resolve();
    expect(later).not.toHaveBeenCalled();
  });

  it("does not block another pane behind a stalled ACK", () => {
    const first = deferred();
    createOrderedInput(vi.fn()).enqueue("first", () => first.promise);
    const other = vi.fn().mockResolvedValue(undefined);
    createOrderedInput(vi.fn()).enqueue("other", other);
    expect(other).toHaveBeenCalledOnce();
    first.resolve();
  });
});
