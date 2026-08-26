export type IdentifierKind = "uuid-v4" | "uuid-v7" | "ulid";

export interface IdentifierOptions {
  kind: IdentifierKind;
  count: number;
  uppercase: boolean;
  hyphens: boolean;
}

export const MAX_IDENTIFIER_BATCH = 100;
/** Safe, user-facing error used when the platform CSPRNG is unavailable. */
export const SECURE_RANDOM_ERROR = "암호학적으로 안전한 난수를 사용할 수 없습니다.";
/** Safe, user-facing error used when a monotonic suffix cannot be advanced. */
export const IDENTIFIER_SEQUENCE_ERROR = "식별자 생성 순서를 유지할 수 없습니다.";
/** Generic UI error; backend details are deliberately not reflected in the view. */
export const IDENTIFIER_GENERATION_ERROR = "식별자를 생성하지 못했습니다. 입력과 보안 난수 상태를 확인하세요.";

const CROCKFORD_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const MAX_TIMESTAMP = 0xffffffffffff;

/**
 * Generates identifiers for the browser preview. The Tauri application uses
 * the same contract through the Rust command, but keeping this fallback local
 * means the tool remains useful without a running Tauri host.
 *
 * UUID v4 has no ordering guarantee. UUID v7 and ULID are strictly increasing
 * within this one batch, including when the wall clock repeats or moves
 * backwards. Separate calls, processes, and machines do not share an order
 * guarantee.
 */
export function generateIdentifiers(options: IdentifierOptions): string[] {
  validateOptions(options);
  let previousUuidV7: Uint8Array | undefined;
  let previousUlid: Uint8Array | undefined;

  return Array.from({ length: options.count }, () => {
    const raw =
      options.kind === "uuid-v4"
        ? generateUuidV4()
        : options.kind === "uuid-v7"
          ? generateUuidV7(previousUuidV7)
          : generateUlid(previousUlid);
    if (options.kind === "uuid-v7") previousUuidV7 = raw;
    if (options.kind === "ulid") previousUlid = raw;
    return formatIdentifier(raw, options);
  });
}

export function validateOptions(options: IdentifierOptions): void {
  if (!options || typeof options !== "object") {
    throw new Error("식별자 생성 옵션이 올바르지 않습니다.");
  }
  if (!Number.isInteger(options.count) || options.count < 1 || options.count > MAX_IDENTIFIER_BATCH) {
    throw new Error(`생성 수량은 1에서 ${MAX_IDENTIFIER_BATCH} 사이여야 합니다.`);
  }
  if (!isIdentifierKind(options.kind)) {
    throw new Error("지원하지 않는 식별자 종류입니다.");
  }
  if (typeof options.uppercase !== "boolean" || typeof options.hyphens !== "boolean") {
    throw new Error("식별자 표시 옵션이 올바르지 않습니다.");
  }
}

function isIdentifierKind(value: unknown): value is IdentifierKind {
  return value === "uuid-v4" || value === "uuid-v7" || value === "ulid";
}

function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  try {
    // Keep lookup, feature detection and invocation in the same boundary:
    // hostile/test doubles can throw from a crypto getter as well as from
    // getRandomValues(). None of those platform details belong in the UI.
    const cryptoApi = globalThis.crypto;
    if (!cryptoApi || typeof cryptoApi.getRandomValues !== "function") {
      throw new Error(SECURE_RANDOM_ERROR);
    }
    cryptoApi.getRandomValues(bytes);
  } catch {
    // Never surface a platform-specific DOMException or fall back to a weak
    // PRNG. The native command maps its RNG panic to the same safe contract.
    throw new Error(SECURE_RANDOM_ERROR);
  }
  return bytes;
}

function generateUuidV4(): Uint8Array {
  const bytes = randomBytes(16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  return bytes;
}

function generateUuidV7(previous: Uint8Array | undefined): Uint8Array {
  const timestamp = currentTimestamp();
  if (previous && timestamp <= readTimestamp(previous)) {
    const bytes = previous.slice();
    if (incrementUuidV7Suffix(bytes)) return bytes;

    const nextTimestamp = readTimestamp(previous);
    if (nextTimestamp >= MAX_TIMESTAMP) throw new Error(IDENTIFIER_SEQUENCE_ERROR);
    return randomUuidV7(nextTimestamp + 1);
  }
  return randomUuidV7(timestamp);
}

function randomUuidV7(timestamp: number): Uint8Array {
  const bytes = randomBytes(16);
  writeTimestamp(bytes, timestamp);
  bytes[6] = (bytes[6] & 0x0f) | 0x70;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  return bytes;
}

function generateUlid(previous: Uint8Array | undefined): Uint8Array {
  const bytes = randomBytes(16);
  const timestamp = currentTimestamp();
  writeTimestamp(bytes, timestamp);

  if (!previous || timestamp > readTimestamp(previous)) return bytes;

  const monotonic = previous.slice();
  if (incrementUlidSuffix(monotonic)) return monotonic;

  const nextTimestamp = readTimestamp(previous);
  if (nextTimestamp >= MAX_TIMESTAMP) throw new Error(IDENTIFIER_SEQUENCE_ERROR);
  writeTimestamp(bytes, nextTimestamp + 1);
  return bytes;
}

function currentTimestamp(): number {
  const now = Date.now();
  if (!Number.isFinite(now) || now <= 0) return 0;
  return Math.min(Math.floor(now), MAX_TIMESTAMP);
}

function readTimestamp(bytes: Uint8Array): number {
  let timestamp = 0;
  for (let index = 0; index < 6; index += 1) {
    timestamp = timestamp * 256 + bytes[index];
  }
  return timestamp;
}

function writeTimestamp(bytes: Uint8Array, timestamp: number): void {
  let remaining = timestamp;
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = remaining % 256;
    remaining = Math.floor(remaining / 256);
  }
}

/** Increment the 74 variable bits in a UUID v7 while preserving version/variant. */
function incrementUuidV7Suffix(bytes: Uint8Array): boolean {
  let carry = 1;
  for (let index = 15; index >= 9 && carry; index -= 1) {
    const next = bytes[index] + carry;
    bytes[index] = next & 0xff;
    carry = next > 0xff ? 1 : 0;
  }

  if (carry) {
    const next = (bytes[8] & 0x3f) + carry;
    bytes[8] = (bytes[8] & 0xc0) | (next & 0x3f);
    carry = next > 0x3f ? 1 : 0;
  }
  if (carry) {
    const next = bytes[7] + carry;
    bytes[7] = next & 0xff;
    carry = next > 0xff ? 1 : 0;
  }
  if (carry) {
    const next = (bytes[6] & 0x0f) + carry;
    bytes[6] = (bytes[6] & 0xf0) | (next & 0x0f);
    carry = next > 0x0f ? 1 : 0;
  }
  return carry === 0;
}

/** Increment the 80-bit random component of a ULID. */
function incrementUlidSuffix(bytes: Uint8Array): boolean {
  let carry = 1;
  for (let index = 15; index >= 6 && carry; index -= 1) {
    const next = bytes[index] + carry;
    bytes[index] = next & 0xff;
    carry = next > 0xff ? 1 : 0;
  }
  return carry === 0;
}

function formatIdentifier(bytes: Uint8Array, options: IdentifierOptions): string {
  const raw = options.kind === "ulid" ? encodeCrockfordUlid(bytes) : encodeUuid(bytes, options.hyphens);
  const cased = options.uppercase ? raw.toUpperCase() : raw.toLowerCase();
  if (options.kind !== "ulid" || !options.hyphens) return cased;
  return groupUlid(cased);
}

function encodeUuid(bytes: Uint8Array, hyphens: boolean): string {
  const compact = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (!hyphens) return compact;
  return `${compact.slice(0, 8)}-${compact.slice(8, 12)}-${compact.slice(12, 16)}-${compact.slice(16, 20)}-${compact.slice(20)}`;
}

/** Encodes the 128-bit ULID payload as 26 Crockford Base32 characters. */
export function encodeCrockfordUlid(bytes: Uint8Array): string {
  if (bytes.length !== 16) throw new Error("ULID payload는 16바이트여야 합니다.");
  let buffer = 0;
  let bitCount = 2;
  let output = "";

  for (const byte of bytes) {
    buffer = (buffer << 8) | byte;
    bitCount += 8;
    while (bitCount >= 5) {
      bitCount -= 5;
      output += CROCKFORD_ALPHABET[(buffer >> bitCount) & 0x1f];
      buffer &= bitCount === 0 ? 0 : (1 << bitCount) - 1;
    }
  }
  return output;
}

/**
 * ULID's canonical representation is the 26-character hyphenless form.
 * Hyphenated output is a display-only grouping for users who need visual
 * separation; removing its four separators restores the canonical value.
 */
function groupUlid(value: string): string {
  return `${value.slice(0, 5)}-${value.slice(5, 10)}-${value.slice(10, 15)}-${value.slice(15, 20)}-${value.slice(20)}`;
}
