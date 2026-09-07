export interface Provenance { product: string; component: string; requestId: string; revision: number }
const messages = {
  unauthorized: "이 작업에 대한 연결 권한이 없습니다.",
  "invalid-request": "작업 요청을 확인할 수 없습니다.",
  expired: "작업 시간이 초과되었습니다.",
  replayed: "이미 처리한 요청입니다.",
  overloaded: "진행 중인 요청이 많습니다. 잠시 후 다시 시도해 주세요.",
  "stale-context": "프로젝트 연결이 변경되었습니다. 화면을 다시 열어 주세요.",
  unavailable: "작업 상태를 확인할 수 없습니다.",
} as const;
export type ProblemCode = keyof typeof messages;
export interface Problem { code: ProblemCode; provenance: Provenance }
export type OperationOutcome = { state: "running" | "succeeded" | "cancelled" | "stale" } | { state: "failed"; code: ProblemCode };
export interface Operation { provenance: Provenance; outcome: OperationOutcome }

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function code(value: unknown): value is ProblemCode {
  return typeof value === "string" && Object.prototype.hasOwnProperty.call(messages, value);
}
export function matchesProvenance(value: unknown, expected: Provenance): value is Provenance {
  return record(value) && Object.keys(value).sort().join(",") === "component,product,requestId,revision"
    && value.product === expected.product && value.component === expected.component
    && value.requestId === expected.requestId && value.revision === expected.revision
    && Number.isSafeInteger(value.revision) && (value.revision as number) > 0;
}
export function isOperation(value: unknown, expected: Provenance): value is Operation {
  if (!record(value) || Object.keys(value).sort().join(",") !== "outcome,provenance"
    || !matchesProvenance(value.provenance, expected) || !record(value.outcome)) return false;
  const outcome = value.outcome;
  return outcome.state === "failed"
    ? Object.keys(outcome).sort().join(",") === "code,state" && code(outcome.code)
    : Object.keys(outcome).join(",") === "state" && ["running", "succeeded", "cancelled", "stale"].includes(outcome.state as string);
}
export function problemMessage(value: unknown, expected: Provenance): string {
  if (!record(value) || Object.keys(value).sort().join(",") !== "code,provenance"
    || !matchesProvenance(value.provenance, expected) || !code(value.code)) return messages.unavailable;
  return messages[value.code];
}
export function operationMessage(operation: Operation): string {
  switch (operation.outcome.state) {
    case "running": return "작업이 진행 중입니다.";
    case "succeeded": return "작업을 완료했습니다.";
    case "cancelled": return "작업이 취소되었습니다.";
    case "stale": return "이 결과는 이전 상태의 결과입니다. 화면을 다시 열어 주세요.";
    case "failed": return messages[operation.outcome.code];
  }
}
