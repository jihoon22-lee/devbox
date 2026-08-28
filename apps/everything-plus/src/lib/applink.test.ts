import { describe, expect, it } from "vitest";
import { routeOpenRequest } from "./applink";

describe("Everything+ applink routing", () => {
  it("trims and routes a bounded Query", () => {
    expect(
      routeOpenRequest({ target: { kind: "query", text: "  Cargo.toml  " }, from: "devbox-launcher" }),
    ).toEqual({ kind: "search", query: "Cargo.toml" });
  });

  it("normalizes and applies a bounded v1 filter", () => {
    expect(
      routeOpenRequest({
        target: {
          kind: "query",
          text: " cargo ",
          filter: { extensions: [".RS", "rs"], minSize: 10, sourceRootId: 2, contentStatus: "TRUNCATED" },
        },
        from: "devbox-launcher",
      }),
    ).toEqual({
      kind: "search",
      query: "cargo",
      filter: { extensions: ["rs"], minSize: 10, sourceRootId: 2, contentStatus: "truncated" },
    });
  });

  it("rejects an invalid filter without echoing its values", () => {
    const action = routeOpenRequest({
      target: { kind: "query", text: "cargo", filter: { sourceRootId: -1 } },
      from: "devbox-launcher",
    });
    expect(action).toEqual({ kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" });
    expect(JSON.stringify(action)).not.toContain("-1");
  });

  it("fails closed for malformed status and unknown filter fields", () => {
    expect(
      routeOpenRequest({
        target: { kind: "query", text: "cargo", filter: { contentStatus: 42 as never } },
        from: null,
      }),
    ).toEqual({ kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" });
    expect(
      routeOpenRequest({
        target: { kind: "query", text: "cargo", filter: { futureField: "ignored" } as never },
        from: null,
      }),
    ).toEqual({ kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" });
    expect(
      routeOpenRequest({
        target: { kind: "query", text: "cargo", filter: { extensions: null } as never },
        from: null,
      }),
    ).toEqual({ kind: "error", message: "요청한 검색 필터를 사용할 수 없습니다" });
  });

  it("rejects empty and oversized Query without echoing it", () => {
    for (const text of ["   ", "x".repeat(513), "unsafe\0query"]) {
      const action = routeOpenRequest({ target: { kind: "query", text }, from: null });
      expect(action).toEqual({ kind: "error", message: "요청한 검색어를 사용할 수 없습니다" });
      expect(JSON.stringify(action)).not.toContain(text);
    }
  });

  it("rejects non-Query targets with a generic recoverable error", () => {
    expect(
      routeOpenRequest({ target: { kind: "path", path: "secret-path", line: null, column: null }, from: null }),
    ).toEqual({ kind: "error", message: "지원하지 않는 열기 요청입니다" });
  });
});
