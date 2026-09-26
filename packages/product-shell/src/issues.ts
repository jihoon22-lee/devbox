import catalog from "../../../apps/products.json";
interface Fields {
  code: string;
  product: string;
  component: string;
  method: string;
  requestId: string;
}
function safe(fields: Fields): Fields {
  const product = catalog.products.some((entry) => entry.id === fields.product) ? fields.product : "unknown";
  return {
    product,
    component: catalog.components.some((entry) => entry.owner === product && entry.id === fields.component)
      ? fields.component
      : `${product}.shell`,
    method: /^[a-z][a-z0-9_]{0,95}$/.test(fields.method) ? fields.method : "unknown",
    code: /^[a-z][a-z0-9_-]{0,95}$/.test(fields.code) ? fields.code : "unavailable",
    requestId: /^[A-Za-z0-9_-]{1,128}$/.test(fields.requestId) ? fields.requestId : "unavailable",
  };
}
export class ProductIssueError extends Error {
  readonly code: string;
  readonly product: string;
  readonly component: string;
  readonly method: string;
  readonly requestId: string;
  readonly occurredAt: string;
  constructor(message: string, fields: Fields) {
    super(message);
    const checked = safe(fields);
    this.code = checked.code;
    this.name = checked.code;
    this.product = checked.product;
    this.component = checked.component;
    this.method = checked.method;
    this.requestId = checked.requestId;
    this.occurredAt = new Date().toISOString();
  }
}
let issues: readonly ProductIssueError[] = [];
const listeners = new Set<() => void>();
export function subscribeIssues(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
export function recordIssue(error: ProductIssueError): void {
  issues = [error, ...issues].slice(0, 20);
  for (const listener of listeners) listener();
}
export function recentIssues(): readonly ProductIssueError[] {
  return issues;
}
export function resetIssues(): void {
  issues = [];
  for (const listener of listeners) listener();
}
export function diagnosticText(error: ProductIssueError, version: string): string {
  const fields = safe(error);
  return JSON.stringify(
    {
      product: fields.product,
      version: /^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$/.test(version) ? version : "",
      component: fields.component,
      method: fields.method,
      code: fields.code,
      requestId: fields.requestId,
      occurredAt: /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(error.occurredAt) ? error.occurredAt : "",
    },
    null,
    2,
  );
}
export function issueFailure(message: string, fields: Fields): ProductIssueError {
  const error = new ProductIssueError(message, fields);
  recordIssue(error);
  return error;
}
