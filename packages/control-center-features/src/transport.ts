import { invoke as legacyInvoke } from "@tauri-apps/api/core";
export type Component = "control-center.tools" | "control-center.delivery";
export type Transport = <T>(component: Component, method: string, args: Record<string, unknown>) => Promise<T>;
let product: Transport | undefined;
export function configureProductTransport(transport: Transport) {
  if (product) throw new Error("제품 연결이 이미 설정되어 있습니다.");
  product = transport;
}
export function componentInvoke(component: Component) {
  return <T>(method: string, args?: Record<string, unknown>): Promise<T> =>
    product
      ? product(component, method, args ?? {})
      : args === undefined
        ? legacyInvoke(method)
        : legacyInvoke(method, args);
}
export function isProductHosted() {
  return product !== undefined;
}
