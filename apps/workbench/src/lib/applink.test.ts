import { describe, expect, it } from "vitest";
import type { OpenRequest, ProjectProfile } from "../api";
import { routeOpenRequest } from "./applink";

function profile(overrides: Partial<ProjectProfile>): ProjectProfile {
  return {
    id: "p-1",
    name: "devbox",
    windowsPath: null,
    wsl: null,
    gitRoot: null,
    expectedPorts: [],
    runManagerServiceIds: [],
    environment: null,
    ...overrides,
  };
}

describe("workbench applink routing", () => {
  it("selects the profile matching windowsPath", () => {
    const profiles = [profile({ id: "p-1", windowsPath: "C:\\projects\\devbox" })];
    const request: OpenRequest = {
      target: { kind: "path", path: "C:\\projects\\devbox", line: null, column: null },
      from: "repo-manager",
    };
    expect(routeOpenRequest(request, profiles)).toEqual({ kind: "selectProfile", profileId: "p-1" });
  });

  it("matches windowsPath case-insensitively and across slash styles", () => {
    const profiles = [profile({ id: "p-1", windowsPath: "C:\\projects\\devbox" })];
    const request: OpenRequest = {
      target: { kind: "path", path: "c:/projects/devbox", line: null, column: null },
      from: null,
    };
    expect(routeOpenRequest(request, profiles)).toEqual({ kind: "selectProfile", profileId: "p-1" });
  });

  it("selects the profile matching wsl.path", () => {
    const profiles = [profile({ id: "p-2", wsl: { distro: "Ubuntu", path: "/mnt/e/projects/devbox" } })];
    const request: OpenRequest = {
      target: { kind: "path", path: "/mnt/e/projects/devbox", line: null, column: null },
      from: "repo-manager",
    };
    expect(routeOpenRequest(request, profiles)).toEqual({ kind: "selectProfile", profileId: "p-2" });
  });

  it("drafts a new profile from a Windows-shaped path when nothing matches", () => {
    const request: OpenRequest = {
      target: { kind: "path", path: "C:\\projects\\other", line: null, column: null },
      from: "repo-manager",
    };
    expect(routeOpenRequest(request, [])).toEqual({
      kind: "draftProfile",
      path: "C:\\projects\\other",
      looksWindows: true,
    });
  });

  it("drafts a new profile from a WSL-shaped path when nothing matches", () => {
    const request: OpenRequest = {
      target: { kind: "path", path: "/mnt/e/projects/other", line: null, column: null },
      from: "repo-manager",
    };
    expect(routeOpenRequest(request, [])).toEqual({
      kind: "draftProfile",
      path: "/mnt/e/projects/other",
      looksWindows: false,
    });
  });

  it("no-ops profile, workspace and query targets with a reason instead of failing silently", () => {
    const profileReq: OpenRequest = { target: { kind: "profile", id: "p-1" }, from: null };
    const workspaceReq: OpenRequest = { target: { kind: "workspace", path: "/tmp/ws" }, from: null };
    const queryReq: OpenRequest = { target: { kind: "query", text: "hello" }, from: null };
    expect(routeOpenRequest(profileReq, [])).toEqual({
      kind: "noop",
      reason: expect.stringContaining("profile"),
    });
    expect(routeOpenRequest(workspaceReq, [])).toEqual({
      kind: "noop",
      reason: expect.stringContaining("workspace"),
    });
    expect(routeOpenRequest(queryReq, [])).toEqual({
      kind: "noop",
      reason: expect.stringContaining("query"),
    });
  });
});
