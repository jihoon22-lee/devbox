import { invoke as legacyInvoke } from "@tauri-apps/api/core";

export type Component = "workspace.overview" | "workspace.source" | "workspace.dependencies" | "workspace.files" | "workspace.lsp" | "workspace.migration" | "workspace.runtime" | "workspace.processes" | "workspace.process-actions" | "workspace.logs" | "workspace.terminal";
export type Transport = <T>(component: Component, method: string, args: Record<string, unknown>) => Promise<T>;
let productTransport: Transport | undefined;
let installationId: string | undefined;
/** Only the native product bridge constructs this from fixed, validated messages. */
export class WorkspaceOperationError extends Error {
  constructor(message: string, readonly code?: string) { super(message); }
}

/** Installed once by native product startup, before any feature is mounted. */
export function configureProductTransport(transport: Transport, ownerInstallationId?: string): void {
  if (productTransport) throw new Error("제품 연결이 이미 설정되어 있습니다.");
  installationId = ownerInstallationId;
  productTransport = transport;
}
export function productInstallationId(): string {
  if (!installationId || !/^[A-Za-z0-9_-]{1,128}$/.test(installationId)) throw new Error("제품 설치 정보를 확인하지 못했습니다.");
  return installationId;
}
export function isProductHosted(): boolean { return productTransport !== undefined; }
export function componentInvoke(component: Component | ((method: string) => Component)) {
  return <T>(method: string, args?: Record<string, unknown>): Promise<T> =>
    productTransport ? productTransport<T>(typeof component === "function" ? component(method) : component, method, args ?? {})
      : args === undefined ? legacyInvoke<T>(method) : legacyInvoke<T>(method, args);
}
