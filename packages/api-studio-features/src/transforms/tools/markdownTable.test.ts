import { describe, expect, it } from "vitest";
import {
  formatMarkdownTable,
  MARKDOWN_TABLE_LIMITS,
  markdownTableErrorMessage,
} from "./markdownTable";

describe("formatMarkdownTable", () => {
  it("pads uneven rows, inserts a missing separator, and preserves source order", () => {
    const input = "| name | value |\n| devbox |\n| tool | 0.5 | extra |";
    const expected = [
      "| name   | value |       |",
      "| ------ | ----- | ----- |",
      "| devbox |       |       |",
      "| tool   | 0.5   | extra |",
    ].join("\n");

    const first = formatMarkdownTable(input);
    expect(first).toEqual({ output: expected, error: null });
    expect(formatMarkdownTable(input)).toEqual(first);
  });

  it("retains left, center, and right alignment markers", () => {
    const result = formatMarkdownTable(
      "| left | center | right |\n| :--- | :---: | ---: |\n| a | b | 7 |\n| longer | x | 1000 |",
    );

    expect(result).toEqual({
      output: [
        "| left   | center | right |",
        "| :----- | :----: | ----: |",
        "| a      |    b   |     7 |",
        "| longer |    x   |  1000 |",
      ].join("\n"),
      error: null,
    });
  });

  it("normalizes line endings and treats escaped pipes as cell text", () => {
    const result = formatMarkdownTable("| key | note |\r\n| --- | --- |\r\n| a\\|b | `keep` |\r");
    expect(result.error).toBeNull();
    expect(result.output).toContain("a\\|b");
    expect(result.output).toContain("`keep`");
    expect(result.output.split("\n")).toHaveLength(3);
    expect(formatMarkdownTable(result.output)).toEqual(result);
  });

  it("keeps pipes inside matched backtick code spans in their source cell", () => {
    const result = formatMarkdownTable(
      "| code | value |\n| --- | --- |\n| `a|b` | keep |\n| ``x|`y`` | next |",
    );

    expect(result.error).toBeNull();
    expect(result.output).toContain("`a\\|b`");
    expect(result.output).toContain("``x\\|`y``");
    expect(result.output).toContain("| keep  |");
    expect(result.output).toContain("| next  |");
    expect(formatMarkdownTable(result.output)).toEqual(result);
  });

  it("does not hide delimiters after an unmatched backtick run", () => {
    const result = formatMarkdownTable("| `open | second |\n| value | next |");
    expect(result.error).toBeNull();
    expect(result.output.split("\n")[0]).toContain("| `open | second |");
  });

  it("preserves backslash parity and tag-like text as data", () => {
    const input = [
      "| value | text |",
      "| --- | --- |",
      `| literal ${"\\".repeat(2)} | <tag> |`,
      `| escaped ${"\\|"} value | <img src=x onerror=alert(1)> |`,
      `| odd ${"\\".repeat(3)}|pipe | plain |`,
    ].join("\n");
    const result = formatMarkdownTable(input);
    expect(result.error).toBeNull();
    expect(result.output).toContain(`literal ${"\\".repeat(2)}`);
    expect(result.output).toContain(`escaped ${"\\|"} value`);
    expect(result.output).toContain(`odd ${"\\".repeat(3)}|pipe`);
    expect(result.output).toContain("<img src=x onerror=alert(1)>");
  });

  it("returns an empty result for blank input and fixed errors for malformed rows", () => {
    expect(formatMarkdownTable(" \r\n\n ")).toEqual({ output: "", error: null });
    expect(formatMarkdownTable("credential=secret /tmp/private")).toEqual({
      output: "",
      error: {
        code: "MALFORMED_ROW",
        message: markdownTableErrorMessage("MALFORMED_ROW"),
      },
    });
    expect(formatMarkdownTable("| a | b |\n| --- | -- |\n| x | y |")).toEqual({
      output: "",
      error: {
        code: "MALFORMED_SEPARATOR",
        message: markdownTableErrorMessage("MALFORMED_SEPARATOR"),
      },
    });
  });

  it("rejects controls and malformed Unicode before formatting", () => {
    for (const control of ["\t", "\u0001", "\u0085", "\u2028", "\u2029"]) {
      expect(formatMarkdownTable(`| ${control} |`).error?.code).toBe("INVALID_CONTROL");
    }
    expect(formatMarkdownTable("| \ud800 |" as string).error?.code).toBe("INVALID_UNICODE");
  });

  it("enforces input, row, column, cell, and output byte bounds", () => {
    expect(formatMarkdownTable("x".repeat(MARKDOWN_TABLE_LIMITS.maxInputBytes + 1)).error?.code).toBe(
      "INPUT_TOO_LARGE",
    );
    expect(
      formatMarkdownTable(Array.from({ length: MARKDOWN_TABLE_LIMITS.maxRows + 1 }, () => "| x |").join("\n"))
        .error?.code,
    ).toBe("TOO_MANY_ROWS");
    expect(
      formatMarkdownTable(`| ${Array.from({ length: MARKDOWN_TABLE_LIMITS.maxColumns + 1 }, () => "x").join(" | ")} |`)
        .error?.code,
    ).toBe("TOO_MANY_COLUMNS");
    expect(
      formatMarkdownTable(`| ${"x".repeat(MARKDOWN_TABLE_LIMITS.maxCellCodePoints + 1)} |`).error?.code,
    ).toBe("CELL_TOO_LARGE");

    const wide = [
      `| ${"x".repeat(MARKDOWN_TABLE_LIMITS.maxCellCodePoints)} |`,
      ...Array.from({ length: MARKDOWN_TABLE_LIMITS.maxRows - 1 }, () => "| |"),
    ].join("\n");
    expect(formatMarkdownTable(wide).error?.code).toBe("OUTPUT_TOO_LARGE");
  });

  it("fails closed for non-string runtime callers without reflecting data", () => {
    expect(formatMarkdownTable(null as unknown as string)).toEqual({
      output: "",
      error: {
        code: "FORMAT_FAILED",
        message: markdownTableErrorMessage("FORMAT_FAILED"),
      },
    });
  });
});
