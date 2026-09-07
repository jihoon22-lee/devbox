import { invoke } from "@tauri-apps/api/core";
import { configureProductTransport, type Component } from "@devbox/api-studio-features/transport";
import { describe, makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation, problemMessage, type Operation } from "@devbox/product-shell/operation";
import catalog from "../../../apps/products.json";

const routeFor: Record<Component, string> = {
  "api-studio.api": "requests", "api-studio.webhooks": "webhooks", "api-studio.transforms": "transforms",
};

configureProductTransport(async <T>(component: Component, method: string, args: Record<string, unknown>): Promise<T> => {
  if (!nativeMode) throw new Error("데스크톱 앱에서 사용할 수 있습니다.");
  const description = await describe("api-studio");
  const header = makeRequest(description.handshake, routeFor[component], Date.now(), description.context);
  const provenance = { product: "api-studio", component, requestId: header.requestId, revision: catalog.catalogRevision };
  let response: { operation: Operation; value: T };
  try {
    response = await invoke("plugin:api-studio|execute", { request: { header, component, method, args } });
  } catch (problem) { throw new Error(problemMessage(problem, provenance)); }
  if (!response || !isOperation(response.operation, provenance) || response.operation.outcome.state !== "succeeded") {
    throw new Error("작업 응답의 출처를 확인할 수 없습니다.");
  }
  return response.value;
});
