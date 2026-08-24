import { describe, expect, it } from "vitest";
import { routeOpenRequest, sameRepositoryKey } from "./applink";

describe("Repo Manager applink routing", () => {
  it("routes a bounded Path without changing it", () => {
    const path = "C:\\Projects\\Devbox";
    expect(
      routeOpenRequest({
        target: { kind: "path", path, line: null, column: null },
        from: "workbench",
      }),
    ).toEqual({ kind: "prepareRepository", path });
  });

  it("rejects empty, oversized, and NUL Path values without echoing them", () => {
    for (const path of ["", "x".repeat(32_768), "unsafe\0path"]) {
      const action = routeOpenRequest({
        target: { kind: "path", path, line: null, column: null },
        from: null,
      });
      expect(action).toEqual({
        kind: "error",
        message: "요청한 repository 경로를 사용할 수 없습니다",
      });
      if (path.length > 0) expect(JSON.stringify(action)).not.toContain(path);
    }
  });

  it("rejects non-Path targets with a generic recoverable error", () => {
    expect(
      routeOpenRequest({ target: { kind: "query", text: "secret" }, from: null }),
    ).toEqual({ kind: "error", message: "지원하지 않는 열기 요청입니다" });
  });

  it("matches Windows repository keys case-insensitively but preserves WSL case", () => {
    expect(sameRepositoryKey("win:c:/Projects/Devbox", "win:C:/projects/devbox")).toBe(true);
    expect(sameRepositoryKey("wsl:Ubuntu:home/User/repo", "wsl:Ubuntu:home/user/repo")).toBe(false);
  });
});
