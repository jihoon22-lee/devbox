import { describe, expect, it } from "vitest";
import {
  MAX_BODY_CHARS,
  MAX_BODY_BYTES,
  MAX_HEADER_NAME_BYTES,
  MAX_HEADER_NAME_CHARS,
  MAX_HEADER_TOTAL_BYTES,
  MAX_HEADER_TOTAL_CHARS,
  MAX_HEADER_VALUE_BYTES,
  MAX_HEADER_VALUE_CHARS,
  MAX_METHOD_BYTES,
  MAX_METHOD_CHARS,
  MAX_PATH_BYTES,
  MAX_PATH_CHARS,
  MAX_RESPONSE_DELAY_MS,
  MAX_RESPONSE_STATUS,
  MAX_RULES,
  MAX_RULE_HEADERS,
  MIN_RESPONSE_STATUS,
  validateRuleCollection,
  type RuleValidationIssue,
  validateRule,
} from "./ruleValidation";

const validRule = {
  id: "rule-1",
  method: "POST",
  path: "/hook",
  status: 200,
  headers: [] as Array<[string, string]>,
  body: "",
  delayMs: 0,
};

describe("validateRule", () => {
  it("accepts the documented response status and delay boundaries", () => {
    expect(validateRule({
      ...validRule,
      status: MIN_RESPONSE_STATUS,
      delayMs: MAX_RESPONSE_DELAY_MS,
    })).toEqual([]);
    expect(validateRule({
      ...validRule,
      status: MAX_RESPONSE_STATUS,
    })).toEqual([]);
  });

  it("rejects out-of-range and fractional response values", () => {
    expect(validateRule({ ...validRule, status: MIN_RESPONSE_STATUS - 1 })[0]).toMatchObject({
      field: "status",
    });
    expect(validateRule({ ...validRule, status: 200.5 })[0]).toMatchObject({
      field: "status",
    });
    expect(validateRule({ ...validRule, delayMs: MAX_RESPONSE_DELAY_MS + 1 })[0]).toMatchObject({
      field: "delayMs",
    });
    expect(validateRule({ ...validRule, delayMs: -1 })[0]).toMatchObject({
      field: "delayMs",
    });
  });

  it("rejects paths that cannot represent a local request path", () => {
    expect(validateRule({ ...validRule, path: "hook" })[0]).toMatchObject({ field: "path" });
    expect(validateRule({ ...validRule, path: "/hook\u0085secret" })[0]).toMatchObject({ field: "path" });
    expect(validateRule({
      ...validRule,
      path: `/${"p".repeat(MAX_PATH_CHARS - 1)}`,
    })).toEqual([]);
    expect(validateRule({
      ...validRule,
      path: `/${"p".repeat(MAX_PATH_CHARS)}`,
    })[0]).toMatchObject({ field: "path" });
    expect(MAX_PATH_BYTES).toBeGreaterThan(MAX_PATH_CHARS);
  });

  it("mirrors method and response body character/byte bounds", () => {
    expect(validateRule({ ...validRule, method: "A".repeat(MAX_METHOD_CHARS) })).toEqual([]);
    expect(validateRule({ ...validRule, method: "!custom" })).toEqual([]);
    expect(validateRule({ ...validRule, method: "A".repeat(MAX_METHOD_CHARS + 1) })[0]).toMatchObject({
      field: "method",
    });
    expect(validateRule({ ...validRule, method: "POST JSON" })[0]).toMatchObject({ field: "method" });
    expect(validateRule({ ...validRule, method: "" })[0]).toMatchObject({ field: "method" });
    expect(validateRule({ ...validRule, method: null })).toEqual([]);
    expect(MAX_METHOD_BYTES).toBe(MAX_METHOD_CHARS);

    expect(validateRule({
      ...validRule,
      body: "b".repeat(MAX_BODY_CHARS),
    })).toEqual([]);
    expect(validateRule({
      ...validRule,
      body: "b".repeat(MAX_BODY_CHARS + 1),
    })[0]).toMatchObject({ field: "body" });
    expect(validateRule({
      ...validRule,
      body: "🙂".repeat(MAX_BODY_BYTES / 4 + 1),
    })[0]).toMatchObject({ field: "body" });
    expect(validateRule({ ...validRule, body: "\ud800" })[0]).toMatchObject({ field: "body" });
  });

  it("mirrors header name, value, count, and aggregate bounds", () => {
    expect(validateRule({
      ...validRule,
      headers: Array.from({ length: MAX_RULE_HEADERS }, (_, index) => [`X-${index}`, "ok"] as [string, string]),
    })).toEqual([]);
    expect(validateRule({
      ...validRule,
      headers: Array.from({ length: MAX_RULE_HEADERS + 1 }, (_, index) => [`X-${index}`, "ok"] as [string, string]),
    })[0]).toMatchObject({ field: "headers" });
    expect(validateRule({
      ...validRule,
      headers: [["not a header", "ok"]],
    })[0]).toMatchObject({ field: "headers" });
    expect(validateRule({
      ...validRule,
      headers: [["X-Test", "v".repeat(MAX_HEADER_VALUE_CHARS)]],
    })).toEqual([]);
    expect(validateRule({
      ...validRule,
      headers: [["X-Test", "v".repeat(MAX_HEADER_VALUE_CHARS + 1)]],
    })[0]).toMatchObject({ field: "headers" });
    expect(validateRule({
      ...validRule,
      headers: Array.from({ length: 5 }, (_, index) => [
        `X-${index}`,
        "v".repeat(MAX_HEADER_TOTAL_CHARS / 4),
      ] as [string, string]),
    }).some((issue: RuleValidationIssue) => issue.field === "headers")).toBe(true);
    expect(MAX_HEADER_NAME_BYTES).toBe(MAX_HEADER_NAME_CHARS);
    expect(MAX_HEADER_VALUE_BYTES).toBe(MAX_HEADER_VALUE_CHARS * 4);
    expect(MAX_HEADER_TOTAL_BYTES).toBe(MAX_HEADER_TOTAL_CHARS * 4);
  });

  it("validates the complete collection before an add or edit", () => {
    const atCount = Array.from({ length: MAX_RULES }, (_, index) => ({
      ...validRule,
      id: `rule-${index}`,
    }));
    expect(validateRuleCollection(atCount)).toEqual([]);

    const overCount = [...atCount, { ...validRule, id: "rule-new" }];
    expect(validateRuleCollection(overCount).some((issue) => issue.field === "collection")).toBe(true);

    const overAggregate = Array.from({ length: 8 }, (_, index) => ({
      ...validRule,
      id: `large-${index}`,
      body: "x".repeat(MAX_BODY_CHARS),
    }));
    expect(validateRuleCollection(overAggregate).some((issue) => issue.field === "collection")).toBe(true);
  });
});
