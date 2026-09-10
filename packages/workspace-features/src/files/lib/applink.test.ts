import { describe, expect, it } from "vitest";
import type { OpenRequest } from "../types";
import { routeOpenRequest } from "./applink";

describe("code-pad applink routing", () => {
  it("routes a path target to openFile, passing line/column through", () => {
    const request: OpenRequest = {
      target: { kind: "path", path: "/tmp/a.ts", line: 10, column: 5 },
      from: "repo-manager",
    };
    expect(routeOpenRequest(request)).toEqual({
      kind: "openFile",
      path: "/tmp/a.ts",
      line: 10,
      column: 5,
    });
  });

  it("routes a path target with no line/column", () => {
    const request: OpenRequest = {
      target: { kind: "path", path: "/tmp/a.ts", line: null, column: null },
      from: null,
    };
    expect(routeOpenRequest(request)).toEqual({
      kind: "openFile",
      path: "/tmp/a.ts",
      line: null,
      column: null,
    });
  });

  it("routes a workspace target to openWorkspace, reusing the folder-open path", () => {
    const request: OpenRequest = { target: { kind: "workspace", path: "/tmp/ws" }, from: "workbench" };
    expect(routeOpenRequest(request)).toEqual({ kind: "openWorkspace", path: "/tmp/ws" });
  });

  it("no-ops profile and query targets with a reason instead of failing silently", () => {
    const profileReq: OpenRequest = { target: { kind: "profile", id: "p-1" }, from: null };
    const queryReq: OpenRequest = { target: { kind: "query", text: "hello" }, from: null };
    expect(routeOpenRequest(profileReq)).toEqual({
      kind: "noop",
      reason: expect.stringContaining("profile"),
    });
    expect(routeOpenRequest(queryReq)).toEqual({
      kind: "noop",
      reason: expect.stringContaining("query"),
    });
  });
});
