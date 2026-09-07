import { describe, expect, it } from "vitest";
import {
  convertByteEncoding,
  MAX_BYTE_CODEC_BYTES,
  MAX_BYTE_CODEC_INPUT_CHARACTERS,
} from "./byteCodec";

describe("convertByteEncoding", () => {
  it("RFC 4648 Base64 vector와 UTF-8 text를 왕복한다", () => {
    const vectors = [
      ["", ""],
      ["f", "Zg=="],
      ["fo", "Zm8="],
      ["foo", "Zm9v"],
      ["foob", "Zm9vYg=="],
      ["fooba", "Zm9vYmE="],
      ["foobar", "Zm9vYmFy"],
    ] as const;

    for (const [plain, encoded] of vectors) {
      expect(convertByteEncoding(plain, "utf8", "base64")).toEqual({
        output: encoded,
        byteLength: plain.length,
        error: null,
      });
      expect(convertByteEncoding(encoded, "base64", "utf8").output).toBe(plain);
    }

    const korean = convertByteEncoding("안녕하세요", "utf8", "base64");
    expect(korean.byteLength).toBe(15);
    expect(convertByteEncoding(korean.output, "base64", "utf8").output).toBe("안녕하세요");
  });

  it("raw byte를 Hex, Base64와 unpadded Base64URL 사이에서 보존한다", () => {
    expect(convertByteEncoding("00 ff 10", "hex", "base64")).toEqual({
      output: "AP8Q",
      byteLength: 3,
      error: null,
    });
    expect(convertByteEncoding("fbff", "hex", "base64url").output).toBe("-_8");
    expect(convertByteEncoding("-_8=", "base64url", "hex").output).toBe("fbff");
    expect(convertByteEncoding("Z m 9 v\n", "base64", "hex").output).toBe("666f6f");
  });

  it("UTF-8 BOM을 raw byte 왕복에서 제거하지 않는다", () => {
    const text = convertByteEncoding("efbbbf41", "hex", "utf8");
    expect(text.output).toBe("\uFEFFA");
    expect(convertByteEncoding(text.output, "utf8", "hex").output).toBe("efbbbf41");
  });

  it("잘못된 Hex 문자와 홀수 nibble의 원문 위치를 표시한다", () => {
    const invalid = convertByteEncoding("de ad zg", "hex", "base64");
    const incomplete = convertByteEncoding("de a", "hex", "base64");

    expect(invalid.error).toMatchObject({
      code: "INVALID_HEX_CHARACTER",
      position: 7,
      unit: "character",
    });
    expect(incomplete.error).toMatchObject({
      code: "INCOMPLETE_HEX_BYTE",
      position: 4,
      unit: "character",
    });
  });

  it("Base64와 Base64URL alphabet 오류의 원문 위치를 구분한다", () => {
    const standard = convertByteEncoding("Zm$9v", "base64", "hex");
    const urlSafe = convertByteEncoding("Zm+8", "base64url", "hex");

    expect(standard.error).toMatchObject({
      code: "INVALID_BASE64_CHARACTER",
      position: 3,
      unit: "character",
    });
    expect(urlSafe.error).toMatchObject({
      code: "INVALID_BASE64URL_CHARACTER",
      position: 3,
      unit: "character",
    });
  });

  it("padding 위치·길이와 incomplete quantum을 거부한다", () => {
    expect(convertByteEncoding("Z=g=", "base64", "hex").error).toMatchObject({
      code: "INVALID_BASE64_PADDING",
      position: 3,
    });
    expect(convertByteEncoding("Zg=", "base64", "hex").error).toMatchObject({
      code: "INVALID_BASE64_PADDING",
      position: 3,
    });
    expect(convertByteEncoding("A", "base64", "hex").error).toMatchObject({
      code: "INVALID_BASE64_LENGTH",
      position: 1,
    });
  });

  it("non-zero pad bit를 non-canonical 위치 오류로 막는다", () => {
    const result = convertByteEncoding("Zh==", "base64", "hex");
    expect(result.error).toMatchObject({
      code: "NON_CANONICAL_BASE64",
      position: 2,
      unit: "character",
    });
  });

  it("invalid·overlong·truncated UTF-8의 최초 byte 위치를 표시한다", () => {
    expect(convertByteEncoding("e228a1", "hex", "utf8").error).toMatchObject({
      code: "INVALID_UTF8_BYTES",
      position: 2,
      unit: "byte",
    });
    expect(convertByteEncoding("c0af", "hex", "utf8").error).toMatchObject({
      code: "INVALID_UTF8_BYTES",
      position: 1,
      unit: "byte",
    });
    expect(convertByteEncoding("f09f", "hex", "utf8").error).toMatchObject({
      code: "INVALID_UTF8_BYTES",
      position: 2,
      unit: "byte",
    });
    expect(convertByteEncoding("eda080", "hex", "utf8").error).toMatchObject({
      code: "INVALID_UTF8_BYTES",
      position: 2,
      unit: "byte",
    });
    expect(convertByteEncoding("f4908080", "hex", "utf8").error).toMatchObject({
      code: "INVALID_UTF8_BYTES",
      position: 2,
      unit: "byte",
    });
    expect(convertByteEncoding("f09f92a9", "hex", "utf8").output).toBe("💩");
  });

  it("짝이 맞지 않는 JS surrogate를 대체 문자로 바꾸지 않는다", () => {
    const result = convertByteEncoding(`A${String.fromCharCode(0xd800)}B`, "utf8", "hex");
    expect(result.error).toMatchObject({
      code: "INVALID_UNICODE_TEXT",
      position: 2,
      unit: "character",
    });
  });

  it("표현 길이와 decoded raw byte 상한을 각각 적용한다", () => {
    const representation = convertByteEncoding(
      "x".repeat(MAX_BYTE_CODEC_INPUT_CHARACTERS + 1),
      "utf8",
      "base64",
    );
    const utf8Bytes = convertByteEncoding(
      "가".repeat(Math.floor(MAX_BYTE_CODEC_BYTES / 3) + 1),
      "utf8",
      "base64",
    );
    const bytes = convertByteEncoding(
      "00".repeat(MAX_BYTE_CODEC_BYTES + 1),
      "hex",
      "base64",
    );
    const base64Bytes = convertByteEncoding(
      "AAAA".repeat(Math.floor(MAX_BYTE_CODEC_BYTES / 3) + 1),
      "base64",
      "hex",
    );

    expect(representation.error?.code).toBe("INPUT_TOO_LARGE");
    expect(utf8Bytes.error?.code).toBe("BYTE_LIMIT_EXCEEDED");
    expect(bytes.error?.code).toBe("BYTE_LIMIT_EXCEEDED");
    expect(bytes.byteLength).toBe(0);
    expect(base64Bytes.error?.code).toBe("BYTE_LIMIT_EXCEEDED");
  });
});
