import { type Node, type ParseError, parseTree } from "jsonc-parser";
import {
  JWT_LIMITS,
  type JwtAlgorithm,
  type JwtDisplayOptions,
  JwtError,
  type JwtErrorCode,
  type JwtKeyEncoding,
  type JwtTemporalClaim,
  type JwtTransformResult,
  type JwtVerifyRequest,
  type ParsedJwt,
} from "./jwtTypes";

export * from "./jwtTypes";

const UTF8_ENCODER = new TextEncoder();
const ALGORITHM_TAG_LENGTH: Readonly<Record<JwtAlgorithm, number>> = Object.freeze({
  HS256: 32,
  HS384: 48,
  HS512: 64,
});
const CRITICAL_HEADER_NAMES = new Set(["alg", "typ", "kid", "cty"]);
const TEMPORAL_CLAIMS = ["exp", "nbf", "iat"] as const;

function error(code: JwtErrorCode): JwtError {
  return new JwtError(code);
}

function utf8ByteLength(value: string): number {
  return UTF8_ENCODER.encode(value).byteLength;
}

/** Shared UI guard; one bounded encode avoids UTF-16 length/UTF-8 byte drift. */
export function jwtUtf8ByteLength(value: string): number {
  return utf8ByteLength(value);
}

function assertWellFormedUnicode(value: string, code: JwtErrorCode): void {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw error(code);
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      throw error(code);
    }
  }
}

function decodeUtf8(bytes: Uint8Array): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw error("invalid_utf8");
  }
}

function base64UrlValue(character: string): number {
  const code = character.charCodeAt(0);
  if (code >= 0x41 && code <= 0x5a) return code - 0x41;
  if (code >= 0x61 && code <= 0x7a) return code - 0x61 + 26;
  if (code >= 0x30 && code <= 0x39) return code - 0x30 + 52;
  if (character === "-") return 62;
  if (character === "_") return 63;
  return -1;
}

/** Decode only unpadded RFC 4648 base64url and reject non-zero pad bits. */
export function decodeBase64Url(value: string, allowEmpty = false): Uint8Array {
  if (
    typeof value !== "string" ||
    utf8ByteLength(value) > JWT_LIMITS.maxKeyTextBytes ||
    (!allowEmpty && value.length === 0)
  ) {
    throw error("invalid_base64url");
  }
  if (value.length % 4 === 1 || !/^[A-Za-z0-9_-]*$/.test(value)) {
    throw error("invalid_base64url");
  }
  const remainder = value.length % 4;
  if (remainder === 2 && (base64UrlValue(value[value.length - 1]!) & 0x0f) !== 0) {
    throw error("invalid_base64url");
  }
  if (remainder === 3 && (base64UrlValue(value[value.length - 1]!) & 0x03) !== 0) {
    throw error("invalid_base64url");
  }

  const output = new Uint8Array(Math.floor((value.length * 6) / 8));
  let buffer = 0;
  let bits = 0;
  let offset = 0;
  for (const character of value) {
    buffer = (buffer << 6) | base64UrlValue(character);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      output[offset] = (buffer >> bits) & 0xff;
      offset += 1;
      buffer &= bits === 0 ? 0 : (1 << bits) - 1;
    }
  }
  return output;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function decodeBase64(value: string): Uint8Array {
  if (value.length === 0 || value.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(value)) {
    throw error("invalid_base64");
  }
  try {
    const binary = atob(value);
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    if (bytesToBase64(bytes) !== value) throw error("invalid_base64");
    return bytes;
  } catch (caught) {
    if (caught instanceof JwtError) throw caught;
    throw error("invalid_base64");
  }
}

function decodeHex(value: string): Uint8Array {
  if (value.length === 0 || value.length % 2 !== 0 || !/^[0-9A-Fa-f]+$/.test(value)) {
    throw error("invalid_key");
  }
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < value.length; index += 2) {
    bytes[index / 2] = Number.parseInt(value.slice(index, index + 2), 16);
  }
  return bytes;
}

function ensureKeyBounds(value: string, bytes: Uint8Array): Uint8Array {
  if (utf8ByteLength(value) > JWT_LIMITS.maxKeyTextBytes || bytes.length === 0) {
    throw error("invalid_key");
  }
  if (bytes.length > JWT_LIMITS.maxKeyBytes) throw error("invalid_key");
  return bytes;
}

/** Decode the explicit raw/hex/Base64/Base64URL key formats. */
export function decodeJwtKey(value: string, encoding: JwtKeyEncoding): Uint8Array {
  if (typeof value !== "string") throw error("invalid_key");
  if (utf8ByteLength(value) > JWT_LIMITS.maxKeyTextBytes) throw error("invalid_key");
  switch (encoding) {
    case "utf8":
      assertWellFormedUnicode(value, "invalid_key");
      return ensureKeyBounds(value, UTF8_ENCODER.encode(value));
    case "hex":
      return ensureKeyBounds(value, decodeHex(value));
    case "base64":
      return ensureKeyBounds(value, decodeBase64(value));
    case "base64url":
      return ensureKeyBounds(value, decodeBase64Url(value));
    default:
      throw error("invalid_key");
  }
}

export function jwtMinimumKeyBytes(algorithm: JwtAlgorithm): number {
  return ALGORITHM_TAG_LENGTH[algorithm];
}

function parseAlgorithm(value: unknown): JwtAlgorithm {
  if (value === "HS256" || value === "HS384" || value === "HS512") return value;
  // `none`, RSA, EC, casing variants, and every future algorithm are rejected
  // by the same allow-list.  There is no algorithm/key-type fallback.
  throw error("algorithm_not_allowed");
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

interface JsonStats {
  nodes: number;
}

function nodeToValue(node: Node, depth: number, stats: JsonStats): unknown {
  stats.nodes += 1;
  if (stats.nodes > JWT_LIMITS.maxJsonNodes || depth > JWT_LIMITS.maxJsonDepth) {
    throw error("json_bounds");
  }

  switch (node.type) {
    case "object": {
      const result = Object.create(null) as Record<string, unknown>;
      for (const property of node.children ?? []) {
        if (property.type !== "property" || !property.children || property.children.length !== 2) {
          throw error("invalid_json");
        }
        const keyNode = property.children[0]!;
        const valueNode = property.children[1]!;
        if (keyNode.type !== "string" || typeof keyNode.value !== "string") {
          throw error("invalid_json");
        }
        if (Object.prototype.hasOwnProperty.call(result, keyNode.value)) {
          throw error("duplicate_json_key");
        }
        if (utf8ByteLength(keyNode.value) > JWT_LIMITS.maxJsonStringBytes) {
          throw error("json_bounds");
        }
        stats.nodes += 1;
        if (stats.nodes > JWT_LIMITS.maxJsonNodes) throw error("json_bounds");
        Object.defineProperty(result, keyNode.value, {
          configurable: true,
          enumerable: true,
          value: nodeToValue(valueNode, depth + 1, stats),
          writable: true,
        });
      }
      return result;
    }
    case "array":
      return (node.children ?? []).map((child) => nodeToValue(child, depth + 1, stats));
    case "string":
      if (typeof node.value !== "string") throw error("invalid_json");
      if (utf8ByteLength(node.value) > JWT_LIMITS.maxJsonStringBytes) throw error("json_bounds");
      return node.value;
    case "number":
      if (
        typeof node.value !== "number" ||
        !Number.isFinite(node.value) ||
        (Number.isInteger(node.value) && !Number.isSafeInteger(node.value))
      ) {
        throw error("invalid_json");
      }
      return node.value;
    case "boolean":
      return node.value === true;
    case "null":
      return null;
    case "property":
      throw error("invalid_json");
    default:
      throw error("invalid_json");
  }
}

function parseBoundedJson(bytes: Uint8Array): { value: unknown; formatted: string } {
  if (bytes.length > JWT_LIMITS.maxJsonBytes) throw error("json_bounds");
  const text = decodeUtf8(bytes);
  const errors: ParseError[] = [];
  const tree = parseTree(text, errors, {
    allowTrailingComma: false,
    disallowComments: true,
  });
  if (!tree || errors.length > 0) throw error("invalid_json");
  const value = nodeToValue(tree, 0, { nodes: 0 });
  let formatted: string;
  try {
    formatted = JSON.stringify(value, null, 2);
  } catch {
    throw error("invalid_json");
  }
  if (formatted === undefined || utf8ByteLength(formatted) > JWT_LIMITS.maxOutputBytes) {
    throw error("json_bounds");
  }
  return { value, formatted };
}

function validateHeader(header: Record<string, unknown>): JwtAlgorithm {
  const algorithm = parseAlgorithm(header.alg);
  for (const name of ["typ", "kid", "cty"] as const) {
    if (name in header && typeof header[name] !== "string") throw error("invalid_header");
  }
  const critical = header.crit;
  if (critical !== undefined) {
    if (
      !Array.isArray(critical) ||
      critical.length > 8 ||
      critical.some((name) => typeof name !== "string" || name.length === 0)
    ) {
      throw error("invalid_header");
    }
    const seen = new Set<string>();
    for (const name of critical) {
      if (
        seen.has(name) ||
        name === "crit" ||
        !CRITICAL_HEADER_NAMES.has(name) ||
        !Object.prototype.hasOwnProperty.call(header, name)
      ) {
        throw error("invalid_header");
      }
      seen.add(name);
    }
  }
  return algorithm;
}

function assertTokenBounds(input: string): string {
  if (typeof input !== "string") throw error("invalid_input");
  if (utf8ByteLength(input) > JWT_LIMITS.maxTokenBytes) throw error("invalid_structure");
  assertWellFormedUnicode(input, "invalid_input");
  const token = input.trim();
  if (token.length === 0 || utf8ByteLength(token) > JWT_LIMITS.maxTokenBytes) {
    throw error("invalid_structure");
  }
  return token;
}

function decodeSegment(value: string): Uint8Array {
  if (value.length === 0 || utf8ByteLength(value) > JWT_LIMITS.maxSegmentBytes) {
    throw error("invalid_structure");
  }
  return decodeBase64Url(value);
}

function temporalClaims(payload: unknown): JwtTemporalClaim[] {
  if (!isObject(payload)) return [];
  return TEMPORAL_CLAIMS.filter((name) => Object.prototype.hasOwnProperty.call(payload, name)).map((name) => {
    const value = payload[name];
    const valid = typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 8_640_000_000_000;
    let iso8601: string | null = null;
    if (valid) {
      try {
        const date = new Date(value * 1000);
        if (!Number.isNaN(date.getTime())) iso8601 = date.toISOString();
      } catch {
        iso8601 = null;
      }
    }
    return {
      name,
      value: valid ? value : null,
      iso8601,
      valid,
    };
  });
}

function registeredClaimValues(payload: unknown): Partial<Record<(typeof TEMPORAL_CLAIMS)[number], number>> {
  if (!isObject(payload)) return {};
  const result: Partial<Record<(typeof TEMPORAL_CLAIMS)[number], number>> = {};
  for (const name of TEMPORAL_CLAIMS) {
    const value = payload[name];
    if (typeof value !== "number" || !Number.isFinite(value) || Math.abs(value) > 8_640_000_000_000) {
      if (Object.prototype.hasOwnProperty.call(payload, name)) throw error("invalid_claims");
      continue;
    }
    result[name] = value;
  }
  return result;
}

export interface JwtTemporalValidation {
  valid: boolean;
  claims: JwtTemporalClaim[];
}

/** Validate NumericDate claims using one captured UTC epoch and fixed ±60s skew. */
export function validateJwtTimes(
  payload: unknown,
  nowSeconds: number,
  clockSkewSeconds = JWT_LIMITS.clockSkewSeconds,
): JwtTemporalValidation {
  if (!Number.isFinite(nowSeconds) || !Number.isFinite(clockSkewSeconds) || clockSkewSeconds < 0) {
    throw error("invalid_claims");
  }
  const values = registeredClaimValues(payload);
  const claims = temporalClaims(payload);
  const valid =
    (values.exp === undefined || nowSeconds <= values.exp + clockSkewSeconds) &&
    (values.nbf === undefined || nowSeconds + clockSkewSeconds >= values.nbf) &&
    (values.iat === undefined || values.iat <= nowSeconds + clockSkewSeconds);
  return { valid, claims };
}

/** Parse and validate a compact JWT without doing signature verification. */
export function parseJwt(input: string): ParsedJwt {
  const token = assertTokenBounds(input);
  const segments = token.split(".");
  if (segments.length !== 3 || segments.some((segment) => segment.length === 0)) {
    throw error("invalid_structure");
  }
  const [headerSegment, payloadSegment, signatureSegment] = segments as [string, string, string];
  const headerBytes = decodeSegment(headerSegment);
  const payloadBytes = decodeSegment(payloadSegment);
  const parsedHeader = parseBoundedJson(headerBytes);
  if (!isObject(parsedHeader.value)) throw error("invalid_header");
  const algorithm = validateHeader(parsedHeader.value);
  const parsedPayload = parseBoundedJson(payloadBytes);
  const signatureBytes = decodeSegment(signatureSegment);
  if (
    signatureBytes.length !== ALGORITHM_TAG_LENGTH[algorithm] ||
    utf8ByteLength(signatureSegment) > JWT_LIMITS.maxSignatureTextBytes
  ) {
    throw error("invalid_signature");
  }
  return {
    algorithm,
    header: parsedHeader.value,
    payload: parsedPayload.value,
    headerJson: parsedHeader.formatted,
    payloadJson: parsedPayload.formatted,
    signingInput: `${headerSegment}.${payloadSegment}`,
    signature: signatureSegment,
    signatureBytes,
    temporalClaims: temporalClaims(parsedPayload.value),
  };
}

function outputJson(value: unknown): string {
  let output: string;
  try {
    output = JSON.stringify(value, null, 2);
  } catch {
    throw error("json_bounds");
  }
  if (output === undefined || utf8ByteLength(output) > JWT_LIMITS.maxOutputBytes) {
    throw error("json_bounds");
  }
  return output;
}

/** Format display data without ever including the compact signature or key. */
export function formatJwtDisplay(parsed: ParsedJwt, options: JwtDisplayOptions = {}): string {
  const status = options.status ?? "unverified";
  const display: Record<string, unknown> = {
    verification: status,
    algorithm: parsed.algorithm,
    header: parsed.header,
    payload: parsed.payload,
    temporalClaims: parsed.temporalClaims,
  };
  if (options.verifiedAtSeconds !== undefined) {
    if (!Number.isFinite(options.verifiedAtSeconds) || Math.abs(options.verifiedAtSeconds) > 8_640_000_000_000) {
      throw error("invalid_claims");
    }
    try {
      display.verificationTime = new Date(options.verifiedAtSeconds * 1000).toISOString();
    } catch {
      throw error("invalid_claims");
    }
    display.clockSkewSeconds = JWT_LIMITS.clockSkewSeconds;
  }
  return outputJson(display);
}

/** Backward-compatible transform shape used by the original decoder entry. */
export function decodeJwt(input: string): Promise<JwtTransformResult> {
  try {
    return Promise.resolve({ output: formatJwtDisplay(parseJwt(input)) });
  } catch (caught) {
    const safe = caught instanceof JwtError ? caught : error("invalid_input");
    return Promise.resolve({ output: "", error: safe.message, errorCode: safe.code });
  }
}

function validateNativeRequest(request: JwtVerifyRequest): {
  algorithm: JwtAlgorithm;
  key: Uint8Array;
  signature: Uint8Array;
  signingInput: string;
} {
  const algorithm = parseAlgorithm(request.algorithm);
  if (
    typeof request.signingInput !== "string" ||
    request.signingInput.length === 0 ||
    utf8ByteLength(request.signingInput) > JWT_LIMITS.maxTokenBytes ||
    !/^[\x21-\x7e]+\.[\x21-\x7e]+$/.test(request.signingInput)
  ) {
    throw error("invalid_structure");
  }
  const [header, payload] = request.signingInput.split(".") as [string, string];
  decodeSegment(header);
  decodeSegment(payload);
  if (typeof request.signature !== "string" || utf8ByteLength(request.signature) > JWT_LIMITS.maxSignatureTextBytes) {
    throw error("invalid_signature");
  }
  const signature = decodeBase64Url(request.signature);
  if (signature.length !== ALGORITHM_TAG_LENGTH[algorithm]) throw error("invalid_signature");
  // The algorithm used by the primitive must be the exact value carried in
  // the protected header.  This check is repeated here instead of relying on
  // the UI's parsed object so a direct browser API caller cannot create an
  // algorithm/key-type mismatch.
  const parsed = parseJwt(`${request.signingInput}.${request.signature}`);
  if (parsed.algorithm !== algorithm) throw error("algorithm_not_allowed");
  const temporal = validateJwtTimes(parsed.payload, Math.floor(Date.now() / 1000));
  if (!temporal.valid) throw error("invalid_claims");
  if (
    request.keyEncoding !== "utf8" &&
    request.keyEncoding !== "hex" &&
    request.keyEncoding !== "base64" &&
    request.keyEncoding !== "base64url"
  )
    throw error("invalid_key");
  const key = decodeJwtKey(request.key, request.keyEncoding);
  if (key.length < ALGORITHM_TAG_LENGTH[algorithm]) throw error("key_too_short");
  return { algorithm, key, signature, signingInput: request.signingInput };
}

/** Browser-only cryptographic fallback. Web Crypto is the primitive; no JS HMAC exists here. */
export async function browserVerifyJwt(request: JwtVerifyRequest): Promise<boolean> {
  const validated = validateNativeRequest(request);
  if (typeof crypto === "undefined" || !crypto.subtle) throw error("verification_unavailable");
  const hash = validated.algorithm.slice(2);
  try {
    const cryptoKey = await crypto.subtle.importKey(
      "raw",
      validated.key,
      { name: "HMAC", hash: { name: `SHA-${hash}` } },
      false,
      ["verify"],
    );
    return await crypto.subtle.verify(
      "HMAC",
      cryptoKey,
      validated.signature,
      UTF8_ENCODER.encode(validated.signingInput),
    );
  } catch {
    throw error("verification_failed");
  }
}

export function jwtErrorMessage(caught: unknown, fallback: JwtErrorCode = "verification_failed"): string {
  return caught instanceof JwtError ? caught.message : new JwtError(fallback).message;
}

export function jwtErrorCode(caught: unknown, fallback: JwtErrorCode = "verification_failed"): JwtErrorCode {
  return caught instanceof JwtError ? caught.code : fallback;
}
