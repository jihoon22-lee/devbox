import { describe, expect, it } from "vitest";
import {
  pipelineCompatibility,
  pipelineOutputType,
  PIPELINE_LIMITS,
  runPipeline,
} from "./transformPipeline";

describe("typed transformer pipeline (#341)", () => {
  it("connects compatible JSON stages and reports the resulting type", () => {
    const result = runPipeline(
      '{"name":"Ada"}',
      "json",
      [{ transformerId: "json-format" }, { transformerId: "json-to-typescript" }],
    );

    expect(result.error).toBeNull();
    expect(result.outputType).toBe("typescript");
    expect(result.completedSteps).toBe(2);
    expect(result.output).toContain("interface RootObject");
  });

  it("connects Base64 text decoding explicitly and keeps output in memory", () => {
    const result = runPipeline("aGVsbG8=", "base64", [{ transformerId: "base64-decode" }]);

    expect(result).toMatchObject({ output: "hello", outputType: "text", completedSteps: 1, error: null });
  });

  it("returns a fixed mismatch before executing an incompatible stage", () => {
    const result = runPipeline("not-json", "text", [{ transformerId: "json-format" }]);

    expect(result.output).toBe("");
    expect(result.error).toMatchObject({
      code: "type_mismatch",
      stepIndex: 0,
      expectedTypes: ["json"],
      actualType: "text",
    });
    expect(JSON.stringify(result)).not.toContain("not-json");
    expect(pipelineCompatibility("text", "json-format").compatible).toBe(false);
    expect(pipelineCompatibility("json", "json-format").compatible).toBe(true);
  });

  it("prevents unknown stages, overlong pipelines, and oversized input", () => {
    const unknown = runPipeline("safe", "text", [{ transformerId: "run-shell" }]);
    expect(unknown.error?.code).toBe("unknown_transformer");

    const tooMany = runPipeline(
      "safe",
      "text",
      Array.from({ length: PIPELINE_LIMITS.maxSteps + 1 }, () => ({ transformerId: "case" })),
    );
    expect(tooMany.error?.code).toBe("too_many_steps");

    const oversized = runPipeline("x".repeat(PIPELINE_LIMITS.maxInputBytes + 1), "text", []);
    expect(oversized.error?.code).toBe("input_too_large");
    expect(oversized.output).toBe("");
  });

  it("fails closed for malformed runtime types instead of throwing or reflecting input", () => {
    const malformedStep = runPipeline("secret-value", "text", [null as never]);
    expect(malformedStep).toMatchObject({
      output: "",
      error: { code: "unknown_transformer", stepIndex: 0 },
    });
    expect(JSON.stringify(malformedStep)).not.toContain("secret-value");

    const invalidType = runPipeline("secret-value", "untrusted" as never, []);
    expect(invalidType).toMatchObject({ output: "", error: { code: "invalid_input" } });
    expect(JSON.stringify(invalidType)).not.toContain("secret-value");
  });

  it("keeps binary byte conversion lossless without pretending it is text", () => {
    const result = runPipeline("00ff10", "hex", [{ transformerId: "hex-to-base64" }]);

    expect(result.error).toBeNull();
    expect(result.outputType).toBe("base64");
    expect(result.output).toBe("AP8Q");
  });

  it("computes the next type without running or leaking a stage", () => {
    expect(pipelineOutputType("text", [{ transformerId: "base64-encode" }])).toBe("base64");
    expect(pipelineOutputType("text", [{ transformerId: "json-format" }])).toBe("text");
  });
});
