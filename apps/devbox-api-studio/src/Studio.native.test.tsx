import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { fixtureDescription } from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import "./transport";
import Studio from "./Studio";

const native = vi.hoisted(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  return { invoke: vi.fn(), listeners: new Map<string, Set<(event: { payload: unknown }) => void>>() };
});
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (event: string, callback: (event: { payload: unknown }) => void) => {
  const set = native.listeners.get(event) ?? new Set(); set.add(callback); native.listeners.set(event, set);
  return () => { set.delete(callback); };
}) }));
afterEach(() => { cleanup(); localStorage.clear(); native.listeners.clear(); native.invoke.mockReset(); });
it("opens a native-owned pending transform preview through the actual hosted transport", async () => {
  const id = "a".repeat(32);
  let pending: unknown = null;
  let navigation: unknown = null;
  native.invoke.mockImplementation(async (command: string, args?: { request: { header: { requestId: string }; component: string; method: string; args: Record<string, unknown> } }) => {
    if (command === "plugin:product-shell|describe") return fixtureDescription("api-studio");
    if (command !== "plugin:api-studio|execute" || !args) throw new Error(`unexpected fixture command ${command}`);
    const request = args.request;
    let value: unknown;
    if (request.method === "migration_status") value = { busy: false, reviewNeeded: false, pending: null, sources: [] };
    else if (request.method === "finish_startup") value = null;
    else if (request.method === "peek_pending_navigation") value = navigation;
    else if (request.method === "ack_pending_navigation") { value = null; navigation = null; }
    else if (request.method === "take_pending_open") {
      value = request.component === "api-studio.transforms" ? pending : null;
      if (request.component === "api-studio.transforms") pending = null;
    } else if (request.method === "sanitize_persisted_json") value = request.args.serialized;
    else if (request.method === "load_workflow_metadata") value = { metadata: { schemaVersion: 1, recentTools: [], favoriteTools: [], pipelines: [] }, writable: true };
    else if (request.method === "preview_toolbox_text") value = { handoffId: id, producerId: "api-playground", expiresAtMs: Date.now() + 600_000, text: "fixture [REDACTED]", redacted: true };
    else if (request.method === "accept_toolbox_text") value = "fixture [REDACTED]";
    else if (["discard_current_response", "save_workflow_metadata"].includes(request.method)) value = null;
    else throw new Error(`unexpected fixture method ${request.method}`);
    return { operation: { provenance: { product: "api-studio", component: request.component, requestId: request.header.requestId, revision: catalog.catalogRevision }, outcome: { state: "succeeded" } }, value };
  });
  render(<Studio />);
  await screen.findByRole("navigation", { name: "제품 화면" });
  await act(async () => { await vi.dynamicImportSettled(); });
  // CI transforms the real shared route on first import; the DOM library's
  // one-second default can expire while that lazy chunk is still compiling.
  await screen.findByPlaceholderText("https://api.example.com/users", {}, { timeout: 10_000 });
  await waitFor(() => expect(native.listeners.get("api-studio://navigate")?.size).toBe(1));
  navigation = { id, route: "transforms" };
  pending = { target: { kind: "handoff", handoffKind: "toolbox-text/v1", id }, from: "api-playground" };
  await act(async () => {
    for (const callback of native.listeners.get("api-studio://navigate") ?? []) callback({ payload: "untrusted-event-is-not-a-route" });
  });
  await act(async () => { await vi.dynamicImportSettled(); });
  await screen.findByRole("dialog", { name: "Toolbox 텍스트 미리보기" }, { timeout: 10_000 });
  fireEvent.click(screen.getByRole("button", { name: "적용" }));
  await waitFor(() => expect((screen.getByRole("textbox", { name: "스마트 워크플로 입력" }) as HTMLTextAreaElement).value).toBe("fixture [REDACTED]"));
}, 30_000);
