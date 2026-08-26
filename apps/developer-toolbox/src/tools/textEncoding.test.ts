import { describe, expect, it } from "vitest";
import {
  htmlEntityDecode,
  htmlEntityEncode,
  runTextTransform,
  TEXT_ENCODING_LIMITS,
  TextTransformError,
  urlComponentDecode,
  urlComponentEncode,
} from "./textEncoding";

describe("HTML entity text codec", () => {
  it("encodes text-significant characters with deterministic canonical entities", () => {
    expect(htmlEntityEncode(`<tag title="O'Reilly">& 한글 😀</tag>`)).toBe(
      "&lt;tag title=&quot;O&#39;Reilly&quot;&gt;&amp; 한글 😀&lt;/tag&gt;",
    );
  });

  it("decodes named and numeric entities without using an HTML parser", () => {
    expect(htmlEntityDecode("&lt;copy&gt; &amp; &copy; &NotEqualTilde; &#169; &#x1F600; &apos;"))
      .toBe("<copy> & © ≂̸ © 😀 '");
  });

  it("preserves a literal ampersand that cannot begin an entity", () => {
    expect(htmlEntityDecode("A & B")).toBe("A & B");
  });

  it("fails closed for unknown, unterminated, invalid, and surrogate entities", () => {
    for (const input of [
      "&unknown;",
      "&constructor;",
      "&amp",
      "&#xZZ;",
      "&#12345678;",
      "&#x110000;",
      "&#xD800;",
    ]) {
      expect(() => htmlEntityDecode(input)).toThrowError(
        new TextTransformError("malformed_entity"),
      );
    }
  });

  it("bounds entity expansion and input before unbounded work", () => {
    expect(() => htmlEntityDecode("&amp;".repeat(TEXT_ENCODING_LIMITS.maxEntityCount + 1)))
      .toThrowError(new TextTransformError("entity_limit"));
    expect(() => htmlEntityEncode("&".repeat(1_000_000))).toThrowError(
      new TextTransformError("output_too_large"),
    );
    expect(() => htmlEntityEncode("a".repeat(TEXT_ENCODING_LIMITS.maxInputBytes + 1)))
      .toThrowError(new TextTransformError("input_too_large"));
  });
});

describe("URL component text codec", () => {
  it("round-trips Unicode and component delimiters", () => {
    const original = "hello world?foo=bar&baz=한글/😀";
    const encoded = urlComponentEncode(original);
    expect(encoded).toBe("hello%20world%3Ffoo%3Dbar%26baz%3D%ED%95%9C%EA%B8%80%2F%F0%9F%98%80");
    expect(urlComponentDecode(encoded)).toBe(original);
  });

  it("rejects malformed percent escapes and invalid UTF-8 with a fixed error", () => {
    for (const input of ["%", "%zz", "%E0%A4%A", "%C0%AF"]) {
      expect(() => urlComponentDecode(input)).toThrowError(
        new TextTransformError("malformed_url"),
      );
    }
  });

  it("rejects lone surrogates instead of replacing them", () => {
    expect(() => urlComponentEncode("\ud800")).toThrowError(
      new TextTransformError("invalid_unicode"),
    );
  });

  it("applies the input bound before URL encoding work", () => {
    expect(() => urlComponentEncode("a".repeat(TEXT_ENCODING_LIMITS.maxInputBytes + 1)))
      .toThrowError(new TextTransformError("input_too_large"));
  });
});

describe("safe text transform result", () => {
  it("never reflects an unexpected raw error", async () => {
    const result = await runTextTransform(() => {
      throw new Error("credential=super-secret /tmp/private");
    }, "ignored");
    expect(result.output).toBe("");
    expect(result.error).toBe("Text transformation failed.");
    expect(result.errorCode).toBe("transform_failed");
    expect(result.error).not.toContain("super-secret");
    expect(result.error).not.toContain("/tmp/private");
  });
});
