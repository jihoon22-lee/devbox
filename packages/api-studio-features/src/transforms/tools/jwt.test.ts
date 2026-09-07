import { describe, expect, it } from "vitest";
import {
  browserVerifyJwt,
  decodeBase64Url,
  decodeJwtKey,
  formatJwtDisplay,
  JwtError,
  JWT_LIMITS,
  parseJwt,
  type JwtKeyEncoding,
  validateJwtTimes,
} from "./jwt";

const SIGNING_INPUT =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
const HS384_SIGNING_INPUT =
  "eyJhbGciOiJIUzM4NCIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
const HS512_SIGNING_INPUT =
  "eyJhbGciOiJIUzUxMiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
const SIGNATURE = "AL_nmexgcwawKDK5uJ0RtfAxT1GguksdPuaahEACpHc";
const TOKEN = `${SIGNING_INPUT}.${SIGNATURE}`;
const KEY_HEX = "3031323334353637383930313233343536373839303132333435363738393031";
const LONG_KEY_HEX = "30313233343536373839303132333435363738393031323334353637383930313031323334353637383930313233343536373839303132333435363738393031";

function encodeBase64UrlBytes(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function encodeBase64UrlText(value: string): string {
  return encodeBase64UrlBytes(new TextEncoder().encode(value));
}

function tokenFor(header: unknown, payload: unknown, signature = SIGNATURE): string {
  return `${encodeBase64UrlText(JSON.stringify(header))}.${encodeBase64UrlText(JSON.stringify(payload))}.${signature}`;
}

describe("JWT bounded decoder", () => {
  it("parses a known compact token without treating it as verified", () => {
    const parsed = parseJwt(TOKEN);
    expect(parsed.algorithm).toBe("HS256");
    expect(parsed.header.alg).toBe("HS256");
    expect((parsed.payload as Record<string, unknown>).sub).toBe("1234567890");
    expect(formatJwtDisplay(parsed)).toContain('"verification": "unverified"');
    expect(formatJwtDisplay(parsed)).not.toContain(SIGNATURE);
  });

  it.each(["none", "RS256", "ES256", "hs256"])('rejects algorithm "%s"', (algorithm) => {
    expect(() => parseJwt(tokenFor({ alg: algorithm }, { sub: "safe" }))).toThrowError(JwtError);
  });

  it("rejects non-canonical base64url pad bits and padding", () => {
    expect(() => decodeBase64Url("Zh")).toThrowError(JwtError);
    expect(() => decodeBase64Url("Zg=")).toThrowError(JwtError);
    expect(() => parseJwt(`${SIGNING_INPUT}.=`)).toThrowError(JwtError);
  });

  it("rejects duplicate JSON keys in nested objects and header critical extensions", () => {
    const duplicateHeader = `${encodeBase64UrlText('{"alg":"HS256","alg":"HS512"}')}.${SIGNING_INPUT.split(".")[1]}.${SIGNATURE}`;
    expect(() => parseJwt(duplicateHeader)).toThrowError(JwtError);

    const critical = tokenFor({ alg: "HS256", crit: ["b64"] }, { safe: true });
    expect(() => parseJwt(critical)).toThrowError(JwtError);

    const nestedDuplicate = `${SIGNING_INPUT.split(".")[0]}.${encodeBase64UrlText('{"nested":{"x":1,"x":2}}')}.${SIGNATURE}`;
    expect(() => parseJwt(nestedDuplicate)).toThrowError(JwtError);
  });

  it("rejects invalid UTF-8 and bounded JSON before rendering", () => {
    const invalidUtf8Header = encodeBase64UrlBytes(new Uint8Array([0x7b, 0x22, 0x61, 0x6c, 0x67, 0x22, 0x3a, 0x22, 0xc3, 0x28, 0x22, 0x7d]));
    expect(() => parseJwt(`${invalidUtf8Header}.${SIGNING_INPUT.split(".")[1]}.${SIGNATURE}`)).toThrowError(JwtError);

    const hugeString = "x".repeat(JWT_LIMITS.maxJsonStringBytes + 1);
    expect(() => parseJwt(tokenFor({ alg: "HS256" }, { hugeString }))).toThrowError(JwtError);
  });

  it("uses a fixed UTC clock-skew contract for exp, nbf, and iat", () => {
    const now = 1_700_000_000;
    expect(validateJwtTimes({ exp: now - JWT_LIMITS.clockSkewSeconds }, now).valid).toBe(true);
    expect(validateJwtTimes({ exp: now - JWT_LIMITS.clockSkewSeconds - 1 }, now).valid).toBe(false);
    expect(validateJwtTimes({ nbf: now + JWT_LIMITS.clockSkewSeconds }, now).valid).toBe(true);
    expect(validateJwtTimes({ nbf: now + JWT_LIMITS.clockSkewSeconds + 1 }, now).valid).toBe(false);
    expect(validateJwtTimes({ iat: now + JWT_LIMITS.clockSkewSeconds + 1 }, now).valid).toBe(false);
    expect(() => validateJwtTimes({ exp: "not-a-number" }, now)).toThrowError(JwtError);
  });

  it("does not format an unbounded verification timestamp", () => {
    const parsed = parseJwt(TOKEN);
    expect(() => formatJwtDisplay(parsed, {
      status: "verified",
      verifiedAtSeconds: Number.POSITIVE_INFINITY,
    })).toThrowError(JwtError);
  });
});

describe("JWT key and browser verification boundary", () => {
  it("accepts only explicit key encodings and enforces byte limits", () => {
    expect(decodeJwtKey("01234567890123456789012345678901", "utf8")).toHaveLength(32);
    expect(decodeJwtKey(KEY_HEX, "hex")).toHaveLength(32);
    expect(decodeJwtKey("MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE=", "base64")).toHaveLength(32);
    expect(decodeJwtKey("MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE", "base64url")).toHaveLength(32);
    expect(() => decodeJwtKey("short", "utf8")).not.toThrow();
    expect(() => decodeJwtKey("-----BEGIN PUBLIC KEY-----", "pem" as JwtKeyEncoding)).toThrowError(JwtError);
    expect(() => decodeJwtKey("not-base64", "base64")).toThrowError(JwtError);
    expect(() => decodeJwtKey("x".repeat(JWT_LIMITS.maxKeyTextBytes + 1), "utf8")).toThrowError(JwtError);
  });

  it("verifies the known HS256 vector through Web Crypto and never returns a tag", async () => {
    const valid = await browserVerifyJwt({
      algorithm: "HS256",
      signingInput: SIGNING_INPUT,
      signature: SIGNATURE,
      key: KEY_HEX,
      keyEncoding: "hex",
    });
    expect(valid).toBe(true);

    const invalid = await browserVerifyJwt({
      algorithm: "HS256",
      signingInput: SIGNING_INPUT,
      signature: `${SIGNATURE.slice(0, -1)}A`,
      key: KEY_HEX,
      keyEncoding: "hex",
    });
    expect(invalid).toBe(false);
  });

  it("rejects invalid time claims at the direct browser verification boundary", async () => {
    const expired = tokenFor({ alg: "HS256" }, { exp: 0 }).split(".");
    await expect(browserVerifyJwt({
      algorithm: "HS256",
      signingInput: `${expired[0]}.${expired[1]}`,
      signature: SIGNATURE,
      key: KEY_HEX,
      keyEncoding: "hex",
    })).rejects.toMatchObject({ code: "invalid_claims" });
  });

  it.each([
    ["HS384", HS384_SIGNING_INPUT, "58Hc1lXLsSwvo-Mor4Son_yMVfSf4OA5qsVBjYpWacUeSlLSMVjLgTZ-rk5ORQrr"],
    ["HS512", HS512_SIGNING_INPUT, "Ck5IG3CaU-sZxfd1TzD9VxRVRbNb45Hv5mO0wzo8cJlVFKgUhVH8ofN1XBNgpq8J9kzS7zfDLKXA-y9bjc4EBw"],
  ] as const)("supports the %s allow-listed browser primitive", async (algorithm, signingInput, signature) => {
    await expect(browserVerifyJwt({
      algorithm,
      signingInput,
      signature,
      key: LONG_KEY_HEX,
      keyEncoding: "hex",
    })).resolves.toBe(true);
  });
});
