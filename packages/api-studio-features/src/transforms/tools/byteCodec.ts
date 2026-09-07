export type ByteEncoding = "utf8" | "hex" | "base64" | "base64url";
export type ByteCodecErrorUnit = "character" | "byte";

export const MAX_BYTE_CODEC_INPUT_CHARACTERS = 2_100_000;
export const MAX_BYTE_CODEC_BYTES = 1_000_000;

export interface ByteCodecError {
  code: string;
  message: string;
  position: number | null;
  unit: ByteCodecErrorUnit | null;
}

export interface ByteCodecResult {
  output: string;
  byteLength: number;
  error: ByteCodecError | null;
}

type DecodeResult =
  | { bytes: Uint8Array; error: null }
  | { bytes: null; error: ByteCodecError };

function failure(
  code: string,
  message: string,
  position: number | null = null,
  unit: ByteCodecErrorUnit | null = null,
): DecodeResult {
  return { bytes: null, error: { code, message, position, unit } };
}

function isAsciiWhitespace(character: string): boolean {
  return character === " " || character === "\t" || character === "\r" || character === "\n";
}

function hexNibbleValue(character: string): number {
  const code = character.charCodeAt(0);
  if (code >= 0x30 && code <= 0x39) return code - 0x30;
  if (code >= 0x41 && code <= 0x46) return code - 0x41 + 10;
  if (code >= 0x61 && code <= 0x66) return code - 0x61 + 10;
  return -1;
}

function encodedCharacterPosition(input: string, encodedIndex: number): number {
  let current = 0;
  for (let index = 0; index < input.length; index += 1) {
    if (isAsciiWhitespace(input[index])) continue;
    if (current === encodedIndex) return index + 1;
    current += 1;
  }
  return input.length + 1;
}

function firstUnpairedSurrogate(input: string): number | null {
  for (let index = 0; index < input.length; index += 1) {
    const code = input.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = input.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        index += 1;
        continue;
      }
      return index;
    }
    if (code >= 0xdc00 && code <= 0xdfff) return index;
  }
  return null;
}

function decodeUtf8Text(input: string): DecodeResult {
  const invalidPosition = firstUnpairedSurrogate(input);
  if (invalidPosition !== null) {
    return failure(
      "INVALID_UNICODE_TEXT",
      "짝이 맞지 않는 Unicode surrogate를 UTF-8로 변환할 수 없습니다.",
      invalidPosition + 1,
      "character",
    );
  }
  return { bytes: new TextEncoder().encode(input), error: null };
}

function decodeHex(input: string): DecodeResult {
  let nibbleCount = 0;
  let lastNibblePosition = 1;
  for (let index = 0; index < input.length; index += 1) {
    const character = input[index];
    if (isAsciiWhitespace(character)) continue;
    if (hexNibbleValue(character) === -1) {
      return failure(
        "INVALID_HEX_CHARACTER",
        "Hex 입력에는 0-9와 A-F만 사용할 수 있습니다.",
        index + 1,
        "character",
      );
    }
    nibbleCount += 1;
    lastNibblePosition = index + 1;
  }

  if (nibbleCount % 2 !== 0) {
    return failure(
      "INCOMPLETE_HEX_BYTE",
      "Hex byte는 두 자리씩 입력해야 합니다.",
      lastNibblePosition,
      "character",
    );
  }
  const byteLength = nibbleCount / 2;
  if (byteLength > MAX_BYTE_CODEC_BYTES) {
    return failure("BYTE_LIMIT_EXCEEDED", "변환할 raw byte는 최대 1,000,000바이트입니다.");
  }

  const bytes = new Uint8Array(byteLength);
  let highNibble = -1;
  let byteIndex = 0;
  for (const character of input) {
    if (isAsciiWhitespace(character)) continue;
    const value = hexNibbleValue(character);
    if (highNibble === -1) highNibble = value;
    else {
      bytes[byteIndex] = (highNibble << 4) | value;
      byteIndex += 1;
      highNibble = -1;
    }
  }
  return { bytes, error: null };
}

function encodeBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000;
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

function decodeBase64(input: string, urlSafe: boolean): DecodeResult {
  const alphabet = urlSafe ? /^[A-Za-z0-9_-]$/u : /^[A-Za-z0-9+/]$/u;
  let encoded = "";
  let firstPadding = -1;
  let paddingLength = 0;

  for (let index = 0; index < input.length; index += 1) {
    const character = input[index];
    if (isAsciiWhitespace(character)) continue;
    if (character === "=") {
      firstPadding = firstPadding === -1 ? encoded.length : firstPadding;
      paddingLength += 1;
      if (paddingLength > 2) {
        return failure(
          "INVALID_BASE64_PADDING",
          "Base64 padding은 최대 두 자리입니다.",
          index + 1,
          "character",
        );
      }
    } else if (!alphabet.test(character)) {
      return failure(
        urlSafe ? "INVALID_BASE64URL_CHARACTER" : "INVALID_BASE64_CHARACTER",
        urlSafe
          ? "Base64URL 입력에는 영문자, 숫자, -와 _만 사용할 수 있습니다."
          : "Base64 입력에는 영문자, 숫자, +와 /만 사용할 수 있습니다.",
        index + 1,
        "character",
      );
    } else if (firstPadding !== -1) {
      return failure(
        "INVALID_BASE64_PADDING",
        "Base64 padding은 입력 끝에만 올 수 있습니다.",
        index + 1,
        "character",
      );
    }
    encoded += urlSafe ? character.replace(/-/u, "+").replace(/_/u, "/") : character;
  }

  const dataLength = firstPadding === -1 ? encoded.length : firstPadding;
  if (firstPadding !== -1) {
    const expectedPadding = (4 - (dataLength % 4)) % 4;
    if (expectedPadding === 0 || paddingLength !== expectedPadding || encoded.length % 4 !== 0) {
      return failure(
        "INVALID_BASE64_PADDING",
        "Base64 padding 길이가 byte 경계와 맞지 않습니다.",
        encodedCharacterPosition(input, firstPadding),
        "character",
      );
    }
  }

  if (dataLength % 4 === 1) {
    return failure(
      "INVALID_BASE64_LENGTH",
      "Base64 입력 길이로 완전한 byte를 만들 수 없습니다.",
      encodedCharacterPosition(input, Math.max(0, dataLength - 1)),
      "character",
    );
  }

  const decodedByteLength = Math.floor((dataLength * 6) / 8);
  if (decodedByteLength > MAX_BYTE_CODEC_BYTES) {
    return failure("BYTE_LIMIT_EXCEEDED", "변환할 raw byte는 최대 1,000,000바이트입니다.");
  }

  const dataCharacters = encoded.slice(0, dataLength);
  const normalized = `${dataCharacters}${"=".repeat((4 - (dataLength % 4)) % 4)}`;

  let bytes: Uint8Array;
  try {
    const binary = atob(normalized);
    bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  } catch {
    return failure("INVALID_BASE64", "Base64 입력을 byte로 변환할 수 없습니다.");
  }

  const canonical = encodeBase64(bytes).replace(/=+$/u, "");
  if (canonical !== dataCharacters) {
    let mismatch = 0;
    while (canonical[mismatch] === dataCharacters[mismatch]) mismatch += 1;
    return failure(
      "NON_CANONICAL_BASE64",
      "사용되지 않는 Base64 pad bit는 0이어야 합니다.",
      encodedCharacterPosition(input, mismatch),
      "character",
    );
  }

  return { bytes, error: null };
}

function firstInvalidUtf8Byte(bytes: Uint8Array): number | null {
  const continuation = (index: number, minimum = 0x80, maximum = 0xbf): number | null => {
    if (index >= bytes.length) return index - 1;
    return bytes[index] >= minimum && bytes[index] <= maximum ? null : index;
  };

  for (let index = 0; index < bytes.length;) {
    const first = bytes[index];
    if (first <= 0x7f) {
      index += 1;
      continue;
    }

    let issue: number | null;
    if (first >= 0xc2 && first <= 0xdf) {
      issue = continuation(index + 1);
      if (issue !== null) return issue;
      index += 2;
    } else if (first === 0xe0) {
      issue = continuation(index + 1, 0xa0, 0xbf) ?? continuation(index + 2);
      if (issue !== null) return issue;
      index += 3;
    } else if (first >= 0xe1 && first <= 0xec) {
      issue = continuation(index + 1) ?? continuation(index + 2);
      if (issue !== null) return issue;
      index += 3;
    } else if (first === 0xed) {
      issue = continuation(index + 1, 0x80, 0x9f) ?? continuation(index + 2);
      if (issue !== null) return issue;
      index += 3;
    } else if (first >= 0xee && first <= 0xef) {
      issue = continuation(index + 1) ?? continuation(index + 2);
      if (issue !== null) return issue;
      index += 3;
    } else if (first === 0xf0) {
      issue = continuation(index + 1, 0x90, 0xbf)
        ?? continuation(index + 2)
        ?? continuation(index + 3);
      if (issue !== null) return issue;
      index += 4;
    } else if (first >= 0xf1 && first <= 0xf3) {
      issue = continuation(index + 1) ?? continuation(index + 2) ?? continuation(index + 3);
      if (issue !== null) return issue;
      index += 4;
    } else if (first === 0xf4) {
      issue = continuation(index + 1, 0x80, 0x8f)
        ?? continuation(index + 2)
        ?? continuation(index + 3);
      if (issue !== null) return issue;
      index += 4;
    } else {
      return index;
    }
  }
  return null;
}

function decodeInput(input: string, source: ByteEncoding): DecodeResult {
  if (source === "utf8") return decodeUtf8Text(input);
  if (source === "hex") return decodeHex(input);
  return decodeBase64(input, source === "base64url");
}

function encodeOutput(bytes: Uint8Array, target: ByteEncoding): string | ByteCodecError {
  if (target === "hex") {
    const chunks: string[] = [];
    const chunkSize = 0x4000;
    for (let offset = 0; offset < bytes.length; offset += chunkSize) {
      let chunk = "";
      for (const byte of bytes.subarray(offset, offset + chunkSize)) {
        chunk += byte.toString(16).padStart(2, "0");
      }
      chunks.push(chunk);
    }
    return chunks.join("");
  }
  if (target === "base64") return encodeBase64(bytes);
  if (target === "base64url") {
    return encodeBase64(bytes).replace(/\+/gu, "-").replace(/\//gu, "_").replace(/=+$/u, "");
  }

  const invalidByte = firstInvalidUtf8Byte(bytes);
  if (invalidByte !== null) {
    return {
      code: "INVALID_UTF8_BYTES",
      message: "입력 byte를 손실 없이 UTF-8 text로 표현할 수 없습니다.",
      position: invalidByte + 1,
      unit: "byte",
    };
  }
  try {
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return {
      code: "UTF8_DECODE_FAILED",
      message: "입력 byte를 UTF-8 text로 변환할 수 없습니다.",
      position: null,
      unit: null,
    };
  }
}

export function convertByteEncoding(
  input: string,
  source: ByteEncoding,
  target: ByteEncoding,
): ByteCodecResult {
  if (input.length > MAX_BYTE_CODEC_INPUT_CHARACTERS) {
    return {
      output: "",
      byteLength: 0,
      error: {
        code: "INPUT_TOO_LARGE",
        message: "입력 표현은 최대 2,100,000자까지 처리할 수 있습니다.",
        position: null,
        unit: null,
      },
    };
  }

  const decoded = decodeInput(input, source);
  if (decoded.error) return { output: "", byteLength: 0, error: decoded.error };
  if (decoded.bytes.length > MAX_BYTE_CODEC_BYTES) {
    return {
      output: "",
      byteLength: decoded.bytes.length,
      error: {
        code: "BYTE_LIMIT_EXCEEDED",
        message: "변환할 raw byte는 최대 1,000,000바이트입니다.",
        position: null,
        unit: null,
      },
    };
  }

  const output = encodeOutput(decoded.bytes, target);
  if (typeof output !== "string") {
    return { output: "", byteLength: decoded.bytes.length, error: output };
  }
  return { output, byteLength: decoded.bytes.length, error: null };
}
