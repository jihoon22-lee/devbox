import { ansiToSpans, followState, LogDecoder, retainLogLines } from "./model";

test("renders ANSI 16, 256, truecolor, bold and underline and resets styles", () => {
  const spans = ansiToSpans("\x1b[31;1;4mred\x1b[0m plain \x1b[38;5;196m256\x1b[48;2;1;2;3mRGB");
  expect(spans[0]).toMatchObject({
    text: "red",
    style: { color: "#cd3131", fontWeight: "bold", textDecoration: "underline" },
  });
  expect(spans[1]).toEqual({ text: " plain ", style: {} });
  expect(spans[2]).toMatchObject({ text: "256", style: { color: "rgb(255, 0, 0)" } });
  expect(spans[3]).toMatchObject({ text: "RGB", style: { backgroundColor: "rgb(1, 2, 3)" } });
});

test("strips cursor/OSC/DCS/C1 and hidden controls without creating HTML", () => {
  const spans = ansiToSpans("\x1b]8;;https://bad\x07safe\x1b]8;;\x1b\\\x1b[2J\x1bPsecret\x1b\\\x9b2J\x00<script>");
  expect(spans.map((s) => s.text).join("")).toBe("safe<script>");
});

test("follow pauses on upward scrolling and new lines never override an explicit pause", () => {
  expect(followState(true, { type: "userScrolled", atBottom: false })).toBe(false);
  expect(followState(false, { type: "newLines" })).toBe(false);
  expect(followState(false, { type: "follow" })).toBe(true);
  expect(followState(false, { type: "userScrolled", atBottom: true })).toBe(true);
});

test("decodes NDJSON across arbitrary byte and UTF-8 boundaries and bounds incomplete records", () => {
  const line = { src: { kind: "run", runId: "r" }, ts: "2026-10-20T00:00:00Z", stream: "stdout", text: "한글" };
  const bytes = new TextEncoder().encode(`${JSON.stringify(line)}\n`);
  const decoder = new LogDecoder();
  const decoded = Array.from(bytes).flatMap((byte) => decoder.push(new Uint8Array([byte])));
  expect(decoded).toEqual([line]);
  expect(() => decoder.push(new TextEncoder().encode("x".repeat(20 * 1024 * 1024 + 1)))).toThrow();
  expect(() => new LogDecoder().push(new TextEncoder().encode('{"text":"invalid"}\n'))).toThrow();
});

test("bounds retained rows and total text while keeping the newest records", () => {
  const rows = Array.from({ length: 10_010 }, (_, i) => ({
    src: { kind: "run" as const, runId: "r" },
    ts: String(i),
    stream: "stdout" as const,
    text: String(i),
  }));
  const retained = retainLogLines([], rows);
  expect(retained).toHaveLength(10_000);
  expect(retained[0]?.text).toBe("10");
  expect(
    retainLogLines(
      [],
      rows.slice(0, 100).map((row) => ({ ...row, text: "x".repeat(100_000) })),
    ).length,
  ).toBeLessThan(100);
});
