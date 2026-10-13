// @vitest-environment node
import { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, it } from "vitest";
import { preserveInitialHistory } from "./initial-history";

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});
function fixture(rows = 9) {
  const terminal = new Terminal({ cols: 80, rows, scrollback: 1000, allowProposedApi: true });
  const history = preserveInitialHistory(terminal);
  cleanups.push(() => {
    history.dispose();
    terminal.dispose();
  });
  const write = (text: string) =>
    new Promise<void>((resolve) => {
      terminal.write(text, () => {
        history.written();
        resolve();
      });
    });
  const lines = () =>
    Array.from({ length: terminal.buffer.active.length }, (_, i) =>
      terminal.buffer.active.getLine(i)?.translateToString(true),
    );
  return { terminal, history, write, lines };
}
const prefix = (label = "history") =>
  Array.from({ length: 30 }, (_, i) => `${label}-${String(i + 1).padStart(4, "0")}\r\n`).join("");
function expectHistory(lines: Array<string | undefined>, label = "history") {
  for (let i = 1; i <= 30; i++) {
    expect(lines.filter((line) => line === `${label}-${String(i).padStart(4, "0")}`)).toHaveLength(1);
  }
}

describe("actual xterm initial history replay", () => {
  it("preserves history when the renderer grows beyond the daemon's initial grid", async () => {
    const f = fixture(9);
    // The daemon prepared a five-row grid; four padding rows no longer suffice.
    await f.write(`${prefix()}${"\r\n".repeat(4)}\x1b[H\x1b[2Jlive`);
    expectHistory(f.lines());
    expect(f.terminal.buffer.active.baseY).toBe(30);
  });

  it("keeps its pending first erase across writes, resize and a split CSI", async () => {
    const f = fixture(5);
    await f.write(prefix());
    f.terminal.resize(80, 9);
    await f.write("\x1b[H\x1b[");
    await f.write("2Jlive");
    expectHistory(f.lines());
    expect(f.terminal.options.scrollOnEraseInDisplay).toBe(false);
  });

  it("leaves later redraws as ordinary erases, including two in one write", async () => {
    const f = fixture(5);
    await f.write(`${prefix()}${"\r\n".repeat(4)}\x1b[H\x1b[2Jfirst-live\x1b[H\x1b[2Jsecond-live`);
    expectHistory(f.lines());
    expect(f.lines()).not.toContain("first-live");
    expect(f.terminal.buffer.active.baseY).toBe(30);
    await f.write("\x1b[H\x1b[2Jthird-live");
    expect(f.lines()).not.toContain("second-live");
    expect(f.terminal.buffer.active.baseY).toBe(30);
  });

  it("rearms for a reset connection without retaining the previous capture", async () => {
    const f = fixture(9);
    await f.write(`${prefix("old")}\x1b[H\x1b[2Jold-live`);
    f.terminal.reset();
    f.history.reset();
    await f.write(`${prefix("new")}\x1b[H\x1b[2Jnew-live`);
    expectHistory(f.lines(), "new");
    expect(f.lines().some((line) => line?.includes("old"))).toBe(false);
  });
});
