import { invoke as legacyInvoke } from "@tauri-apps/api/core";

export type Component = "knowledge.notes" | "knowledge.activity" | "knowledge.search" | "knowledge.search-settings" | "knowledge.opener" | "knowledge.migration";
export type Transport = <T>(component: Component, method: string, args: Record<string, unknown>) => Promise<T>;
let productTransport: Transport | undefined;

export function configureProductTransport(transport: Transport): void {
  if (productTransport) throw new Error("제품 연결이 이미 설정되어 있습니다.");
  productTransport = transport;
}
export function isProductHosted(): boolean { return productTransport !== undefined; }

const searchSettings = new Set(["add_root", "remove_root", "index_now", "cancel_index", "save_saved_query", "delete_saved_query"]);
const searchOpeners = new Set(["open_file", "reveal_file", "open_targets", "open_in"]);
export function componentInvoke(component: Component) {
  return <T>(method: string, args?: Record<string, unknown>): Promise<T> => {
    if (!productTransport) return args === undefined ? legacyInvoke<T>(method) : legacyInvoke<T>(method, args);
    const owner = component === "knowledge.search"
      ? searchSettings.has(method) ? "knowledge.search-settings" : searchOpeners.has(method) ? "knowledge.opener" : component
      : component;
    return productTransport<T>(owner, method, args ?? {});
  };
}
