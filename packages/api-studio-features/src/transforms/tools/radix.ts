export type RadixInputMode = "auto" | "2" | "8" | "10" | "16";

export const MAX_RADIX_INPUT_CHARACTERS = 512;
export const MAX_RADIX_BITS = 256;
const MAX_RADIX_MAGNITUDE = (1n << BigInt(MAX_RADIX_BITS)) - 1n;

export interface RadixOutputs {
  binary: string;
  octal: string;
  decimal: string;
  hexadecimal: string;
}

export interface RadixMetadata {
  inputBase: 2 | 8 | 10 | 16;
  digitCount: number;
  bitLength: number;
}

export interface RadixError {
  code: string;
  message: string;
  position: number | null;
}

export interface RadixResult {
  outputs: RadixOutputs | null;
  metadata: RadixMetadata | null;
  error: RadixError | null;
}

const PREFIX_BASES: Readonly<Record<string, 2 | 8 | 16>> = {
  "0b": 2,
  "0o": 8,
  "0x": 16,
};

function digitValue(character: string): number {
  const code = character.charCodeAt(0);
  if (code >= 0x30 && code <= 0x39) return code - 0x30;
  if (code >= 0x41 && code <= 0x46) return code - 0x41 + 10;
  if (code >= 0x61 && code <= 0x66) return code - 0x61 + 10;
  return -1;
}

function formatSigned(magnitude: bigint, negative: boolean, base: 2 | 8 | 10 | 16): string {
  const sign = negative && magnitude !== 0n ? "-" : "";
  const prefix = base === 2 ? "0b" : base === 8 ? "0o" : base === 16 ? "0x" : "";
  return `${sign}${prefix}${magnitude.toString(base)}`;
}

function error(code: string, message: string, position: number | null): RadixResult {
  return { outputs: null, metadata: null, error: { code, message, position } };
}

export function convertRadix(input: string, mode: RadixInputMode): RadixResult {
  if (input.length > MAX_RADIX_INPUT_CHARACTERS) {
    return error(
      "INPUT_TOO_LONG",
      "진법 변환 입력은 최대 512자까지 처리할 수 있습니다.",
      null,
    );
  }

  let start = 0;
  let end = input.length;
  while (start < end && /\s/u.test(input[start])) start += 1;
  while (end > start && /\s/u.test(input[end - 1])) end -= 1;
  if (start === end) return { outputs: null, metadata: null, error: null };

  let cursor = start;
  let negative = false;
  if (input[cursor] === "+" || input[cursor] === "-") {
    negative = input[cursor] === "-";
    cursor += 1;
    if (cursor === end) {
      return error("SIGN_WITHOUT_DIGITS", "부호 뒤에 숫자를 입력해야 합니다.", start + 1);
    }
  }

  const prefix = input.slice(cursor, cursor + 2).toLowerCase();
  const prefixBase = PREFIX_BASES[prefix];
  let inputBase: 2 | 8 | 10 | 16;
  if (prefixBase !== undefined) {
    if (mode !== "auto" && Number(mode) !== prefixBase) {
      return error(
        "BASE_PREFIX_MISMATCH",
        "선택한 입력 진법과 접두사가 일치하지 않습니다.",
        cursor + 2,
      );
    }
    inputBase = prefixBase;
    cursor += 2;
    if (cursor === end) {
      return error("PREFIX_WITHOUT_DIGITS", "진법 접두사 뒤에 숫자를 입력해야 합니다.", end + 1);
    }
  } else {
    inputBase = mode === "auto" ? 10 : Number(mode) as 2 | 8 | 10 | 16;
  }

  const digitStart = cursor;
  let magnitude = 0n;
  for (; cursor < end; cursor += 1) {
    const digit = digitValue(input[cursor]);
    if (digit < 0 || digit >= inputBase) {
      return error(
        "INVALID_DIGIT",
        `${inputBase}진수에 사용할 수 없는 digit입니다.`,
        cursor + 1,
      );
    }
    magnitude = magnitude * BigInt(inputBase) + BigInt(digit);
    if (magnitude > MAX_RADIX_MAGNITUDE) {
      return error(
        "VALUE_OUT_OF_RANGE",
        "값의 절댓값은 최대 256bit까지 변환할 수 있습니다.",
        cursor + 1,
      );
    }
  }

  return {
    outputs: {
      binary: formatSigned(magnitude, negative, 2),
      octal: formatSigned(magnitude, negative, 8),
      decimal: formatSigned(magnitude, negative, 10),
      hexadecimal: formatSigned(magnitude, negative, 16),
    },
    metadata: {
      inputBase,
      digitCount: end - digitStart,
      bitLength: magnitude === 0n ? 1 : magnitude.toString(2).length,
    },
    error: null,
  };
}
