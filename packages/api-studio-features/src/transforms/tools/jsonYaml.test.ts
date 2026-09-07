import { describe, expect, it } from "vitest";
import {
  convertJsonYaml,
  MAX_JSON_YAML_INPUT_BYTES,
} from "./jsonYaml";

describe("convertJsonYaml", () => {
  it("중첩 JSON을 2칸 들여쓰기 YAML로 변환한다", () => {
    const result = convertJsonYaml(
      '{"service":{"name":"devbox","ports":[3000,8080]},"enabled":true}',
      "json-to-yaml",
    );

    expect(result.error).toBeNull();
    expect(result.output).toBe([
      "service:",
      "  name: devbox",
      "  ports:",
      "    - 3000",
      "    - 8080",
      "enabled: true",
      "",
    ].join("\n"));
  });

  it("__proto__ key를 데이터로 보존하고 JSON 확장 문법은 거부한다", () => {
    const prototypeKey = convertJsonYaml('{"__proto__":{"safe":true}}', "json-to-yaml");
    const comment = convertJsonYaml('{"name":"devbox" // comment\n}', "json-to-yaml");
    const trailingComma = convertJsonYaml('{"name":"devbox",}', "json-to-yaml");

    expect(prototypeKey.error).toBeNull();
    expect(prototypeKey.output).toContain("__proto__:");
    expect(prototypeKey.output).toContain("safe: true");
    expect(comment.error?.code).toBe("INVALID_JSON");
    expect(trailingComma.error?.code).toBe("INVALID_JSON");
  });

  it("JS가 안전하게 표현할 수 없는 JSON 숫자를 위치와 함께 거부한다", () => {
    for (const input of ["9007199254740993", "1e400"]) {
      const result = convertJsonYaml(input, "json-to-yaml");
      expect(result.error?.code).toBe("UNSUPPORTED_JSON_NUMBER");
      expect(result.error?.line).toBe(1);
      expect(result.error?.column).toBe(1);
    }
  });

  it("과도하게 깊은 JSON을 UI 예외 대신 고정 오류로 격리한다", () => {
    const deeplyNested = `${"[".repeat(20_000)}0${"]".repeat(20_000)}`;
    const result = convertJsonYaml(deeplyNested, "json-to-yaml");

    expect(result.output).toBe("");
    expect(["JSON_PARSE_FAILED", "YAML_SERIALIZE_FAILED"]).toContain(result.error?.code);
  });

  it("YAML 주석을 제거하고 alias 값을 JSON에 확장한다", () => {
    const result = convertJsonYaml([
      "# environment defaults",
      "defaults: &defaults",
      "  retries: 3",
      "copy: *defaults",
    ].join("\n"), "yaml-to-json");

    expect(result.error).toBeNull();
    expect(JSON.parse(result.output)).toEqual({
      defaults: { retries: 3 },
      copy: { retries: 3 },
    });
    expect(result.output).not.toContain("environment defaults");
    expect(result.output).not.toContain("&defaults");
  });

  it("root YAML 정수를 JSON 숫자로 변환한다", () => {
    const result = convertJsonYaml("3", "yaml-to-json");
    expect(result).toEqual({ output: "3", error: null });
  });

  it("merge key를 확장하지 않고 YAML 1.2의 일반 key로 다룬다", () => {
    const result = convertJsonYaml([
      "base: &base",
      "  retries: 3",
      "service:",
      "  <<: *base",
      "  name: api",
    ].join("\n"), "yaml-to-json");

    expect(result.error).toBeNull();
    expect(JSON.parse(result.output)).toEqual({
      base: { retries: 3 },
      service: { "<<": { retries: 3 }, name: "api" },
    });
  });

  it("깨진 JSON의 1-based 위치를 반환하고 입력 원문을 오류에 노출하지 않는다", () => {
    const secret = "DO_NOT_REFLECT_THIS_SECRET";
    const result = convertJsonYaml(
      `{\n  \"token\": \"${secret}\",\n  \"broken\": }`,
      "json-to-yaml",
    );

    expect(result.output).toBe("");
    expect(result.error?.code).toBe("INVALID_JSON");
    expect(result.error?.line).toBe(3);
    expect(result.error?.column).toBeGreaterThan(1);
    expect(JSON.stringify(result.error)).not.toContain(secret);
  });

  it("깨진 YAML의 위치와 안전한 오류 code를 반환한다", () => {
    const secret = "DO_NOT_REFLECT_THIS_YAML_SECRET";
    const result = convertJsonYaml(
      `token: ${secret}\nbroken: [one, two`,
      "yaml-to-json",
    );

    expect(result.output).toBe("");
    expect(result.error).not.toBeNull();
    expect(result.error?.line).toBe(2);
    expect(result.error?.column).toBeGreaterThan(1);
    expect(JSON.stringify(result.error)).not.toContain(secret);
  });

  it("중복 key와 여러 YAML 문서를 거부한다", () => {
    const duplicate = convertJsonYaml("name: first\nname: second", "yaml-to-json");
    const multiple = convertJsonYaml("name: first\n---\nname: second", "yaml-to-json");

    expect(duplicate.error?.code).toBe("DUPLICATE_KEY");
    expect(multiple.error?.code).toBe("MULTIPLE_DOCS");
  });

  it("의미가 손실되는 YAML tag와 JSON 범위 밖의 숫자를 거부한다", () => {
    const tagged = convertJsonYaml("value: !duration 5m", "yaml-to-json");
    const unsafeInteger = convertJsonYaml("value: 9007199254740993", "yaml-to-json");
    const infinity = convertJsonYaml("value: .inf", "yaml-to-json");

    expect(tagged.error?.code).toBe("TAG_RESOLVE_FAILED");
    expect(tagged.error?.line).toBe(1);
    expect(unsafeInteger.error?.code).toBe("UNSUPPORTED_YAML_VALUE");
    expect(infinity.error?.code).toBe("UNSUPPORTED_YAML_VALUE");
  });

  it("순환 alias와 과도한 alias 확장을 고정된 오류로 막는다", () => {
    const circular = convertJsonYaml("value: &value [*value]", "yaml-to-json");
    const aliases = Array.from({ length: 51 }, () => "*base").join(", ");
    const excessive = convertJsonYaml(`base: &base [1]\nvalues: [${aliases}]`, "yaml-to-json");

    for (const result of [circular, excessive]) {
      expect(result.output).toBe("");
      expect(result.error?.code).toBe("UNSUPPORTED_YAML_GRAPH");
    }
  });

  it("빈 입력은 비우고 UTF-8 byte 기준 입력 제한을 적용한다", () => {
    expect(convertJsonYaml("  \n", "json-to-yaml")).toEqual({ output: "", error: null });

    const oversized = convertJsonYaml(
      `\"${"가".repeat(Math.ceil(MAX_JSON_YAML_INPUT_BYTES / 3))}\"`,
      "json-to-yaml",
    );
    expect(oversized.error?.code).toBe("INPUT_TOO_LARGE");
    expect(oversized.output).toBe("");
  });
});
