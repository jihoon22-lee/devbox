import { expect, test } from "vitest";
import { detectIndent, detectLanguage } from "./languages";

test("extension and filename win before shebang, including Unicode paths", () => {
  expect(detectLanguage("src/가😀.ts", "#!/bin/bash")).toBe("typescript");
  expect(detectLanguage("Dockerfile", "")).toBe("dockerfile");
  expect(detectLanguage("tools/run script", "#!/usr/bin/env python3\nprint(1)")).toBe("python");
  expect(detectLanguage("script", "#!/usr/bin/env -S bash -eu\necho ok")).toBe("shell");
  expect(detectLanguage("script", "#!/bin/bash")).toBe("shell");
  expect(detectLanguage("unrecognized", "plain text")).toBeUndefined();
});
test("indentation follows a file's tabs or repeated space width", () => {
  expect(detectIndent("a\n\tb\n\t\tc\n")).toBe("\t");
  expect(detectIndent("a\n    b\n        c\n    d")).toBe("    ");
  expect(detectIndent("a\n  b\n    c")).toBe("  ");
  expect(detectIndent("plain")).toBe("  ");
});
