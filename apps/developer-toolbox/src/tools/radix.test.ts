import { describe, expect, it } from "vitest";
import { convertRadix, MAX_RADIX_INPUT_CHARACTERS } from "./radix";

describe("convertRadix", () => {
  it("자동 모드에서 0b, 0o, 0x prefix와 prefix 없는 10진수를 감지한다", () => {
    expect(convertRadix("0b101010", "auto").outputs?.decimal).toBe("42");
    expect(convertRadix("0o52", "auto").outputs?.decimal).toBe("42");
    expect(convertRadix("42", "auto").outputs?.hexadecimal).toBe("0x2a");
    expect(convertRadix("0X2A", "auto").outputs?.binary).toBe("0b101010");
  });

  it("명시적 입력 진법은 prefix 없는 값을 정확히 변환한다", () => {
    expect(convertRadix("101010", "2").outputs?.decimal).toBe("42");
    expect(convertRadix("52", "8").outputs?.decimal).toBe("42");
    expect(convertRadix("42", "10").outputs?.decimal).toBe("42");
    expect(convertRadix("2a", "16").outputs?.decimal).toBe("42");
  });

  it("sign을 prefix 앞에서 처리하고 canonical signed-magnitude 결과를 만든다", () => {
    const negative = convertRadix(" -0x2A ", "auto");
    expect(negative.outputs).toEqual({
      binary: "-0b101010",
      octal: "-0o52",
      decimal: "-42",
      hexadecimal: "-0x2a",
    });
    expect(negative.metadata).toEqual({ inputBase: 16, digitCount: 2, bitLength: 6 });

    expect(convertRadix("+42", "10").outputs?.decimal).toBe("42");
    expect(convertRadix("-0", "10").outputs?.decimal).toBe("0");
  });

  it("선택 진법과 알려진 prefix가 다르면 prefix 위치에서 거부한다", () => {
    expect(convertRadix("0x10", "10").error).toMatchObject({
      code: "BASE_PREFIX_MISMATCH",
      position: 2,
    });
    expect(convertRadix("-0b10", "16").error).toMatchObject({
      code: "BASE_PREFIX_MISMATCH",
      position: 3,
    });
  });

  it("잘못된 digit의 원문 위치를 부호와 앞 공백까지 포함해 표시한다", () => {
    expect(convertRadix("  -0b102", "auto").error).toMatchObject({
      code: "INVALID_DIGIT",
      position: 8,
    });
    expect(convertRadix("19", "8").error).toMatchObject({
      code: "INVALID_DIGIT",
      position: 2,
    });

    const secret = "DO_NOT_REFLECT_RADIX_SECRET";
    const safeError = convertRadix(`0xdead${secret}`, "auto").error;
    expect(JSON.stringify(safeError)).not.toContain(secret);
    expect(JSON.stringify(safeError)).not.toContain("dead");
  });

  it("sign·prefix만 있거나 sign 위치가 잘못된 입력을 거부한다", () => {
    expect(convertRadix("-", "auto").error).toMatchObject({
      code: "SIGN_WITHOUT_DIGITS",
      position: 1,
    });
    expect(convertRadix("+0x", "auto").error).toMatchObject({
      code: "PREFIX_WITHOUT_DIGITS",
      position: 4,
    });
    expect(convertRadix("0x-1", "auto").error).toMatchObject({
      code: "INVALID_DIGIT",
      position: 3,
    });
  });

  it("내부 공백과 digit separator를 invalid digit 위치에서 거부한다", () => {
    expect(convertRadix("10 10", "2").error).toMatchObject({ code: "INVALID_DIGIT", position: 3 });
    expect(convertRadix("1_000", "10").error).toMatchObject({ code: "INVALID_DIGIT", position: 2 });
  });

  it("256bit 최대값은 정확히 유지하고 초과 digit 위치에서 중단한다", () => {
    const maximumHex = "f".repeat(64);
    const accepted = convertRadix(maximumHex, "16");
    expect(accepted.error).toBeNull();
    expect(accepted.metadata?.bitLength).toBe(256);
    expect(accepted.outputs?.hexadecimal).toBe(`0x${maximumHex}`);

    const overflow = convertRadix(`0x1${"0".repeat(64)}`, "auto");
    expect(overflow.error).toMatchObject({ code: "VALUE_OUT_OF_RANGE", position: 67 });
  });

  it("Number precision 범위를 넘는 64bit 정수도 정확히 변환한다", () => {
    const result = convertRadix("18446744073709551615", "10");
    expect(result.outputs?.hexadecimal).toBe("0xffffffffffffffff");
    expect(result.metadata?.bitLength).toBe(64);
  });

  it("빈 입력은 조용히 비우고 표현 길이 상한을 적용한다", () => {
    expect(convertRadix(" \n ", "auto")).toEqual({ outputs: null, metadata: null, error: null });
    expect(convertRadix("1".repeat(MAX_RADIX_INPUT_CHARACTERS + 1), "2").error?.code).toBe(
      "INPUT_TOO_LONG",
    );
  });
});
