import { invoke as legacyInvoke } from "@tauri-apps/api/core";

export type Component = "api-studio.api" | "api-studio.webhooks" | "api-studio.transforms" | "api-studio.migration";
export type Transport = <T>(component: Component, method: string, args: Record<string, unknown>) => Promise<T>;
let productTransport: Transport | undefined;

/** Set by the native product entry point before mounting any feature. */
export function configureProductTransport(transport: Transport): void {
  if (productTransport) throw new Error("제품 연결이 이미 설정되어 있습니다.");
  productTransport = transport;
}
export function isProductHosted(): boolean { return productTransport !== undefined; }
export function componentInvoke(component: Component) {
  return <T>(method: string, args?: Record<string, unknown>): Promise<T> =>
    productTransport ? productTransport<T>(component, method, args ?? {})
      : args === undefined ? legacyInvoke<T>(method) : legacyInvoke<T>(method, args);
}
