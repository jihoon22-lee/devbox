import { describe, expect, it } from "vitest";
import {
  base64Decode,
  base64Encode,
  convertCase,
  convertTimestamp,
  decodeJwt,
  formatJson,
  fromBase64,
  jsonFormatter,
  jsonMinifier,
  parseTimestampInput,
  toBase64,
  urlDecode,
  urlEncode,
} from "./transformers";

describe("formatJson", () => {
  it("2칸 들여쓰기로 예쁘게 포맷한다", async () => {
    const res = await formatJson('{"a":1,"b":[1,2]}', "format");
    expect(res.error).toBeUndefined();
    expect(res.output).toBe('{\n  "a": 1,\n  "b": [\n    1,\n    2\n  ]\n}');
  });

  it("공백 없이 압축한다", async () => {
    const res = await formatJson('{ "a" : 1, "b" : [1, 2] }', "minify");
    expect(res.error).toBeUndefined();
    expect(res.output).toBe('{"a":1,"b":[1,2]}');
  });

  it("깨진 JSON은 에러를 반환하고 output은 빈 문자열", async () => {
    const res = await formatJson("{not valid json", "format");
    expect(res.output).toBe("");
    expect(res.error).toBeTruthy();
  });

  it("빈 문자열도 파싱 실패로 처리한다", async () => {
    const res = await formatJson("", "format");
    expect(res.output).toBe("");
    expect(res.error).toBeTruthy();
  });

  it("jsonFormatter/jsonMinifier 팩토리가 formatJson과 동일하게 동작한다", async () => {
    const viaFactory = await jsonFormatter()('{"x":1}');
    const direct = await formatJson('{"x":1}', "format");
    expect(viaFactory).toEqual(direct);

    const viaMinifyFactory = await jsonMinifier()('{"x": 1}');
    expect(viaMinifyFactory.output).toBe('{"x":1}');
  });
});

describe("Base64 인코딩/디코딩", () => {
  it("ASCII 문자열을 왕복한다", async () => {
    const encoded = await toBase64("hello world");
    expect(encoded.error).toBeUndefined();
    const decoded = await fromBase64(encoded.output);
    expect(decoded.output).toBe("hello world");
  });

  it("UTF-8(한글) 문자열도 왕복한다", async () => {
    const encoded = await toBase64("안녕하세요");
    expect(encoded.error).toBeUndefined();
    const decoded = await fromBase64(encoded.output);
    expect(decoded.output).toBe("안녕하세요");
  });

  it("잘못된 Base64 입력은 에러를 반환한다", async () => {
    const res = await fromBase64("not-valid-base64!!!");
    expect(res.output).toBe("");
    expect(res.error).toBeTruthy();
  });

  it("base64Encode/base64Decode 팩토리가 원본 함수와 동일하게 동작한다", async () => {
    expect(await base64Encode()("abc")).toEqual(await toBase64("abc"));
    const enc = await toBase64("abc");
    expect(await base64Decode()(enc.output)).toEqual(await fromBase64(enc.output));
  });
});

describe("decodeJwt", () => {
  const token =
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.AL_nmexgcwawKDK5uJ0RtfAxT1GguksdPuaahEACpHc";

  it("유효한 JWT의 헤더와 페이로드를 검증되지 않음으로 출력한다", async () => {
    const res = await decodeJwt(token);
    expect(res.error).toBeUndefined();
    const output = JSON.parse(res.output) as Record<string, unknown>;
    expect(output.verification).toBe("unverified");
    expect((output.header as Record<string, unknown>).alg).toBe("HS256");
    expect((output.payload as Record<string, unknown>).sub).toBe("1234567890");
  });

  it("compact 구조가 아니면 고정 오류를 반환한다", async () => {
    const res = await decodeJwt("not-a-jwt-token");
    expect(res.output).toBe("");
    expect(res.error).toBeTruthy();
    expect(res.error).not.toContain("not-a-jwt-token");
  });

  it("서명이 없거나 유효한 길이가 아니면 에러를 반환한다", async () => {
    const res = await decodeJwt(`${token.slice(0, token.lastIndexOf(".") + 1)}AA`);
    expect(res.output).toBe("");
    expect(res.error).toBeTruthy();
  });

  it("페이로드를 디코딩해도 JSON이 아니면 에러를 반환한다", async () => {
    const notJsonB64 = btoa("this is not json").replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    const header = "eyJhbGciOiJIUzI1NiJ9";
    const res = await decodeJwt(`${header}.${notJsonB64}.${token.split(".")[2]}`);
    expect(res.output).toBe("");
    expect(res.error).toBeTruthy();
  });
});

describe("urlEncode / urlDecode", () => {
  it("특수문자를 인코딩한다", () => {
    expect(urlEncode("a b&c=d")).toBe("a%20b%26c%3Dd");
  });

  it("인코딩한 문자열을 다시 디코딩하면 원본으로 돌아온다", () => {
    const original = "hello world?foo=bar&baz=한글";
    expect(urlDecode(urlEncode(original))).toBe(original);
  });

  it("잘못된 %-시퀀스는 고정 오류를 던져 원문을 반향하지 않는다", () => {
    expect(() => urlDecode("%")).toThrow();
    expect(() => urlDecode("%zz")).toThrow();
  });
});

describe("parseTimestampInput", () => {
  it("1e12 미만 숫자는 초 단위로 해석해 ms로 환산한다", () => {
    const d = parseTimestampInput("1700000000");
    expect(d).not.toBeNull();
    expect(d?.getTime()).toBe(1700000000000);
  });

  it("1e12 이상 숫자는 이미 ms 단위로 본다", () => {
    const d = parseTimestampInput("1700000000000");
    expect(d?.getTime()).toBe(1700000000000);
  });

  it("0은 epoch(1970-01-01)로 해석한다", () => {
    const d = parseTimestampInput("0");
    expect(d?.getTime()).toBe(0);
  });

  it("음수는 epoch 이전 초 단위로 해석한다", () => {
    const d = parseTimestampInput("-1000");
    expect(d?.getTime()).toBe(-1000000);
  });

  it("Date 표현 범위를 넘는 거대한 숫자는 null(해석 불가)을 반환한다", () => {
    // 1e12 이상이라 ms로 취급되지만, Date가 표현 가능한 최대치(약 8.64e15)를 넘는다.
    const d = parseTimestampInput("99999999999999999999");
    expect(d).toBeNull();
  });

  it("숫자가 아닌 ISO 날짜 문자열은 Date로 파싱한다", () => {
    const d = parseTimestampInput("2024-01-15T00:00:00.000Z");
    expect(d?.getTime()).toBe(new Date("2024-01-15T00:00:00.000Z").getTime());
  });

  it("해석할 수 없는 문자열은 null을 반환한다", () => {
    expect(parseTimestampInput("not-a-date")).toBeNull();
  });
});

describe("convertTimestamp (TimestampConverter가 실제로 쓰는 변환 파이프라인)", () => {
  it("빈 입력은 에러 없이 빈 출력", async () => {
    const res = await convertTimestamp("   ");
    expect(res.output).toBe("");
    expect(res.error).toBeUndefined();
  });

  it("해석 불가능한 입력은 한국어 에러 메시지를 반환한다", async () => {
    const res = await convertTimestamp("not-a-date");
    expect(res.output).toBe("");
    expect(res.error).toBe("날짜로 해석할 수 없습니다");
  });

  it("유효한 타임스탬프는 로컬 문자열로 변환한다 (같은 런타임의 toLocaleString과 비교)", async () => {
    const res = await convertTimestamp("1700000000");
    expect(res.error).toBeUndefined();
    expect(res.output).toBe(new Date(1700000000000).toLocaleString());
  });
});

describe("convertCase", () => {
  it("공백으로 구분된 단어를 6가지 표기로 변환한다", () => {
    const out = convertCase("hello world");
    expect(out).toBe(
      ["UPPER: HELLO WORLD", "lower: hello world", "Pascal: HelloWorld", "camel: helloWorld", "kebab: hello-world", "snake: hello_world"].join(
        "\n",
      ),
    );
  });

  it("빈 문자열은 모든 표기가 빈 값이다", () => {
    const out = convertCase("");
    expect(out).toBe(["UPPER: ", "lower: ", "Pascal: ", "camel: ", "kebab: ", "snake: "].join("\n"));
  });

  it("밑줄(_)은 camelCase 단어 경계로 인식되지 않는다 (실제 정규식 동작 그대로)", () => {
    // \b\w 는 단어문자(\w에는 밑줄도 포함) 사이에서는 경계로 보지 않으므로, foo_bar의
    // "b"는 대문자로 바뀌지 않는다. 반면 하이픈은 비-단어문자라 그 다음 문자는 경계로 인식된다.
    const out = convertCase("foo_bar-baz");
    const camelLine = out.split("\n").find((l) => l.startsWith("camel:"));
    expect(camelLine).toBe("camel: foo_bar-Baz");
  });
});
