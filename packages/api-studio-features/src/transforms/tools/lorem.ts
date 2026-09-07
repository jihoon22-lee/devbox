/**
 * Deterministic, offline Lorem Ipsum generation for the Developer Toolbox.
 *
 * The generator deliberately owns no browser, clipboard, filesystem, network,
 * random, or clock dependency.  Keeping the corpus and count semantics here
 * makes the native WebView and browser preview use the same byte-for-byte
 * result and fixed validation errors.
 */

export type LoremUnit = "paragraphs" | "sentences" | "words";

export interface LoremOptions {
  unit: LoremUnit;
  count: number;
}

export type LoremErrorCode = "INVALID_UNIT" | "INVALID_COUNT" | "OUTPUT_TOO_LARGE";

export interface LoremError {
  code: LoremErrorCode;
  message: string;
}

export interface LoremResult {
  output: string;
  error: LoremError | null;
  unitCount: number;
  byteLength: number;
}

/** Maximum amount of one requested unit. */
export const MAX_LOREM_COUNT = 100;
/** The count field is intentionally a bounded decimal field, not a number parser. */
export const MAX_LOREM_COUNT_DIGITS = 3;
/** Maximum UTF-8 bytes in a generated result kept in the in-memory tool. */
export const MAX_LOREM_OUTPUT_BYTES = 64 * 1024;
/** Maximum UTF-8 bytes for the count token inserted by the explicit Paste action. */
export const MAX_LOREM_COUNT_INPUT_BYTES = MAX_LOREM_COUNT_DIGITS;

const SENTENCES = [
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
  "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
  "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.",
  "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.",
  "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.",
] as const;

const WORDS = SENTENCES.join(" ").split(/\s+/u);

const ERROR_MESSAGES: Readonly<Record<LoremErrorCode, string>> = Object.freeze({
  INVALID_UNIT: "지원하지 않는 Lorem 분량 단위입니다.",
  INVALID_COUNT: `Lorem 분량은 1에서 ${MAX_LOREM_COUNT} 사이의 정수여야 합니다.`,
  OUTPUT_TOO_LARGE: "Lorem 결과가 안전한 출력 크기 제한을 초과합니다.",
});

const UTF8_ENCODER = new TextEncoder();

function utf8ByteLength(value: string): number {
  return UTF8_ENCODER.encode(value).byteLength;
}

function emptyResult(error: LoremError | null = null): LoremResult {
  return { output: "", error, unitCount: 0, byteLength: 0 };
}

function isLoremUnit(value: unknown): value is LoremUnit {
  return value === "paragraphs" || value === "sentences" || value === "words";
}

function isValidCount(value: unknown): value is number {
  return (
    typeof value === "number"
    && Number.isSafeInteger(value)
    && value >= 1
    && value <= MAX_LOREM_COUNT
  );
}

function sentenceAt(index: number): string {
  return SENTENCES[index % SENTENCES.length];
}

function makeSentences(count: number): string {
  return Array.from({ length: count }, (_, index) => sentenceAt(index)).join(" ");
}

function makeParagraphs(count: number): string {
  return Array.from({ length: count }, (_, paragraphIndex) => {
    // Rotate the fixed corpus between paragraphs for visual variety while
    // preserving deterministic output and exactly five sentences per paragraph.
    const sentences = Array.from(
      { length: SENTENCES.length },
      (_, offset) => sentenceAt(paragraphIndex * 2 + offset),
    );
    return sentences.join(" ");
  }).join("\n\n");
}

function makeWords(count: number): string {
  return Array.from({ length: count }, (_, index) => WORDS[index % WORDS.length]).join(" ");
}

/**
 * Generate reproducible placeholder text from the bundled corpus.
 * Invalid runtime callers receive a fixed, empty result rather than an
 * exception containing input data.
 */
export function generateLorem(options: LoremOptions): LoremResult {
  if (!options || typeof options !== "object" || !isLoremUnit(options.unit)) {
    return emptyResult({ code: "INVALID_UNIT", message: ERROR_MESSAGES.INVALID_UNIT });
  }
  if (!isValidCount(options.count)) {
    return emptyResult({ code: "INVALID_COUNT", message: ERROR_MESSAGES.INVALID_COUNT });
  }

  const output = options.unit === "paragraphs"
    ? makeParagraphs(options.count)
    : options.unit === "sentences"
      ? makeSentences(options.count)
      : makeWords(options.count);
  const byteLength = utf8ByteLength(output);
  if (byteLength > MAX_LOREM_OUTPUT_BYTES) {
    return emptyResult({ code: "OUTPUT_TOO_LARGE", message: ERROR_MESSAGES.OUTPUT_TOO_LARGE });
  }

  return { output, error: null, unitCount: options.count, byteLength };
}

/** Parse only a bounded decimal count; exponent, sign, fraction, and infinity are rejected. */
export function parseLoremCount(value: string): number | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (trimmed.length < 1 || trimmed.length > MAX_LOREM_COUNT_DIGITS) return null;
  if (utf8ByteLength(trimmed) > MAX_LOREM_COUNT_INPUT_BYTES || !/^\d+$/u.test(trimmed)) {
    return null;
  }
  const count = Number(trimmed);
  return isValidCount(count) ? count : null;
}

export const LOREM_ERROR_MESSAGES = ERROR_MESSAGES;
