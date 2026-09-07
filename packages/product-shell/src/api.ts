import { invoke, isTauri } from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";

export type ProductId = "workspace" | "api-studio" | "knowledge" | "control-center";
export type Feature = typeof catalog.features[number];
export interface ProjectContext {
  projectId: string;
  worktreeId: string;
  target: { kind: "windows" } | { kind: "wsl"; distroId: string };
  revision: number;
}
export interface Handshake { protocolVersion: number; product: string; installationId: string; sessionId: string }
export interface Description { handshake: Handshake; product: typeof catalog.products[number]; features: Feature[]; context: ProjectContext | null }
export interface RouteRequest { protocolVersion: number; installationId: string; sessionId: string; requestId: string; deadlineMs: number; route: string; context?: ProjectContext }
export interface RouteStatus { route: string; availability: string; provenance: { product: string; component: string; requestId: string; revision: number } }

export const nativeMode = isTauri();

export function isProjectContext(value: unknown): value is ProjectContext {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const context = value as Record<string, unknown>;
  const opaqueId = (id: unknown) => typeof id === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(id);
  if (Object.keys(context).sort().join(",") !== "projectId,revision,target,worktreeId"
    || !opaqueId(context.projectId) || !opaqueId(context.worktreeId)
    || !Number.isSafeInteger(context.revision) || (context.revision as number) < 1) return false;
  if (!context.target || typeof context.target !== "object" || Array.isArray(context.target)) return false;
  const target = context.target as Record<string, unknown>;
  return target.kind === "windows" ? Object.keys(target).join(",") === "kind"
    : target.kind === "wsl" && Object.keys(target).sort().join(",") === "distroId,kind" && opaqueId(target.distroId);
}

export function fixtureDescription(product: ProductId): Description {
  const entry = catalog.products.find((p) => p.id === product);
  if (!entry) throw new Error("제품을 찾을 수 없습니다.");
  return { handshake: { protocolVersion: 1, product, installationId: "browser-fixture", sessionId: "browser-fixture" }, product: entry, features: catalog.features.filter((f) => f.owner === product), context: null };
}

export async function describe(product: ProductId): Promise<Description> {
  const result = nativeMode ? await invoke<Description>("plugin:product-shell|describe") : fixtureDescription(product);
  if (result.handshake.protocolVersion !== 1 || result.handshake.product !== product || result.product.id !== product
    || result.features.some((feature) => feature.owner !== product)
    || (result.context !== null && !isProjectContext(result.context))) throw new Error("제품 연결 정보를 확인할 수 없습니다.");
  return result;
}

export function makeRequest(handshake: Handshake, route: string, now = Date.now(), context: ProjectContext | null = null): RouteRequest {
  if (context !== null && !isProjectContext(context)) throw new Error("프로젝트 연결 정보를 확인할 수 없습니다.");
  return { protocolVersion: handshake.protocolVersion, installationId: handshake.installationId, sessionId: handshake.sessionId, requestId: crypto.randomUUID(), deadlineMs: now + 5000, route, ...(context ? { context } : {}) };
}

export async function routeStatus(description: Description, route: string): Promise<RouteStatus> {
  if (!description.features.some((f) => f.route === route)) throw new Error("지원하지 않는 화면입니다.");
  const request = makeRequest(description.handshake, route, Date.now(), description.context);
  const response = nativeMode ? await invoke<RouteStatus>("plugin:product-shell|route_status", { request }) : {
    route, availability: "foundation", provenance: { product: description.product.id, component: `${description.product.id}.shell`, requestId: request.requestId, revision: catalog.catalogRevision },
  };
  if (response.route !== route || response.provenance.product !== description.product.id || response.provenance.requestId !== request.requestId
    || response.provenance.component !== `${description.product.id}.shell` || response.provenance.revision !== catalog.catalogRevision) {
    throw new Error("화면 응답의 출처가 일치하지 않습니다.");
  }
  return response;
}
