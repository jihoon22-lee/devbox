import { toolsMessages } from "@devbox/control-center-features/issues";
import ManagerTools, { type ToolsMode } from "@devbox/control-center-features/manager";
import { configureProductTransport, type Component } from "@devbox/control-center-features/transport";
import { currentDescription, makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation } from "@devbox/product-shell/operation";
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
    const response = await invoke<{ operation: unknown; value: T }>("plugin:control-center|tools", {
      request: { header, method, args },
    });
    if (!isOperation(response.operation, provenance) || response.operation.outcome.state !== "succeeded") {
      const issue = (response.value as { issue?: unknown } | null)?.issue;
      throw new Error(
        typeof issue === "string" && Object.prototype.hasOwnProperty.call(toolsMessages, issue)
          ? toolsMessages[issue as keyof typeof toolsMessages]
          : toolsMessages.unavailable,
      );
    }
    return response.value;
  },
);
export default function Tools({ route }: { route: string }) {
  const mode: ToolsMode = route === "environment" ? "dev-setup" : route === "tools" ? "related-tools" : "doctor";
  return <ManagerTools key={route} mode={mode} />;
}
