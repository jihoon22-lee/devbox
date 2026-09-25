import { invoke } from "@tauri-apps/api/core";
import { configureProductTransport, type Component } from "@devbox/knowledge-features/transport";
import { currentDescription, makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation, problemMessage, type Operation } from "@devbox/product-shell/operation";
import catalog from "../../../apps/products.json";
const routeFor: Record<Component, string> = {
  "knowledge.commands": "notes",
  "knowledge.notes": "notes",
  "knowledge.activity": "activity",
  "knowledge.search": "search",
  "knowledge.search-settings": "search",
  "knowledge.opener": "search",
  "knowledge.setup": "notes",
};
const commandFor: Record<Component, string> = {
  "knowledge.activity": "plugin:knowledge|activity",
  "knowledge.notes": "plugin:knowledge|notes",
  "knowledge.search": "plugin:knowledge|search",
  "knowledge.search-settings": "plugin:knowledge|search_settings",
  "knowledge.opener": "plugin:knowledge|opener",
  "knowledge.setup": "plugin:knowledge|setup",
  "knowledge.commands": "plugin:knowledge|commands",
};
import { issueError } from "./issues";
export { issueError } from "./issues";
configureProductTransport(
  async <T>(component: Component, method: string, args: Record<string, unknown>): Promise<T> => {
    if (!nativeMode) throw new Error("데스크톱 앱에서 사용할 수 있습니다.");
    const description = await currentDescription("knowledge");
    const header = makeRequest(description.handshake, routeFor[component], Date.now(), description.context);
    if (component === "knowledge.opener" && method === "open_in") header.deadlineMs = Date.now() + 29000;
    const provenance = {
      product: "knowledge",
      component,
      requestId: header.requestId,
      revision: catalog.catalogRevision,
    };
    let response: { operation: Operation; value: T };
    try {
      response = await invoke(commandFor[component], { request: { header, method, args } });
    } catch (problem) {
      throw new Error(problemMessage(problem, provenance));
    }
    if (!response || !isOperation(response.operation, provenance))
      throw new Error("작업 응답의 출처를 확인할 수 없습니다.");
    if (response.operation.outcome.state !== "succeeded") {
      const value = response.value as { issue?: unknown } | null;
      throw issueError(value?.issue, component);
    }
    return response.value;
  },
);
