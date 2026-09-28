import { expect, it } from "vitest";
import { generateCode } from "./codegen";
import { emptyRequest } from "./importers";
const env = [];
const post = { ...emptyRequest(), method: "POST", url: "https://example.test/" };
it("produces parsable JavaScript, Python and shell for quotes, newlines and Unicode", async () => {
  const { spawnSync } = await import("node:child_process");
  const request = { ...post, body_kind: "raw", body: "'\"\\\n한글😀\u2028" };
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  expect(() => new AsyncFunction(generateCode("fetch", request, env).code)).not.toThrow();
  for (const [target, executable, args] of [
    ["python", "python3", ["-c", "import ast,sys; ast.parse(sys.stdin.read())"]],
    ["curl", "bash", ["-n"]],
  ]) {
    const result = spawnSync(executable, [...args], {
      input: generateCode(target, request, env).code,
      encoding: "utf8",
    });
    expect(result.status, result.stderr).toBe(0);
  }
});
