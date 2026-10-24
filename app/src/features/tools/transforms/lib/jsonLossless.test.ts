import { expect, it } from "vitest";
import { convertJsonYaml } from "./jsonYaml";
import { jsonFormatter, jsonMinifier } from "./transformers";

it.each([jsonFormatter, jsonMinifier])(
  "preserves numeric and duplicate-key tokens through the tool factory",
  async (factory) => {
    const input =
      '{ "id":9007199254740993, "value":1e400, "tiny":1e-400, "fraction":0.1234567890123456789, "zero":-0, "id":"first", "id":"second", "text":"a b\\n\\u0061" }';
    const result = await factory()(input);
    expect(result.error).toBeUndefined();
    for (const token of [
      "9007199254740993",
      "1e400",
      "1e-400",
      "0.1234567890123456789",
      "-0",
      '"first"',
      '"second"',
      '"a b\\n\\u0061"',
    ])
      expect(result.output).toContain(token);
  },
);

it.each(['{"id":"first","id":"second"}', '{"outer":[{"id":1,"\\u0069d":2}]}'])(
  "rejects duplicate JSON keys before YAML conversion: %s",
  (input) => {
    const result = convertJsonYaml(input, "json-to-yaml");
    expect(result.output).toBe("");
    expect(result.error?.code).toBe("DUPLICATE_JSON_KEY");
  },
);

it("allows the same JSON key in independent objects", () => {
  expect(convertJsonYaml('[{"id":1},{"id":2}]', "json-to-yaml").error).toBeNull();
});
