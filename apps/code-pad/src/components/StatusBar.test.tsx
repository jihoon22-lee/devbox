import { describe, expect, it } from "vitest";
import { cursorPosition, encodingForOption, encodingOptionValue } from "./StatusBar";

describe("status bar metadata", () => {
  it("reports one-based line and UTF-16 column positions", () => {
    expect(cursorPosition("one\n두😀", 0)).toEqual({ line: 1, column: 1 });
    expect(cursorPosition("one\n두😀", 4)).toEqual({ line: 2, column: 1 });
    expect(cursorPosition("one\n두😀", 7)).toEqual({ line: 2, column: 4 });
    expect(cursorPosition("one\n두😀", 999)).toEqual({ line: 2, column: 4 });
  });

  it("keeps the five encoding choices mapped to their wire metadata", () => {
    expect(encodingOptionValue({ encodingKind: "utf8", bom: false })).toBe("utf8");
    expect(encodingOptionValue({ encodingKind: "utf8", bom: true })).toBe("utf8-bom");
    expect(encodingOptionValue({ encodingKind: "utf16Le", bom: false })).toBe("utf16-le");
    expect(encodingOptionValue({ encodingKind: "utf16Be", bom: false })).toBe("utf16-be");
    expect(encodingOptionValue({ encodingKind: "cp949", bom: false })).toBe("cp949");
  });

  it("uses BOM-less UTF-16 for explicit reopen and a BOM for conversion", () => {
    expect(encodingForOption("utf16-le", "reopen")).toEqual({
      encodingKind: "utf16Le",
      bom: false,
    });
    expect(encodingForOption("utf16-be", "reopen")).toEqual({
      encodingKind: "utf16Be",
      bom: false,
    });
    expect(encodingForOption("utf16-le", "convert")).toEqual({
      encodingKind: "utf16Le",
      bom: true,
    });
  });
});
