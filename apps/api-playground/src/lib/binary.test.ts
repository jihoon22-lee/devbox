import { describe, expect, it } from "vitest";
import { isBinaryResponse, MAX_BINARY_PREVIEW_BYTES, projectBinaryResponse } from "./binary";

describe("binary response projection", () => {
  it("classifies media types and invalid UTF-8 conservatively", () => {
    expect(isBinaryResponse("application/octet-stream", new Uint8Array([1, 2]))).toBe(true);
    expect(isBinaryResponse("text/plain", new TextEncoder().encode("hello"))).toBe(false);
    expect(isBinaryResponse("text/plain", new Uint8Array([0xff, 0xfe]))).toBe(true);
    expect(isBinaryResponse("text/plain", new Uint8Array([0, 1, 2]))).toBe(true);
  });

  it("returns bounded hex/text metadata without returning raw bytes", () => {
    const projection = projectBinaryResponse("application/octet-stream", new TextEncoder().encode("hello"));
    expect(projection.size_bytes).toBe(5);
    expect(projection.hex_preview).toBe("68656c6c6f");
    expect(projection.text_preview).toBe("hello");
    expect(projection.save_available).toBe(false);
    expect("bytes" in projection).toBe(false);
  });

  it("masks a secret before exposing text or hex and bounds previews", () => {
    const payload = new TextEncoder().encode(`prefix-secret-${"x".repeat(MAX_BINARY_PREVIEW_BYTES)}`);
    const projection = projectBinaryResponse("application/octet-stream", payload, (value) => value.replace("secret", "[REDACTED]"));
    expect(projection.hex_preview).toBe("[REDACTED]");
    expect(projection.text_preview).toContain("[REDACTED]");
    expect(projection.hex_truncated).toBe(true);
    expect(projection.text_truncated).toBe(true);
    expect(projection.text_preview).not.toContain("secret-");
  });
});
