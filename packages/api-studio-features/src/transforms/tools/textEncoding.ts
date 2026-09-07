import { decodeHTMLStrict } from "entities";

/**
 * Bounded, deterministic text codecs used by the HTML entity and URL
 * component tools.
 *
 * The browser has URL component primitives, but there is deliberately no
 * HTML parser in this module. HTML entity decoding uses a pinned strict codec
 * plus explicit validation so malformed input cannot be silently repaired by
 * a browser parser.
 */

export const TEXT_ENCODING_LIMITS = Object.freeze({
  maxInputBytes: 1_000_000,
  maxOutputBytes: 4_000_000,
  maxExpansionRatio: 16,
  maxEntityCount: 100_000,
  maxEntityTokenLength: 32,
  maxNumericEntityDigits: 7,
});

export type TextTransformErrorCode =
  | "invalid_input"
  | "invalid_unicode"
  | "input_too_large"
  | "output_too_large"
  | "malformed_url"
  | "malformed_entity"
  | "entity_limit"
  | "transform_failed";

const TEXT_TRANSFORM_ERROR_MESSAGES: Readonly<Record<TextTransformErrorCode, string>> = {
  invalid_input: "Text input is invalid.",
  invalid_unicode: "Text contains invalid Unicode.",
  input_too_large: "Input exceeds the 1,000,000-byte limit.",
  output_too_large: "Output exceeds the 4,000,000-byte safety limit.",
  malformed_url: "URL component contains malformed percent-encoding.",
  malformed_entity: "HTML entity contains malformed or unsupported syntax.",
  entity_limit: "HTML entity expansion exceeds the safety limit.",
  transform_failed: "Text transformation failed.",
};

/** Fixed-message error: input, credentials, paths, and platform diagnostics never enter the UI. */
export class TextTransformError extends Error {
  readonly code: TextTransformErrorCode;

  constructor(code: TextTransformErrorCode) {
    super(TEXT_TRANSFORM_ERROR_MESSAGES[code]);
    this.name = "TextTransformError";
    this.code = code;
  }
}

export interface TextTransformResult {
  output: string;
  error?: string;
  errorCode?: TextTransformErrorCode;
}

const HTML_ENTITY_ENCODINGS: Readonly<Record<string, string>> = Object.freeze({
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
});

const URL_COMPONENT_SAFE_ASCII = /^[A-Za-z0-9\-_.!~*'()]$/;
const HEX_DIGIT = /^[0-9A-Fa-f]$/;
const ENTITY_NAME = /^[A-Za-z][A-Za-z0-9]*$/;
const ENTITY_NUMERIC_DECIMAL = /^#[0-9]+$/;
const ENTITY_NUMERIC_HEX = /^#[xX][0-9A-Fa-f]+$/;
const UTF8_ENCODER = new TextEncoder();

function utf8ByteLength(value: string): number {
  return UTF8_ENCODER.encode(value).byteLength;
}

function assertStringInput(input: string): number {
  if (typeof input !== "string") throw new TextTransformError("invalid_input");
  // A UTF-8 string is never smaller than its UTF-16 code-unit count. This
  // rejects oversized strings before a full Unicode scan.
  if (input.length > TEXT_ENCODING_LIMITS.maxInputBytes) {
    throw new TextTransformError("input_too_large");
  }
  assertWellFormedUnicode(input);
  const inputBytes = utf8ByteLength(input);
  if (inputBytes > TEXT_ENCODING_LIMITS.maxInputBytes) {
    throw new TextTransformError("input_too_large");
  }
  return inputBytes;
}

/** TextEncoder replaces lone surrogates, which would make a false round-trip. Reject them. */
function assertWellFormedUnicode(input: string): void {
  for (let index = 0; index < input.length; index += 1) {
    const code = input.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = input.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) throw new TextTransformError("invalid_unicode");
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      throw new TextTransformError("invalid_unicode");
    }
  }
}

function assertOutputBounds(inputBytes: number, output: string): string {
  const outputBytes = utf8ByteLength(output);
  const allowedByExpansion = Math.min(
    TEXT_ENCODING_LIMITS.maxOutputBytes,
    Math.max(inputBytes, 1) * TEXT_ENCODING_LIMITS.maxExpansionRatio,
  );
  if (outputBytes > allowedByExpansion) throw new TextTransformError("output_too_large");
  return output;
}

function assertEstimatedOutputBounds(inputBytes: number, outputBytes: number): void {
  const allowedByExpansion = Math.min(
    TEXT_ENCODING_LIMITS.maxOutputBytes,
    Math.max(inputBytes, 1) * TEXT_ENCODING_LIMITS.maxExpansionRatio,
  );
  if (!Number.isSafeInteger(outputBytes) || outputBytes > allowedByExpansion) {
    throw new TextTransformError("output_too_large");
  }
}

function addOutputBytes(total: number, increment: number): number {
  const next = total + increment;
  if (!Number.isSafeInteger(next) || next > TEXT_ENCODING_LIMITS.maxOutputBytes) {
    throw new TextTransformError("output_too_large");
  }
  return next;
}

function utf8BytesForCodePoint(codePoint: number): number {
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

function estimateUrlEncodedBytes(input: string): number {
  let outputBytes = 0;
  for (let index = 0; index < input.length; index += 1) {
    const codePoint = input.codePointAt(index);
    if (codePoint === undefined) throw new TextTransformError("invalid_unicode");
    const character = String.fromCodePoint(codePoint);
    const increment = codePoint <= 0x7f && URL_COMPONENT_SAFE_ASCII.test(character)
      ? 1
      : utf8BytesForCodePoint(codePoint) * 3;
    outputBytes = addOutputBytes(outputBytes, increment);
    if (codePoint > 0xffff) index += 1;
  }
  return outputBytes;
}

/** Encode text for one URL component using the platform's standard URI primitive. */
export function urlComponentEncode(input: string): string {
  const inputBytes = assertStringInput(input);
  assertEstimatedOutputBounds(inputBytes, estimateUrlEncodedBytes(input));
  let output: string;
  try {
    output = encodeURIComponent(input);
  } catch {
    throw new TextTransformError("invalid_unicode");
  }
  return assertOutputBounds(inputBytes, output);
}

function assertPercentEncoding(input: string): void {
  for (let index = 0; index < input.length; index += 1) {
    if (input[index] !== "%") continue;
    if (
      index + 2 >= input.length ||
      !HEX_DIGIT.test(input[index + 1]) ||
      !HEX_DIGIT.test(input[index + 2])
    ) {
      throw new TextTransformError("malformed_url");
    }
    index += 2;
  }
}

/** Decode exactly one URL component; percent escapes and UTF-8 must both be valid. */
export function urlComponentDecode(input: string): string {
  const inputBytes = assertStringInput(input);
  assertPercentEncoding(input);
  let output: string;
  try {
    output = decodeURIComponent(input);
  } catch {
    throw new TextTransformError("malformed_url");
  }
  return assertOutputBounds(inputBytes, output);
}

function numericEntityValue(token: string): string | null {
  const isHex = ENTITY_NUMERIC_HEX.test(token);
  if (!isHex && !ENTITY_NUMERIC_DECIMAL.test(token)) return null;
  const digits = isHex ? token.slice(2) : token.slice(1);
  if (digits.length > TEXT_ENCODING_LIMITS.maxNumericEntityDigits) return null;
  const value = Number.parseInt(digits, isHex ? 16 : 10);
  if (
    !Number.isInteger(value) ||
    value <= 0 ||
    value > 0x10ffff ||
    (value >= 0xd800 && value <= 0xdfff)
  ) {
    return null;
  }
  return String.fromCodePoint(value);
}

function decodeEntityToken(token: string): string | null {
  if (token.length === 0 || token.length > TEXT_ENCODING_LIMITS.maxEntityTokenLength) return null;
  if (ENTITY_NAME.test(token)) {
    const encoded = `&${token};`;
    const decoded = decodeHTMLStrict(encoded);
    return decoded === encoded ? null : decoded;
  }
  return numericEntityValue(token);
}

/**
 * Decode semicolon-terminated standard named entities and validated numeric
 * points. A literal ampersand is retained only when it cannot begin an entity;
 * entity-looking malformed input fails closed.
 */
export function htmlEntityDecode(input: string): string {
  const inputBytes = assertStringInput(input);
  const output: string[] = [];
  let entityCount = 0;
  let outputBytes = 0;

  for (let index = 0; index < input.length; index += 1) {
    if (input[index] !== "&") {
      const codePoint = input.codePointAt(index);
      if (codePoint === undefined) throw new TextTransformError("invalid_unicode");
      const character = String.fromCodePoint(codePoint);
      output.push(character);
      outputBytes = addOutputBytes(outputBytes, utf8ByteLength(character));
      if (codePoint > 0xffff) index += 1;
      continue;
    }

    const next = input[index + 1] ?? "";
    if (!next || (!ENTITY_NAME.test(next) && next !== "#")) {
      output.push("&");
      outputBytes = addOutputBytes(outputBytes, 1);
      continue;
    }

    const semicolon = input.indexOf(";", index + 1);
    if (
      semicolon < 0 ||
      semicolon - index - 1 > TEXT_ENCODING_LIMITS.maxEntityTokenLength
    ) {
      throw new TextTransformError("malformed_entity");
    }
    const token = input.slice(index + 1, semicolon);
    const decoded = decodeEntityToken(token);
    if (decoded === null) throw new TextTransformError("malformed_entity");

    entityCount += 1;
    if (entityCount > TEXT_ENCODING_LIMITS.maxEntityCount) {
      throw new TextTransformError("entity_limit");
    }
    output.push(decoded);
    outputBytes = addOutputBytes(outputBytes, utf8ByteLength(decoded));
    index = semicolon;
  }

  if (outputBytes > TEXT_ENCODING_LIMITS.maxOutputBytes) {
    throw new TextTransformError("output_too_large");
  }
  return assertOutputBounds(inputBytes, output.join(""));
}

/** Encode only text-significant HTML characters; this is not an HTML parser or sanitizer. */
export function htmlEntityEncode(input: string): string {
  const inputBytes = assertStringInput(input);
  let estimatedBytes = 0;
  for (let index = 0; index < input.length; index += 1) {
    const codePoint = input.codePointAt(index);
    if (codePoint === undefined) throw new TextTransformError("invalid_unicode");
    const character = String.fromCodePoint(codePoint);
    const encoded = HTML_ENTITY_ENCODINGS[character];
    estimatedBytes = addOutputBytes(
      estimatedBytes,
      encoded === undefined ? utf8ByteLength(character) : encoded.length,
    );
    if (codePoint > 0xffff) index += 1;
  }
  assertEstimatedOutputBounds(inputBytes, estimatedBytes);

  const output = [] as string[];
  for (let index = 0; index < input.length; index += 1) {
    const codePoint = input.codePointAt(index);
    if (codePoint === undefined) throw new TextTransformError("invalid_unicode");
    const character = String.fromCodePoint(codePoint);
    output.push(HTML_ENTITY_ENCODINGS[character] ?? character);
    if (codePoint > 0xffff) index += 1;
  }
  return assertOutputBounds(inputBytes, output.join(""));
}

/** Convert a codec into the shape consumed by TransformerTool without reflecting raw errors. */
export function runTextTransform(
  transform: (input: string) => string,
  input: string,
): Promise<TextTransformResult> {
  try {
    return Promise.resolve({ output: transform(input) });
  } catch (error) {
    const safeError = error instanceof TextTransformError
      ? error
      : new TextTransformError("transform_failed");
    return Promise.resolve({
      output: "",
      error: safeError.message,
      errorCode: safeError.code,
    });
  }
}
