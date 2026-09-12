import { describe, expect, it } from "vitest";
import type { OpenRequest } from "../types";
import { routeOpenRequest } from "./applink";

describe("wsl-desktop applink routing", () => {
  it("routes a path target to openTerminal", () => {
    const request: OpenRequest = {
      target: { kind: "path", path: "/mnt/e/projects/devbox", line: null, column: null },
      from: "repo-manager",
    };
    expect(routeOpenRequest(request)).toEqual({ kind: "openTerminal", path: "/mnt/e/projects/devbox" });
  });

  it("ignores line/column on a path target — wsl-desktop only uses the path", () => {
    const request: OpenRequest = {
      target: { kind: "path", path: "/tmp/x", line: 5, column: 2 },
      from: null,
    };
    expect(routeOpenRequest(request)).toEqual({ kind: "openTerminal", path: "/tmp/x" });
  });

  it("routes profile to the named terminal workspace", () => {
    const request: OpenRequest = { target: { kind: "profile", id: "p-1" }, from: "workbench" };
    expect(routeOpenRequest(request)).toEqual({ kind: "openProfile", id: "p-1" });
  });

  it("no-ops workspace and query targets with a reason instead of failing silently", () => {
    const workspaceReq: OpenRequest = { target: { kind: "workspace", path: "/tmp/ws" }, from: null };
    const queryReq: OpenRequest = { target: { kind: "query", text: "hello" }, from: null };
    expect(routeOpenRequest(workspaceReq)).toEqual({
      kind: "noop",
      reason: expect.stringContaining("workspace"),
    });
    expect(routeOpenRequest(queryReq)).toEqual({
      kind: "noop",
      reason: expect.stringContaining("query"),
    });
  });
});
