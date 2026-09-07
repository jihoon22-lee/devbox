import { describe, expect, it } from "vitest";
import {
  analyzeMcpToolSchema,
  appendMcpListPage,
  hasMcpCapability,
  initialMcpArguments,
  parseMcpJsonObject,
  projectMcpListPage,
  setMcpValueAtPath,
  validateMcpArguments,
} from "./mcp";

describe("MCP frontend projections", () => {
  it("gates sections only on object-shaped capabilities", () => {
    expect(hasMcpCapability({ tools: {} }, "tools")).toBe(true);
    expect(hasMcpCapability({ tools: true }, "tools")).toBe(false);
    expect(hasMcpCapability({}, "resources")).toBe(false);
  });

  it("projects one list page and rejects duplicate identities across pages", () => {
    const first = projectMcpListPage({
      tools: [{ name: "echo", inputSchema: { type: "object" } }],
    }, "tools");
    expect(appendMcpListPage([], first, "tools")).toHaveLength(1);
    const duplicate = projectMcpListPage({
      tools: [{ name: "echo", inputSchema: { type: "object" } }],
    }, "tools");
    expect(() => appendMcpListPage(first.items, duplicate, "tools"))
      .toThrow("mcp_message_invalid");
    expect(() => projectMcpListPage({ resources: [{ uri: "bad\nuri" }] }, "resources"))
      .toThrow("mcp_message_invalid");
    expect(() => projectMcpListPage({ resources: [{ uri: "fixture://valid" }] }, "resources"))
      .toThrow("mcp_message_invalid");
    expect(() => projectMcpListPage({ tools: [{ name: "bad", inputSchema: {} }] }, "tools"))
      .toThrow("mcp_message_invalid");
    expect(() => projectMcpListPage({
      prompts: [{ name: "draft", arguments: [{ name: "topic" }, { name: "topic" }] }],
    }, "prompts")).toThrow("mcp_message_invalid");
  });
});

describe("MCP schema projection", () => {
  const schema = {
    type: "object",
    additionalProperties: false,
    required: ["message", "options"],
    properties: {
      message: { type: "string", minLength: 1, maxLength: 20 },
      count: { type: "integer", minimum: 1, maximum: 5 },
      options: {
        type: "object",
        required: ["enabled"],
        properties: { enabled: { type: "boolean" } },
      },
      tags: { type: "array", maxItems: 3, items: { type: "string" } },
    },
  };

  it("builds and validates the supported deterministic form subset", () => {
    const analysis = analyzeMcpToolSchema({
      ...schema,
      $schema: "https://json-schema.org/draft/2020-12/schema",
    });
    expect(analysis.mode).toBe("form");
    let value = initialMcpArguments(schema);
    expect(validateMcpArguments(schema, value)).toContain("arguments.message: 너무 짧습니다.");
    value = setMcpValueAtPath(value, ["message"], "hello");
    value = setMcpValueAtPath(value, ["options", "enabled"], true);
    value = setMcpValueAtPath(value, ["count"], 3);
    value = setMcpValueAtPath(value, ["tags"], ["one", "two"]);
    expect(validateMcpArguments(schema, value)).toEqual([]);
    expect(analyzeMcpToolSchema({ type: "object" }).mode).toBe("form");
    expect(initialMcpArguments({ type: "object" })).toEqual({});
    expect(validateMcpArguments({ type: "object" }, {})).toEqual([]);
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: { nested: { type: "string", $schema: "nested" } },
    }).mode).toBe("json");
  });

  it("supports valid x-mcp-header fields and fails closed on invalid placement", () => {
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: {
        region: { type: "string", "x-mcp-header": "Region" },
        nested: {
          type: "object",
          properties: { enabled: { type: "boolean", "x-mcp-header": "Enabled" } },
        },
      },
    }).mode).toBe("form");
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: {
        first: { type: "string", "x-mcp-header": "Tenant" },
        second: { type: "string", "x-mcp-header": "tenant" },
      },
    }).mode).toBe("json");
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: {
        values: {
          type: "array",
          items: {
            type: "object",
            properties: { item: { type: "string", "x-mcp-header": "Item" } },
          },
        },
      },
    }).mode).toBe("json");
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: Object.fromEntries(Array.from({ length: 101 }, (_, index) => [
        `field-${index}`,
        { type: "string", "x-mcp-header": `Field-${index}` },
      ])),
    }).mode).toBe("json");
  });

  it("fails closed for refs, composition, and unknown keywords", () => {
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: { value: { $ref: "#/$defs/value" } },
    }).mode).toBe("json");
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: {},
      oneOf: [],
    }).mode).toBe("json");
    expect(validateMcpArguments({
      type: "object",
      properties: {},
      patternProperties: {},
    }, {})).toEqual(["지원하지 않는 schema는 호출할 수 없습니다."]);
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: { value: { type: "string", minLength: 5, maxLength: 2 } },
    }).mode).toBe("json");
    expect(analyzeMcpToolSchema({
      type: "object",
      properties: { values: { type: "array", minItems: 101, items: { type: "string" } } },
    }).mode).toBe("json");
  });

  it("parses only bounded JSON objects", () => {
    expect(parseMcpJsonObject('{"value":1}')).toEqual({ value: 1 });
    expect(() => parseMcpJsonObject("[]")).toThrow("mcp_message_invalid");
    expect(() => parseMcpJsonObject("not-json")).toThrow("mcp_message_invalid");
  });
});
