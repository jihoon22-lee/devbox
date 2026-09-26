import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { describe as describeProduct, type RouteRequest } from "@devbox/product-shell/api";
import { componentCall } from "./native";
import catalog from "../../products.json";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => false }));
afterEach(() => {
  vi.useRealTimers();
  vi.resetAllMocks();
});

it("accepts a bounded large-file read that completes after the old five-second UI deadline", async () => {
  const description = await describeProduct("workspace");
  vi.useFakeTimers();
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { request } = args as { request: { header: RouteRequest; component: string } };
    const remaining = request.header.deadlineMs - Date.now();
    expect(remaining).toBeLessThanOrEqual(30_000);
    await new Promise((resolve) => setTimeout(resolve, 6_000));
    return {
      operation: {
        provenance: {
          product: "workspace",
          component: "workspace.files",
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: request.header.deadlineMs > Date.now() ? { state: "succeeded" } : { state: "failed", code: "expired" },
      },
      value: { readOnly: true },
    };
  });
  const result = componentCall(
    description,
    "workspace.files",
    "open_file",
    { request: { path: "/fixture/큰 파일.txt", encoding: null } },
    "files",
  );
  const assertion = expect(result).resolves.toEqual({ readOnly: true });
  await vi.advanceTimersByTimeAsync(6_000);
  await assertion;
  expect(vi.mocked(invoke).mock.calls[0]?.[0]).toBe("plugin:workspace|files");
});

it("uses a native typed command without a renderer-selected owner field", async () => {
  const description = await describeProduct("workspace");
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    expect(command).toBe("plugin:workspace|runtime");
    const { request } = args as { request: { header: RouteRequest; method: string; args: Record<string, never> } };
    expect(Object.keys(request).sort()).toEqual(["args", "header", "method"]);
    expect(request.method).toBe("runtime_status");
    expect(request.header.deadlineMs - Date.now()).toBeGreaterThan(28_000);
    return {
      operation: {
        provenance: {
          product: "workspace",
          component: "workspace.runtime",
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "succeeded" },
      },
      value: { ready: true },
    };
  });
  await expect(componentCall(description, "workspace.runtime", "runtime_status", {}, "tasks")).resolves.toEqual({
    ready: true,
  });
});

it("records safe request diagnostics while preserving the Workspace error class", async () => {
  const { recentIssues, resetIssues, diagnosticText, ProductIssueError } = await import("@devbox/product-shell/issues");
  const { WorkspaceOperationError } = await import("@devbox/workspace-features/transport");
  resetIssues();
  const description = await describeProduct("workspace");
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { request } = args as { request: { header: RouteRequest } };
    return {
      operation: {
        provenance: {
          product: "workspace",
          component: "workspace.files",
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "failed", code: "unavailable" },
      },
      value: { issue: "file_changed" },
    };
  });
  const error = await componentCall(
    description,
    "workspace.files",
    "save_file",
    { path: "C:\\private\\note.md" },
    "files",
  ).catch((cause) => cause);
  expect(error).toBeInstanceOf(WorkspaceOperationError);
  expect(error).toBeInstanceOf(ProductIssueError);
  expect(recentIssues()).toHaveLength(1);
  expect(recentIssues()[0].method).toBe("save_file");
  expect(diagnosticText(recentIssues()[0], "0.8.1")).not.toContain("private");
  resetIssues();
});
