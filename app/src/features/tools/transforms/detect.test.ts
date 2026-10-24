import { describe, expect, it } from "vitest";
import { detect, detectSmartInput, SMART_DETECTION_LIMITS } from "./detect";

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

it("adds ordered Unix time, UUID and encoded URL suggestions without probing resources", () => {
  expect(detect("1700000000")[0]).toBe("timestamp");
  expect(detect("1700000000000")[0]).toBe("timestamp");
  expect(detect("550e8400-e29b-41d4-a716-446655440000")[0]).toBe("uuid");
  expect(detect("hello%20world")[0]).toBe("url-decode");
  expect(detect("deadbeef")).toEqual(["byte-codec"]);
});
it("bounds raw UTF-8 input and suppresses encoded credential URLs and paths", () => {
  for (const input of [
    "/etc/passwd",
    "../secret",
    "file:///tmp/secret",
    "https%3A%2F%2Fuser%3Apass%40example.test",
    "https%3A%2F%2Fexample.test%2F%3Ftoken%3Dx",
    "bad\ud800",
  ]) {
    expect(detect(input)).toEqual([]);
  }
  expect(detect("한".repeat(350000))).toEqual([]);
});
