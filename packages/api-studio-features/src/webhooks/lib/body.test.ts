import { describe, expect, it } from "vitest";
import { bodyPreview } from "./body";

describe("bodyPreview", () => {
  it("shows the first 200 characters of text bodies", () => {
    expect(bodyPreview("x".repeat(300), undefined)).toBe("x".repeat(200));
    expect(bodyPreview("{}", "utf8")).toBe("{}");
  });

  it("describes binary bodies by decoded size", () => {
    expect(bodyPreview("/wAB", "base64")).toBe("바이너리 본문 · 3 bytes");
    expect(bodyPreview("/w==", "base64")).toBe("바이너리 본문 · 1 bytes");
  });
});
