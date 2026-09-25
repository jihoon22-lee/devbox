import { invoke as legacyInvoke } from "@tauri-apps/api/core";

export type Component =
  | "knowledge.notes"
  | "knowledge.activity"
  | "knowledge.search"
  | "knowledge.search-settings"
  | "knowledge.opener"
  | "knowledge.setup"
  | "knowledge.commands";
export type Transport = <T>(component: Component, method: string, args: Record<string, unknown>) => Promise<T>;
let productTransport: Transport | undefined;

export function configureProductTransport(transport: Transport): void {
  if (productTransport) throw new Error("제품 연결이 이미 설정되어 있습니다.");
  productTransport = transport;
}
export function isProductHosted(): boolean {
  return productTransport !== undefined;
}

export function componentInvoke(component: Component) {
  return <T>(method: string, args?: Record<string, unknown>): Promise<T> => {
    if (!productTransport) return args === undefined ? legacyInvoke<T>(method) : legacyInvoke<T>(method, args);
    return productTransport<T>(component, method, args ?? {});
  };
}
