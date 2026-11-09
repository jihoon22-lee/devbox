import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ansiToSpans, ansiToText, followState, LogDecoder, retainLogLines } from "./model";

// Read from disk: Vitest turns every CSS import, `?raw` included, into an empty string.
const tokensCss = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../ui/tokens.css"), "utf8");

test("renders ANSI 16, 256, truecolor, bold and underline and resets styles", () => {
  const spans = ansiToSpans("\x1b[31;1;4mred\x1b[0m plain \x1b[38;5;196m256\x1b[48;2;1;2;3mRGB");
  expect(spans[0]).toMatchObject({
    text: "red",
    style: { color: "var(--log-ansi-red)", fontWeight: "bold", textDecoration: "underline" },
  });
  expect(spans[1]).toEqual({ text: " plain ", style: {} });
  expect(spans[2]).toMatchObject({ text: "256", style: { color: "rgb(255, 0, 0)" } });
  expect(spans[3]).toMatchObject({ text: "RGB", style: { backgroundColor: "rgb(1, 2, 3)" } });
  expect(ansiToSpans("\x1b[33mW")[0]!.style.color).toBe("var(--log-ansi-yellow)");
});

test("a basic foreground on an explicit background keeps its classic pair, decided by the span's final style", () => {
  // White on red is 4.09:1 as a classic pair, but 1.04:1 with the light theme's white text (#6f7077).
  const badge = { color: "#e5e5e5", backgroundColor: "var(--log-ansi-bg-red)" };
  expect(ansiToSpans("\x1b[37;41mX")).toStrictEqual([{ text: "X", style: badge }]);
  expect(ansiToSpans("\x1b[41;37mX")).toStrictEqual([{ text: "X", style: badge }]);
  expect(ansiToSpans("\x1b[41m\x1b[37mX")).toStrictEqual([{ text: "X", style: badge }]);
  expect(ansiToSpans("\x1b[37;41mA\x1b[49mB")).toStrictEqual([
    { text: "A", style: badge },
    { text: "B", style: { color: "var(--log-ansi-white)" } },
  ]);
  expect(ansiToSpans("\x1b[37mA\x1b[41mB\x1b[0mC")).toStrictEqual([
    { text: "A", style: { color: "var(--log-ansi-white)" } },
    { text: "B", style: badge },
    { text: "C", style: {} },
  ]);
  // Bright pairs, and 256-colour or truecolor backgrounds, are explicit backgrounds too.
  expect(ansiToSpans("\x1b[97;101mX")[0]!.style).toStrictEqual({
    color: "#ffffff",
    backgroundColor: "var(--log-ansi-bg-bright-red)",
  });
  expect(ansiToSpans("\x1b[31;48;2;1;2;3mX")[0]!.style).toStrictEqual({
    color: "#cd3131",
    backgroundColor: "rgb(1, 2, 3)",
  });
  expect(ansiToSpans("\x1b[30;48;5;11mX")[0]!.style).toStrictEqual({ color: "#000000", backgroundColor: "#f5f543" });
});

test("256-colour and truecolor foregrounds stay as they are, indexes below 16 included", () => {
  expect(ansiToSpans("\x1b[38;5;3mX")[0]!.style).toStrictEqual({ color: "#e5e510" });
  expect(ansiToSpans("\x1b[38;2;1;2;3mX")[0]!.style).toStrictEqual({ color: "rgb(1, 2, 3)" });
});

/** The custom properties on <html> in one theme: `:root` rules apply to both themes, later rules win. */
function themeTokens(theme: "light" | "dark") {
  const values = new Map<string, string>();
  const css = tokensCss.replace(/\/\*[\s\S]*?\*\//g, "");
  for (const [, selectors, body] of css.matchAll(/([^{}]*)\{([^{}]*)\}/g)) {
    const list = selectors!.split(",").map((selector) => selector.trim());
    if (!list.includes(":root") && !list.includes(`[data-theme="${theme}"]`)) continue;
    for (const [, name, value] of body!.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) values.set(name!, value!.trim());
  }
  if (!values.size) throw new Error("tokens.css has no custom properties");
  return values;
}
/** A colour value as #rrggbb, following var() references; anything else fails the test. */
function hex(values: Map<string, string>, value: string): string {
  const reference = /^var\((--[\w-]+)\)$/.exec(value);
  if (reference) {
    const next = values.get(reference[1]!);
    if (next === undefined) throw new Error(`${reference[1]} is not defined`);
    return hex(values, next);
  }
  if (!/^#[\da-f]{6}$/i.test(value)) throw new Error(`not a #rrggbb colour: ${value}`);
  return value.toLowerCase();
}
/** WCAG 2.x contrast ratio of two #rrggbb colours. */
function contrast(a: string, b: string) {
  const luminance = (color: string) => {
    const [red, green, blue] = [1, 3, 5].map((at) => {
      const c = Number.parseInt(color.slice(at, at + 2), 16) / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * red! + 0.7152 * green! + 0.0722 * blue!;
  };
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (high! + 0.05) / (low! + 0.05);
}
const BASIC = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
/** SGR 30–37 then 90–97, with the token each one names. */
const ANSI16 = [
  ...BASIC.map((name, index) => ({ code: 30 + index, name })),
  ...BASIC.map((name, index) => ({ code: 90 + index, name: `bright-${name}` })),
];

test("16 log colours meet 4.5:1 on --bg-1 in both themes", () => {
  const unreadable: string[] = [];
  for (const theme of ["light", "dark"] as const) {
    const values = themeTokens(theme);
    const background = hex(values, "var(--bg-1)");
    for (const { code } of ANSI16) {
      // The colour the log renders, looked up in the theme's tokens.
      const color = String(ansiToSpans(`\x1b[${code}mW`)[0]!.style.color);
      const value = hex(values, color);
      const ratio = contrast(value, background);
      if (ratio < 4.5)
        unreadable.push(`${theme} SGR ${code}: ${color} = ${value} on ${background}, ${ratio.toFixed(2)}`);
    }
  }
  expect(unreadable).toEqual([]);
  for (const { code, name } of ANSI16)
    expect(ansiToSpans(`\x1b[${code}mW`)[0]!.style.color, `SGR ${code}`).toBe(`var(--log-ansi-${name})`);
});

test("explicit ANSI backgrounds keep the classic colours in both themes", () => {
  for (const theme of ["light", "dark"] as const) {
    const values = themeTokens(theme);
    for (const { code } of ANSI16) {
      // The background token holds the same classic colour that its paired foreground keeps.
      const style = ansiToSpans(`\x1b[${code};${code + 10}mW`)[0]!.style;
      expect(hex(values, String(style.backgroundColor)), `${theme} SGR ${code + 10}`).toBe(style.color);
    }
  }
});

test("strips cursor/OSC/DCS/C1 and hidden controls without creating HTML", () => {
  const spans = ansiToSpans("\x1b]8;;https://bad\x07safe\x1b]8;;\x1b\\\x1b[2J\x1bPsecret\x1b\\\x9b2J\x00<script>");
  expect(spans.map((s) => s.text).join("")).toBe("safe<script>");
});

test("plain text drops the same sequences whole and marks every control code but tab and line feed", () => {
  const text = ansiToText("\x1b]8;;https://bad\x07safe\x1b]8;;\x1b\\\x1b[2J\x1bPsecret\x1b\\\x9b2J\x00<script>");
  expect(text).toBe("safe�<script>");
  expect(ansiToText("\x1b[1mok\x1b[0m\ta\nb\rc\x7f\x85\x9c")).toBe("ok\ta\nb�c���");
});

test("an escape before a character outside the BMP takes the whole character, never half of it", () => {
  // A lone surrogate would make the failure-log send invalid JSON for the daemon.
  expect(ansiToText("\x1b😀ok\x1b(😀")).toBe("ok");
  expect(ansiToSpans("\x1b😀ok").map((span) => span.text)).toEqual(["ok"]);
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
