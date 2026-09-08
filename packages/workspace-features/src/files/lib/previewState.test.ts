import { describe, expect, it } from "vitest";
import { applySvgResult } from "./previewState";

describe("preview SVG state", () => {
  it("retains the last successful SVG after a syntax failure", () => {
    const cache = new Map<string, string>();
    expect(applySvgResult(cache, "0", { ok: true, svg: "<svg>good</svg>" })).toEqual({
      svg: "<svg>good</svg>",
      hasError: false,
    });
    expect(applySvgResult(cache, "0", { ok: false })).toEqual({
      svg: "<svg>good</svg>",
      hasError: true,
    });
  });
});
