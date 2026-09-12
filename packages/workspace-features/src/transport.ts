import { invoke as legacyInvoke } from "@tauri-apps/api/core";

export type Component = "workspace.overview" | "workspace.source" | "workspace.dependencies" | "workspace.files" | "workspace.lsp" | "workspace.migration" | "workspace.runtime" | "workspace.processes" | "workspace.process-actions" | "workspace.logs";
export type Transport = <T>(component: Component, method: string, args: Record<string, unknown>) => Promise<T>;
let productTransport: Transport | undefined;
/** Only the native product bridge constructs this from fixed, validated messages. */
export class WorkspaceOperationError extends Error {}

/** Installed once by native product startup, before any feature is mounted. */
export function configureProductTransport(transport: Transport): void {
  if (productTransport) throw new Error("제품 연결이 이미 설정되어 있습니다.");
  productTransport = transport;
}
export function isProductHosted(): boolean { return productTransport !== undefined; }
export function componentInvoke(component: Component | ((method: string) => Component)) {
  return <T>(method: string, args?: Record<string, unknown>): Promise<T> =>
    productTransport ? productTransport<T>(typeof component === "function" ? component(method) : component, method, args ?? {})
      : args === undefined ? legacyInvoke<T>(method) : legacyInvoke<T>(method, args);
}
