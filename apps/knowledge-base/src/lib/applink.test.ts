import { describe, expect, it } from "vitest";
import { routeOpenRequest } from "./applink";

describe("knowledge applink routing", () => {
  it("routes Path without normalizing the filesystem value in frontend", () => {
    expect(
      routeOpenRequest({
        target: { kind: "path", path: "C:\\Knowledge\\Notes\\one.md", line: 8, column: 2 },
        from: "devbox-launcher",
      }),
    ).toEqual({ kind: "openNote", path: "C:\\Knowledge\\Notes\\one.md" });
  });

  it("trims and routes a bounded Query", () => {
    expect(
      routeOpenRequest({ target: { kind: "query", text: "  rust ownership  " }, from: null }),
    ).toEqual({ kind: "search", query: "rust ownership" });
  });

  it("rejects empty, oversized, and unsupported targets with generic messages", () => {
    expect(routeOpenRequest({ target: { kind: "query", text: "   " }, from: null })).toEqual({
      kind: "error",
      message: "요청한 검색어를 사용할 수 없습니다",
    });
    expect(routeOpenRequest({ target: { kind: "query", text: "x".repeat(513) }, from: null })).toEqual({
      kind: "error",
      message: "요청한 검색어를 사용할 수 없습니다",
    });
    expect(
      routeOpenRequest({ target: { kind: "workspace", path: "secret-workspace" }, from: null }),
    ).toEqual({ kind: "error", message: "지원하지 않는 열기 요청입니다" });
  });
});
