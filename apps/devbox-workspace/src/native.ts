import { componentCommands, deadlineBudgets } from "@devbox/workspace-features/generated/deadline-budgets";
import { bindTypedCall } from "@devbox/workspace-features/typed";
import { invoke } from "@tauri-apps/api/core";
import { currentDescription, makeRequest, type Description } from "@devbox/product-shell/api";
import { isOperation, problemMessage } from "@devbox/product-shell/operation";
import { WorkspaceOperationError } from "@devbox/workspace-features/transport";
import catalog from "../../products.json";

export { workspaceIssueMessage as issueMessage } from "@devbox/workspace-features/issues/shared";
import { workspaceIssueMessage as issueMessage } from "@devbox/workspace-features/issues/shared";
export async function nativeCall<T>(
  component: string,
  method: string,
  args: Record<string, unknown> = {},
  route = "overview",
): Promise<T> {
  const description = await currentDescription("workspace");
  return componentCall(description, component, method, args, route);
}
/** Feature actions bind the description currently displayed by the caller. */
export async function componentCall<T>(
  description: Description,
  component: string,
  method: string,
  args: Record<string, unknown>,
  route: string,
): Promise<T> {
  const header = makeRequest(description.handshake, route, Date.now(), description.context);
  header.deadlineMs = Date.now() + (deadlineBudgets[component]?.[method] ?? 5_000);
  const provenance = {
    product: "workspace",
    component,
    requestId: header.requestId,
    revision: catalog.catalogRevision,
  };
  let response: { operation: unknown; value: T & { issue?: string } };
  try {
    const command = componentCommands[component];
    response = command
      ? await invoke(`plugin:workspace|${command}`, { request: { header, method, args } })
      : await invoke("plugin:workspace|execute", { request: { header, component, method, args } });
  } catch (problem) {
    throw new WorkspaceOperationError(problemMessage(problem, provenance));
  }
  if (!response || !isOperation(response.operation, provenance)) throw new Error("응답을 확인하지 못했습니다.");
  if (response.operation.outcome.state !== "succeeded") {
    const issue = response.value?.issue ?? "operation_failed";
    const message = /^(runtime_|process_|logs_owner_)/.test(issue)
      ? await import("./runtimeIssues").then((module) => module.runtimeIssueMessage(issue)).catch(() => undefined)
      : issue.startsWith("wsl_")
        ? await import("./wslIssues").then((module) => module.wslIssueMessage(issue)).catch(() => undefined)
        : undefined;
    throw new WorkspaceOperationError(message ?? issueMessage(issue), issue);
  }
  return response.value;
}

/** Bind generated call/result types to the exact context rendered by this view. */
export function typedComponentCall<
  Calls extends { method: string; args: unknown },
  Results extends Record<Calls["method"], unknown>,
>(description: Description, component: string, route: string) {
  return bindTypedCall<Calls, Results>((method, args) => componentCall(description, component, method, args, route));
}
