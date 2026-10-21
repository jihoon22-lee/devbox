// @vitest-environment node
import { spawnSync } from "node:child_process";
import { expect, it } from "vitest";
import { emptyRequest } from "../importers";
import type { RequestTemplate } from "../template";
import { generateCode } from "./index";

// 적응: v0.9.0 `codegen.syntax.test.mjs`를 vitest include(`*.test.ts`)에 맞춘 것. 실행하지 않고 문법만 검사한다
// (JS는 Function 생성자, Python은 ast.parse, curl은 bash -n).
const env: never[] = [];
const post: RequestTemplate = { ...emptyRequest(), method: "POST", url: "https://example.test/" };
it("produces parsable JavaScript, Python and shell for quotes, newlines and Unicode", () => {
  const request = { ...post, body_kind: "raw", body: "'\"\\\n한글😀 " };
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  expect(() => new AsyncFunction(generateCode("fetch", request, env).code)).not.toThrow();
  const axios = generateCode("axios", request, env).code.replace(/^import .*$/gm, "");
  expect(() => new AsyncFunction(axios)).not.toThrow();
  for (const [target, executable, args] of [
    ["python", "python3", ["-c", "import ast,sys; ast.parse(sys.stdin.read())"]],
    ["curl", "bash", ["-n"]],
  ] as const) {
    const result = spawnSync(executable, [...args], {
      input: generateCode(target, request, env).code,
      encoding: "utf8",
    });
    expect(result.status, result.stderr).toBe(0);
  }
});
