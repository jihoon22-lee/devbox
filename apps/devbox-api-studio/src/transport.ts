import { issueFailure } from "@devbox/product-shell/issues";
import { invoke } from "@tauri-apps/api/core";
import { configureProductTransport, type Component } from "@devbox/api-studio-features/transport";
import { currentDescription, makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation, problemCode, problemMessage, type Operation } from "@devbox/product-shell/operation";
import { componentFailure } from "./componentErrors";
import catalog from "../../../apps/products.json";

const routeFor: Record<Component, string> = {
  "api-studio.api": "requests",
  "api-studio.webhooks": "webhooks",
  "api-studio.transforms": "transforms",
  "api-studio.store": "requests",
};

const commandFor: Record<Component, string> = {
  "api-studio.api": "plugin:api-studio|api",
  "api-studio.webhooks": "plugin:api-studio|webhooks",
  "api-studio.transforms": "plugin:api-studio|transforms",
  "api-studio.store": "plugin:api-studio|store",
};
configureProductTransport(
  async <T>(component: Component, method: string, args: Record<string, unknown>): Promise<T> => {
    if (!nativeMode) throw new Error("데스크톱 앱에서 사용할 수 있습니다.");
    const description = await currentDescription("api-studio");
    const header = makeRequest(description.handshake, routeFor[component], Date.now(), description.context);
    if (method === "send_knowledge_draft" || method === "open_workspace_selection")
      header.deadlineMs = Date.now() + 29000;
    const provenance = {
      product: "api-studio",
      component,
      requestId: header.requestId,
      revision: catalog.catalogRevision,
    };
    let response: { operation: Operation; value: T };
    try {
      response = await invoke(commandFor[component], { request: { header, method, args } });
    } catch (problem) {
      throw issueFailure(problemMessage(problem, provenance), {
        ...provenance,
        method,
        code: problemCode(problem, provenance),
      });
    }
    if (!response || !isOperation(response.operation, provenance)) {
      throw issueFailure("작업 응답의 출처를 확인할 수 없습니다.", { ...provenance, method, code: "invalid_response" });
    }
    if (response.operation.outcome.state !== "succeeded") {
      const error = componentFailure(component, response.value);
      throw issueFailure(error.message, {
        ...provenance,
        method,
        code: error.name === "Error" ? "unavailable" : error.name,
      });
    }
    return response.value;
  },
);
