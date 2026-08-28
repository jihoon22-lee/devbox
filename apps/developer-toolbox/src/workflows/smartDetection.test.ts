import { describe, expect, it } from "vitest";
import {
  detectSmartInput,
  SMART_DETECTION_LIMITS,
} from "./smartDetection";

const JWT =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.AL_nmexgcwawKDK5uJ0RtfAxT1GguksdPuaahEACpHc";

describe("smart detection (#340)", () => {
  it("detects structured JSON and recommends the bounded formatter", () => {
    const result = detectSmartInput('{"users":[{"id":1}]}');

    expect(result.status).toBe("detected");
    expect(result.recommendedTransformerId).toBe("json-format");
    expect(result.candidates[0]).toMatchObject({
      kind: "json",
      transformerId: "json-format",
      toolId: "json-format",
    });
  });

  it("detects JWT without verifying or exposing its signature", () => {
    const result = detectSmartInput(JWT);
    const serialized = JSON.stringify(result);

    expect(result.recommendedTransformerId).toBe("jwt-decode");
    expect(result.sensitive).toBe(true);
    expect(serialized).not.toContain(JWT);
    expect(serialized).not.toContain("AL_nmex");
  });

  it("keeps plain Base64 ambiguity explicit when standard and URL alphabets overlap", () => {
    const result = detectSmartInput("Zm9v");

    expect(result.status).toBe("ambiguous");
    expect(result.recommendedTransformerId).toBeNull();
    expect(result.candidates.map((candidate) => candidate.kind)).toEqual(["base64", "base64url"]);
  });

  it("keeps candidate ordering and count deterministic across repeated detection", () => {
    const input = "deadbeef";
    const first = detectSmartInput(input);
    const second = detectSmartInput(input);

    expect(first).toEqual(second);
    expect(first.candidates.length).toBeLessThanOrEqual(SMART_DETECTION_LIMITS.maxCandidates);
    expect(first.candidates.map((candidate) => candidate.kind)).toEqual(["hex", "base64", "base64url"]);
  });

  it("detects binary hex and chooses a lossless raw-byte transformer", () => {
    const result = detectSmartInput("00ff10");

    expect(result.status).toBe("detected");
    expect(result.recommendedTransformerId).toBe("hex-to-base64");
    expect(result.candidates[0]?.toolId).toBe("byte-codec");
  });

  it("rejects invalid values, unsafe paths, credential URLs, and oversized input", () => {
    expect(detectSmartInput("not a supported value").status).toBe("unsupported");
    expect(detectSmartInput("C:\\private\\credential.txt").status).toBe("unsupported");
    expect(detectSmartInput("https://user:password@example.test/private").status).toBe("unsupported");
    expect(detectSmartInput("https://example.test/?api_key=secret").status).toBe("unsupported");
    expect(detectSmartInput("https://example.test/?redirect=Bearer%20secret-value").status).toBe("unsupported");

    const oversized = detectSmartInput("x".repeat(SMART_DETECTION_LIMITS.maxInputBytes + 1));
    expect(oversized.status).toBe("too_large");
    expect(oversized.candidates).toEqual([]);

    const tooManyJsonNodes = `[${Array.from({ length: 100_001 }, () => "0").join(",")}]`;
    expect(detectSmartInput(tooManyJsonNodes).status).toBe("unsupported");
  });

  it("marks credential-shaped JSON without reflecting the value", () => {
    const result = detectSmartInput('{"password":"super-secret-value"}');
    const serialized = JSON.stringify(result);

    expect(result.sensitive).toBe(true);
    expect(result.candidates[0]?.reason).toContain("저장하거나 전송하지 않습니다");
    expect(serialized).not.toContain("super-secret-value");
    expect(detectSmartInput("Bearer super-secret-value").status).toBe("unsupported");
    expect(detectSmartInput("Basic c2VjcmV0OnBhc3M=").status).toBe("unsupported");
  });
});
