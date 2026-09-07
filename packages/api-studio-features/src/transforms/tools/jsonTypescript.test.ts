import { describe, expect, it } from "vitest";
import {
  convertJsonToTypescript,
  MAX_JSON_TYPESCRIPT_DEPTH,
  MAX_JSON_TYPESCRIPT_INPUT_BYTES,
  MAX_JSON_TYPESCRIPT_NODES,
} from "./jsonTypescript";

describe("convertJsonToTypescript", () => {
  it("정렬된 속성과 중첩 object를 root interface로 생성한다", () => {
    const result = convertJsonToTypescript(
      '{"version":1,"service":{"ports":[3000,8080],"name":"api"},"enabled":true}',
      "ApiConfig",
    );

    expect(result.error).toBeNull();
    expect(result.output).toBe([
      "export interface ApiConfig {",
      "  enabled: boolean;",
      "  service: {",
      "    name: string;",
      "    ports: Array<number>;",
      "  };",
      "  version: number;",
      "}",
      "",
    ].join("\n"));
  });

  it("배열 object를 병합해 누락 속성과 null union을 추론한다", () => {
    const input = JSON.stringify({
      users: [
        { id: 1, meta: { active: true, score: 3 }, name: "Ada" },
        { meta: { score: null, active: false }, id: 2 },
      ],
    });
    const result = convertJsonToTypescript(input, "ApiResponse");

    expect(result.error).toBeNull();
    expect(result.output).toBe([
      "export interface ApiResponse {",
      "  users: Array<{",
      "    id: number;",
      "    meta: {",
      "      active: boolean;",
      "      score: null | number;",
      "    };",
      "    name?: string;",
      "  }>;",
      "}",
      "",
    ].join("\n"));
  });

  it("object key와 배열 표본 순서가 달라도 같은 결과를 만든다", () => {
    const first = convertJsonToTypescript(
      '{"items":[{"name":"one","id":1},{"id":2,"active":true}]}',
      "Root",
    );
    const second = convertJsonToTypescript(
      '{"items":[{"active":false,"id":2},{"id":1,"name":"two"}]}',
      "Root",
    );

    expect(first.error).toBeNull();
    expect(second.error).toBeNull();
    expect(first.output).toBe(second.output);
    expect(first.output).toContain("active?: boolean;");
    expect(first.output).toContain("name?: string;");
  });

  it("서로 다른 key를 가진 대량 object 표본을 결정적으로 한 번에 병합한다", () => {
    const samples = Array.from({ length: 2_000 }, (_, index) => ({ [`field_${index}`]: index }));
    const forward = convertJsonToTypescript(JSON.stringify(samples), "SparseRows");
    const reverse = convertJsonToTypescript(JSON.stringify([...samples].reverse()), "SparseRows");

    expect(forward.error).toBeNull();
    expect(forward.output).toBe(reverse.output);
    expect(forward.output).toContain("field_0?: number;");
    expect(forward.output).toContain("field_1999?: number;");
  });

  it("빈 배열과 혼합 배열, root primitive를 명시적으로 표현한다", () => {
    expect(convertJsonToTypescript("[]", "EmptyList").output)
      .toBe("export type EmptyList = Array<unknown>;\n");
    expect(convertJsonToTypescript('[1,"two",null]', "MixedList").output)
      .toBe("export type MixedList = Array<null | number | string>;\n");
    expect(convertJsonToTypescript("true", "Flag").output)
      .toBe("export type Flag = boolean;\n");
  });

  it("빈 object와 TypeScript identifier가 아닌 key를 안전하게 표현한다", () => {
    expect(convertJsonToTypescript("{}", "EmptyObject").output)
      .toBe("export type EmptyObject = Record<string, never>;\n");

    const result = convertJsonToTypescript(
      '{"__proto__":{"safe":true},"content-type":"json","normal":1}',
      "Headers",
    );
    expect(result.error).toBeNull();
    expect(result.output).toContain("__proto__: {");
    expect(result.output).toContain('"content-type": string;');
    expect(result.output).toContain("normal: number;");
  });

  it("잘못된 JSON 위치를 반환하고 입력 원문을 오류에 반영하지 않는다", () => {
    const secret = "DO_NOT_REFLECT_JSON_SECRET";
    const result = convertJsonToTypescript(
      `{\n  "token": "${secret}",\n  "broken": }`,
      "SecretShape",
    );

    expect(result.output).toBe("");
    expect(result.error?.code).toBe("INVALID_JSON");
    expect(result.error?.line).toBe(3);
    expect(result.error?.column).toBeGreaterThan(1);
    expect(JSON.stringify(result.error)).not.toContain(secret);
  });

  it("JSON comment와 trailing comma를 허용하지 않는다", () => {
    expect(convertJsonToTypescript('{"name":"api" // comment\n}', "Root").error?.code)
      .toBe("INVALID_JSON");
    expect(convertJsonToTypescript('{"name":"api",}', "Root").error?.code)
      .toBe("INVALID_JSON");
  });

  it("root type 이름의 빈 값, 예약어, 잘못된 identifier를 구분한다", () => {
    expect(convertJsonToTypescript("{}", "").error?.code).toBe("EMPTY_ROOT_TYPE_NAME");
    expect(convertJsonToTypescript("{}", "type").error?.code).toBe("RESERVED_ROOT_TYPE_NAME");
    expect(convertJsonToTypescript("{}", "123 Root").error?.code).toBe("INVALID_ROOT_TYPE_NAME");
  });

  it("입력 byte, 중첩 깊이와 값 개수 상한을 고정 오류로 처리한다", () => {
    const oversized = `"${"가".repeat(Math.ceil(MAX_JSON_TYPESCRIPT_INPUT_BYTES / 3))}"`;
    expect(convertJsonToTypescript(oversized, "Root").error?.code).toBe("INPUT_TOO_LARGE");

    const depth = MAX_JSON_TYPESCRIPT_DEPTH + 2;
    const deeplyNested = `${"[".repeat(depth)}0${"]".repeat(depth)}`;
    expect(convertJsonToTypescript(deeplyNested, "Root").error?.code).toBe("INPUT_TOO_DEEP");

    const tooManyValues = `[${Array.from({ length: MAX_JSON_TYPESCRIPT_NODES }, () => "0").join(",")}]`;
    expect(convertJsonToTypescript(tooManyValues, "Root").error?.code).toBe("INPUT_TOO_COMPLEX");
  });
});
