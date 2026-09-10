import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { describe as describeProduct, type RouteRequest } from "@devbox/product-shell/api";
import { componentCall } from "./native";
import catalog from "../../products.json";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => false }));
afterEach(() => { vi.useRealTimers(); vi.resetAllMocks(); });

it("accepts a bounded large-file read that completes after the old five-second UI deadline", async () => {
  const description = await describeProduct("workspace");
  vi.useFakeTimers();
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const { request } = args as { request: { header: RouteRequest; component: string } };
    const remaining = request.header.deadlineMs - Date.now();
    expect(remaining).toBeLessThanOrEqual(30_000);
    await new Promise(resolve => setTimeout(resolve, 6_000));
    return {
      operation: {
        provenance: { product: "workspace", component: request.component, requestId: request.header.requestId, revision: catalog.catalogRevision },
        outcome: request.header.deadlineMs > Date.now() ? { state: "succeeded" } : { state: "failed", code: "expired" },
      },
      value: { readOnly: true },
    };
  });
  const result = componentCall(description, "workspace.files", "open_file", { request: { path: "/fixture/큰 파일.txt", encoding: null } }, "files");
  const assertion = expect(result).resolves.toEqual({ readOnly: true });
  await vi.advanceTimersByTimeAsync(6_000);
  await assertion;
});
