import { issueFailure } from "@devbox/product-shell/issues";
import { toolsMessages } from "@devbox/control-center-features/issues";
import ManagerTools, { type ToolsMode } from "@devbox/control-center-features/manager";
import { configureProductTransport, type Component } from "@devbox/control-center-features/transport";
import { currentDescription, makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation, problemCode, problemMessage } from "@devbox/product-shell/operation";
import { invoke } from "@tauri-apps/api/core";
import catalog from "../../../apps/products.json";
const routeFor = (method: string) =>
  /dev_setup/.test(method) ? "environment" : /related_(tool|url)/.test(method) ? "tools" : "diagnostics";
configureProductTransport(
  async <T,>(component: Component, method: string, args: Record<string, unknown>): Promise<T> => {
    if (component !== "control-center.tools") throw new Error("도구 소유자를 확인해 주세요.");
    if (!nativeMode) throw new Error("데스크톱 앱에서 사용할 수 있습니다.");
    const description = await currentDescription("control-center");
    const header = makeRequest(description.handshake, routeFor(method), Date.now(), description.context);
    const provenance = {
      product: "control-center",
      component: "control-center.tools",
      requestId: header.requestId,
      revision: catalog.catalogRevision,
    };
    let response: { operation: unknown; value: T };
    try {
      response = await invoke("plugin:control-center|tools", { request: { header, method, args } });
    } catch (problem) {
      throw issueFailure(problemMessage(problem, provenance), {
        ...provenance,
        method,
        code: problemCode(problem, provenance),
      });
    }
    if (!response || !isOperation(response.operation, provenance))
      throw issueFailure(toolsMessages.unavailable, { ...provenance, method, code: "invalid_response" });
    if (response.operation.outcome.state !== "succeeded") {
      const issue = (response.value as { issue?: unknown } | null)?.issue;
      const known = typeof issue === "string" && Object.prototype.hasOwnProperty.call(toolsMessages, issue);
      throw issueFailure(known ? toolsMessages[issue as keyof typeof toolsMessages] : toolsMessages.unavailable, {
        ...provenance,
        method,
        code: known ? (issue as string) : "unavailable",
      });
    }
    return response.value;
  },
);
export default function Tools({ route }: { route: string }) {
  const mode: ToolsMode = route === "environment" ? "dev-setup" : route === "tools" ? "related-tools" : "doctor";
  return <ManagerTools key={route} mode={mode} />;
}
