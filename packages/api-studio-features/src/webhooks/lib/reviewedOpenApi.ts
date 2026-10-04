import { confirmAction } from "@devbox/product-shell/confirm";
import { openApiOperationToRule, type OpenApiRuleOperation } from "./openapiRules";

export async function reviewedOpenApiDraft(operation: OpenApiRuleOperation | undefined) {
  if (!operation?.applyable) return null;
  const draft = openApiOperationToRule(operation);
  if (
    !draft ||
    !(await confirmAction(
      `${operation.method} ${operation.path} → ${operation.status} operation을 규칙 초안으로 편집기에 채울까요?`,
    ))
  )
    return null;
  return draft;
}
