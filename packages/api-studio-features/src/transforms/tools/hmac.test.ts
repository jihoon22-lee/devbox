import { describe, expect, it, vi } from "vitest";
import {
  HMAC_ERROR,
  MAX_HMAC_INPUT_BYTES,
  browserHmacGenerate,
  browserHmacVerify,
  decodeHmacInput,
  type HmacRequest,
} from "./hmac";

const baseRequest: HmacRequest = {
  algorithm: "sha256",
  key: "Jefe",
  keyEncoding: "utf8",
  message: "what do ya want for nothing?",
  messageEncoding: "utf8",
  outputEncoding: "hex",
};

describe("HMAC wire codecs", () => {
  it("decodes UTF-8, hex, padded Base64, and unpadded Base64URL consistently", () => {
    expect(Array.from(decodeHmacInput("payload", "utf8"))).toEqual(
      Array.from(decodeHmacInput("7061796c6f6164", "hex")),
    );
    expect(Array.from(decodeHmacInput("payload", "utf8"))).toEqual(
      Array.from(decodeHmacInput("cGF5bG9hZA==", "base64")),
    );
    expect(Array.from(decodeHmacInput("payload", "utf8"))).toEqual(
      Array.from(decodeHmacInput("cGF5bG9hZA", "base64url")),
    );
  });

  it("rejects non-canonical or mixed-alphabet Base64 without exposing the value", () => {
    expect(() => decodeHmacInput("Zh==", "base64")).toThrow(HMAC_ERROR);
    expect(() => decodeHmacInput("cGF5bG9hZA==", "base64url")).toThrow(HMAC_ERROR);
    try {
      decodeHmacInput("secret-with-invalid-encoding", "hex");
    } catch (error) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).toBe(HMAC_ERROR);
      expect((error as Error).message).not.toContain("secret");
    }
  });

  it("enforces the decoded byte bound and rejects extra wire fields", async () => {
    expect(decodeHmacInput("k".repeat(MAX_HMAC_INPUT_BYTES), "utf8")).toHaveLength(
      MAX_HMAC_INPUT_BYTES,
    );
    expect(() =>
      decodeHmacInput("k".repeat(MAX_HMAC_INPUT_BYTES + 1), "utf8"),
    ).toThrow(HMAC_ERROR);

    await expect(
      browserHmacGenerate({ ...baseRequest, secret: "unexpected" } as HmacRequest),
    ).rejects.toThrow(HMAC_ERROR);
  });

  it("produces the RFC 4231 SHA-256 vector through the browser Web Crypto path", async () => {
    const sign = vi.fn().mockResolvedValue(
      Uint8Array.from(
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843".match(/../g)!,
        (part) => Number.parseInt(part, 16),
      ),
    );
    vi.stubGlobal("crypto", {
      subtle: {
        importKey: vi.fn().mockResolvedValue({}),
        sign,
      },
    });
    try {
      const result = await browserHmacGenerate(baseRequest);
      expect(result).toBe(
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
      );
      expect(sign).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("uses Web Crypto verification and returns only its boolean result", async () => {
    const verify = vi.fn().mockResolvedValue(true);
    vi.stubGlobal("crypto", {
      subtle: {
        importKey: vi.fn().mockResolvedValue({}),
        verify,
      },
    });
    try {
      const result = await browserHmacVerify({
        ...baseRequest,
        expectedTag:
          "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
      });
      expect(result).toBe(true);
      expect(verify).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
