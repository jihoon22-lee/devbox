import { beforeEach, expect, it, vi } from "vitest";
import catalog from "../../../apps/products.json";

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  transport: undefined as
    | undefined
    | ((component: string, method: string, args: Record<string, unknown>) => Promise<unknown>),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@devbox/knowledge-features/transport", () => ({
  configureProductTransport: (transport: typeof native.transport) => {
    native.transport = transport;
  },
}));
vi.mock("@devbox/product-shell/api", () => ({
  nativeMode: true,
  currentDescription: async () => ({ handshake: {}, context: undefined }),
  makeRequest: (_handshake: unknown, route: string) => ({
    protocolVersion: 1,
    installationId: "i",
    sessionId: "s",
    requestId: "r",
    deadlineMs: 6000,
    route,
  }),
}));
import "./transport";

beforeEach(() => native.invoke.mockReset());
it("sends Activity through the typed command without a renderer-selected component", async () => {
  native.invoke.mockImplementation(async (command, { request }) => {
    expect(command).toBe("plugin:knowledge|activity");
    expect(request.method).toBe("is_tracking");
    expect(request.args).toEqual({});
    expect(request).not.toHaveProperty("component");
    return {
      operation: {
        provenance: {
          product: "knowledge",
          component: "knowledge.activity",
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "succeeded" },
      },
      value: true,
    };
  });
  await expect(native.transport!("knowledge.activity", "is_tracking", {})).resolves.toBe(true);
});
it("does not reflect an unknown native error value into the Activity screen", async () => {
  native.invoke.mockResolvedValue({
    operation: {
      provenance: {
        product: "knowledge",
        component: "knowledge.activity",
        requestId: "r",
        revision: catalog.catalogRevision,
      },
      outcome: { state: "failed", code: "unavailable" },
    },
    value: { issue: "private_remote_value" },
  });
  await expect(native.transport!("knowledge.activity", "is_tracking", {})).rejects.toThrow(
    "활동 작업을 완료하지 못했습니다",
  );
});
it.each([
  ["knowledge.notes", "notes", "notes"],
  ["knowledge.search", "search", "search"],
  ["knowledge.search-settings", "search_settings", "search"],
  ["knowledge.opener", "opener", "search"],
  ["knowledge.setup", "setup", "notes"],
  ["knowledge.commands", "commands", "notes"],
])("uses the declared native command for %s", async (component, command, route) => {
  native.invoke.mockImplementation(async (name, { request }) => {
    expect(name).toBe(`plugin:knowledge|${command}`);
    expect(request.header.route).toBe(route);
    expect(request).not.toHaveProperty("component");
    return {
      operation: {
        provenance: {
          product: "knowledge",
          component,
          requestId: request.header.requestId,
          revision: catalog.catalogRevision,
        },
        outcome: { state: "succeeded" },
      },
      value: null,
    };
  });
  await expect(native.transport!(component, "synthetic", {})).resolves.toBeNull();
});
