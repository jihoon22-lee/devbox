/**
 * Browser-side half of the HMAC contract.
 *
 * Web Crypto supplies the HMAC primitive; the small codecs below only decode
 * the explicit wire formats and never persist or log the key/message.
 */

export const HMAC_ERROR = "HMAC 입력을 처리할 수 없습니다.";
export const MAX_HMAC_INPUT_BYTES = 1_000_000;
export const MAX_HMAC_TEXT_BYTES = 2_100_000;
export const MAX_HMAC_OUTPUT_CHARS = 128;

export type HmacAlgorithm = "sha256" | "sha384" | "sha512";
export type HmacInputEncoding = "utf8" | "hex" | "base64" | "base64url";
export type HmacOutputEncoding = "hex" | "base64" | "base64url";

export interface HmacRequest {
  algorithm: HmacAlgorithm;
  key: string;
  keyEncoding: HmacInputEncoding;
  message: string;
  messageEncoding: HmacInputEncoding;
  outputEncoding: HmacOutputEncoding;
}

export interface HmacVerifyRequest extends HmacRequest {
  expectedTag: string;
}

interface PreparedRequest {
  algorithm: HmacAlgorithm;
  key: Uint8Array;
  message: Uint8Array;
  outputEncoding: HmacOutputEncoding;
}

const GENERATE_REQUEST_KEYS = [
  "algorithm",
  "key",
  "keyEncoding",
  "message",
  "messageEncoding",
  "outputEncoding",
] as const;
const VERIFY_REQUEST_KEYS = [...GENERATE_REQUEST_KEYS, "expectedTag"] as const;

const STANDARD_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

function fail(): never {
  throw new Error(HMAC_ERROR);
}

function assertString(value: unknown): asserts value is string {
  if (typeof value !== "string") fail();
}

function assertTextBound(value: string): void {
  if (new TextEncoder().encode(value).byteLength > MAX_HMAC_TEXT_BYTES) fail();
}

function assertInputBound(length: number): void {
  if (length > MAX_HMAC_INPUT_BYTES) fail();
}

function assertExactOwnKeys(
  request: object,
  expected: readonly string[],
): void {
  const actual = Object.keys(request).sort();
  const sortedExpected = [...expected].sort();
  if (
    actual.length !== sortedExpected.length ||
    actual.some((key, index) => key !== sortedExpected[index])
  ) {
    fail();
  }
}

function parseAlgorithm(value: unknown): HmacAlgorithm {
  if (value === "sha256" || value === "sha384" || value === "sha512") return value;
  fail();
}

function parseInputEncoding(value: unknown): HmacInputEncoding {
  if (value === "utf8" || value === "hex" || value === "base64" || value === "base64url") {
    return value;
  }
  fail();
}

function parseOutputEncoding(value: unknown): HmacOutputEncoding {
  if (value === "hex" || value === "base64" || value === "base64url") return value;
  fail();
}

/** Decode one key/message field according to the exact native wire contract. */
export function decodeHmacInput(value: string, encoding: HmacInputEncoding): Uint8Array {
  assertString(value);
  assertTextBound(value);
  switch (encoding) {
    case "utf8": {
      const bytes = new TextEncoder().encode(value);
      assertInputBound(bytes.byteLength);
      return bytes;
    }
    case "hex":
      return decodeHex(value, MAX_HMAC_INPUT_BYTES);
    case "base64":
      return decodeBase64(value, false, MAX_HMAC_INPUT_BYTES);
    case "base64url":
      return decodeBase64(value, true, MAX_HMAC_INPUT_BYTES);
    default:
      fail();
  }
}

function decodeHex(value: string, maxBytes: number): Uint8Array {
  if (value.length % 2 !== 0 || value.length / 2 > maxBytes) fail();
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < value.length; index += 2) {
    const high = hexDigit(value.charCodeAt(index));
    const low = hexDigit(value.charCodeAt(index + 1));
    if (high < 0 || low < 0) fail();
    bytes[index / 2] = (high << 4) | low;
  }
  return bytes;
}

function hexDigit(value: number): number {
  if (value >= 0x30 && value <= 0x39) return value - 0x30;
  if (value >= 0x41 && value <= 0x46) return value - 0x41 + 10;
  if (value >= 0x61 && value <= 0x66) return value - 0x61 + 10;
  return -1;
}

function decodeBase64(
  value: string,
  urlSafe: boolean,
  maxBytes: number,
): Uint8Array {
  const alphabet = urlSafe ? URL_ALPHABET : STANDARD_ALPHABET;
  if (urlSafe) {
    if (value.length % 4 === 1 || value.includes("=")) fail();
  } else {
    if (value.length % 4 !== 0) fail();
    const firstPadding = value.indexOf("=");
    if (firstPadding >= 0 && firstPadding < value.length - 2) fail();
    if (firstPadding >= 0 && value.length - firstPadding > 2) fail();
  }

  const paddedLength = urlSafe ? Math.ceil(value.length / 4) * 4 : value.length;
  const output = new Uint8Array(Math.floor((value.length * 3) / 4));
  let outputIndex = 0;
  for (let index = 0; index < paddedLength; index += 4) {
    const c1 = index < value.length ? alphabet.indexOf(value[index]) : -1;
    const c2 = index + 1 < value.length ? alphabet.indexOf(value[index + 1]) : -1;
    const c3 = index + 2 < value.length ? value[index + 2] : undefined;
    const c4 = index + 3 < value.length ? value[index + 3] : undefined;
    const v3 = c3 === undefined || c3 === "=" ? 64 : alphabet.indexOf(c3);
    const v4 = c4 === undefined || c4 === "=" ? 64 : alphabet.indexOf(c4);
    if (c1 < 0 || c2 < 0 || v3 < 0 || v4 < 0) fail();
    if (v3 === 64 && v4 !== 64) fail();
    if (v3 === 64 && index + 4 < paddedLength) fail();
    if (v4 === 64 && index + 4 < paddedLength) fail();
    const combined = (c1 << 18) | (c2 << 12) | ((v3 & 0x3f) << 6) | (v4 & 0x3f);
    if (outputIndex < output.length) output[outputIndex++] = (combined >> 16) & 0xff;
    if (v3 !== 64 && outputIndex < output.length) output[outputIndex++] = (combined >> 8) & 0xff;
    if (v4 !== 64 && outputIndex < output.length) output[outputIndex++] = combined & 0xff;
  }

  const decoded = output.slice(0, outputIndex);
  if (decoded.byteLength > maxBytes) fail();
  const canonical = encodeBase64(decoded, alphabet, !urlSafe);
  if (canonical !== value) fail();
  return decoded;
}

function encodeBase64(value: Uint8Array, alphabet: string, padded: boolean): string {
  let output = "";
  for (let index = 0; index < value.length; index += 3) {
    const first = value[index];
    const second = index + 1 < value.length ? value[index + 1] : 0;
    const third = index + 2 < value.length ? value[index + 2] : 0;
    output += alphabet[first >> 2];
    output += alphabet[((first & 0x03) << 4) | (second >> 4)];
    output += index + 1 < value.length ? alphabet[((second & 0x0f) << 2) | (third >> 6)] : "=";
    output += index + 2 < value.length ? alphabet[third & 0x3f] : "=";
  }
  return padded ? output : output.replace(/=+$/, "");
}

function encodeHex(value: Uint8Array): string {
  let output = "";
  for (const byte of value) output += byte.toString(16).padStart(2, "0");
  return output;
}

function encodeOutput(value: Uint8Array, encoding: HmacOutputEncoding): string {
  const output =
    encoding === "hex"
      ? encodeHex(value)
      : encodeBase64(value, encoding === "base64" ? STANDARD_ALPHABET : URL_ALPHABET, encoding === "base64");
  if (output.length > MAX_HMAC_OUTPUT_CHARS) fail();
  return output;
}

function prepareRequest(
  request: HmacRequest,
  verify = false,
): PreparedRequest {
  if (request === null || typeof request !== "object") fail();
  assertExactOwnKeys(request, verify ? VERIFY_REQUEST_KEYS : GENERATE_REQUEST_KEYS);
  const algorithm = parseAlgorithm(request.algorithm);
  const keyEncoding = parseInputEncoding(request.keyEncoding);
  const messageEncoding = parseInputEncoding(request.messageEncoding);
  const outputEncoding = parseOutputEncoding(request.outputEncoding);
  assertString(request.key);
  assertString(request.message);
  const key = decodeHmacInput(request.key, keyEncoding);
  const message = decodeHmacInput(request.message, messageEncoding);
  if (key.byteLength === 0) fail();
  return { algorithm, key, message, outputEncoding };
}

/** Validate the same bounds/encoding rules before a browser or native call. */
export function validateHmacRequest(request: HmacRequest): void {
  prepareRequest(request);
}

function cryptoAlgorithm(algorithm: HmacAlgorithm): HmacImportParams {
  return {
    name: "HMAC",
    hash: `SHA-${algorithm.slice(3)}`,
  };
}

/** Browser preview path; Web Crypto performs the HMAC operation offline. */
export async function browserHmacGenerate(request: HmacRequest): Promise<string> {
  const prepared = prepareRequest(request);
  try {
    const cryptoKey = await globalThis.crypto.subtle.importKey(
      "raw",
      prepared.key,
      cryptoAlgorithm(prepared.algorithm),
      false,
      ["sign"],
    );
    const tag = new Uint8Array(
      await globalThis.crypto.subtle.sign("HMAC", cryptoKey, prepared.message),
    );
    return encodeOutput(tag, prepared.outputEncoding);
  } catch {
    throw new Error(HMAC_ERROR);
  }
}

/** Browser preview path; Web Crypto's verify operation is constant-time. */
export async function browserHmacVerify(request: HmacVerifyRequest): Promise<boolean> {
  const prepared = prepareRequest(request, true);
  assertString(request.expectedTag);
  const expected = decodeHmacTag(request.expectedTag, prepared.outputEncoding);
  const expectedLength = prepared.algorithm === "sha256" ? 32 : prepared.algorithm === "sha384" ? 48 : 64;
  if (expected.byteLength !== expectedLength) return false;
  try {
    const cryptoKey = await globalThis.crypto.subtle.importKey(
      "raw",
      prepared.key,
      cryptoAlgorithm(prepared.algorithm),
      false,
      ["verify"],
    );
    return await globalThis.crypto.subtle.verify(
      "HMAC",
      cryptoKey,
      expected,
      prepared.message,
    );
  } catch {
    throw new Error(HMAC_ERROR);
  }
}

function decodeHmacTag(value: string, encoding: HmacOutputEncoding): Uint8Array {
  assertString(value);
  assertTextBound(value);
  if (value.length > MAX_HMAC_OUTPUT_CHARS) fail();
  switch (encoding) {
    case "hex":
      return decodeHex(value, 64);
    case "base64":
      return decodeBase64(value, false, 64);
    case "base64url":
      return decodeBase64(value, true, 64);
    default:
      fail();
  }
}
