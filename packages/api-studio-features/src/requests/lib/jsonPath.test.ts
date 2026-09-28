import { describe, expect, it } from "vitest";
import { evaluateJsonPath, JsonPathError } from "./jsonPath";

const doc = {
  data: {
    items: [
      { id: 1, tags: ["a"] },
      { id: 2, tags: [] },
    ],
    "odd key": true,
  },
  token: "t",
};

describe("jsonPath", () => {
  it("selects members, indices and wildcards", () => {
    expect(evaluateJsonPath(doc, "$.token")).toEqual(["t"]);
    expect(evaluateJsonPath(doc, "$.data.items[0].id")).toEqual([1]);
    expect(evaluateJsonPath(doc, "$.data.items[-1].id")).toEqual([2]);
    expect(evaluateJsonPath(doc, "$.data.items[*].id")).toEqual([1, 2]);
    expect(evaluateJsonPath(doc, "$['data']['odd key']")).toEqual([true]);
    expect(evaluateJsonPath(doc, "$.data.*")).toHaveLength(2);
  });
  it("descends recursively", () => {
    expect(evaluateJsonPath(doc, "$..id")).toEqual([1, 2]);
  });
  it("returns nothing for missing paths and rejects bad syntax", () => {
    expect(evaluateJsonPath(doc, "$.missing.deeper")).toEqual([]);
    for (const bad of ["token", "$.", "$[", "$.a[?(@.b)]", "$..", "$['x"]) {
      expect(() => evaluateJsonPath(doc, bad)).toThrow(JsonPathError);
    }
  });
  it("stops on very large documents", () => {
    const wide = { list: Array.from({ length: 20_000 }, (_, i) => ({ i })) };
    expect(() => evaluateJsonPath(wide, "$..i")).toThrow("JSONPath 탐색 한도");
  });
});
it("uses own members, escaped bracket keys and bounded iterative traversal", () => {
  expect(evaluateJsonPath({}, "$.constructor")).toEqual([]);
  expect(evaluateJsonPath({ "a'b": 3 }, "$['a\\'b']")).toEqual([3]);
  const cycle: { self?: unknown } = {};
  cycle.self = cycle;
  expect(() => evaluateJsonPath(cycle, "$..x")).toThrow("JSONPath 탐색 한도");
  expect(() => evaluateJsonPath({}, "$" + ".x".repeat(128))).toThrow(JsonPathError);
});
