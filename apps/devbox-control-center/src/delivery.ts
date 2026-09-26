import { issueFailure } from "@devbox/product-shell/issues";
import { invoke } from "@tauri-apps/api/core";
import { makeRequest } from "@devbox/product-shell/api";
import { isOperation, problemCode, problemMessage, type Operation } from "@devbox/product-shell/operation";
import type { DeliveryCall } from "@devbox/control-center-features/generated/DeliveryCall";
import type { DeliveryResults } from "@devbox/control-center-features/generated/delivery-results";
import { deliveryMessages } from "@devbox/control-center-features/issues";
import catalog from "../../../apps/products.json";
type Args<M> = Extract<DeliveryCall, { method: M }> extends { args: infer A } ? A : never;
export async function deliveryCall<M extends DeliveryCall["method"]>(
  header: ReturnType<typeof makeRequest>,
  method: M,
  args: Args<M>,
): Promise<{ operation: Operation; value: DeliveryResults[M] }> {
  const provenance = {
    product: "control-center",
    component: "control-center.delivery",
    requestId: header.requestId,
    revision: catalog.catalogRevision,
  };
  let response: { operation: Operation; value: DeliveryResults[M] };
  try {
    response = await invoke("plugin:control-center|delivery", { request: { header, method, args } });
  } catch (problem) {
    throw issueFailure(problemMessage(problem, provenance), {
      ...provenance,
      method,
      code: problemCode(problem, provenance),
    });
  }
  if (!response || !isOperation(response.operation, provenance))
    throw issueFailure("작업 응답의 출처를 확인할 수 없습니다.", { ...provenance, method, code: "invalid_response" });
  if (response.operation.outcome.state !== "succeeded") {
    const value = response.value as { issue?: unknown } | null;
    const issue = value?.issue;
    const known = typeof issue === "string" && Object.prototype.hasOwnProperty.call(deliveryMessages, issue);
    throw issueFailure(
      known ? deliveryMessages[issue as keyof typeof deliveryMessages] : deliveryMessages.unavailable,
      { ...provenance, method, code: known ? (issue as string) : "unavailable" },
    );
  }
  return response;
}
