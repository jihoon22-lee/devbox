import { matchAssertionRegex } from "./assertionRegex";
import { looksLikeSecret } from "./secretPatterns";
import type { ApiResponse } from "../types";
import { evaluateJsonPath } from "./jsonPath";
export type AssertionSource = "status" | "header" | "jsonPath" | "body" | "duration";
export type AssertionOperator =
  | "equals"
  | "notEquals"
  | "contains"
  | "notContains"
  | "matches"
  | "exists"
  | "notExists"
  | "lessThan"
  | "greaterThan";
export interface Assertion {
  id: string;
  enabled: boolean;
  source: AssertionSource;
  target: string;
  operator: AssertionOperator;
  expected: string;
}
export interface AssertionResult {
  id: string;
  passed: boolean;
  actual: string | null;
  message: string;
}
export const ASSERTION_SOURCES: AssertionSource[] = ["status", "header", "jsonPath", "body", "duration"];
export const ASSERTION_OPERATORS: AssertionOperator[] = [
  "equals",
  "notEquals",
  "contains",
  "notContains",
  "matches",
  "exists",
  "notExists",
  "lessThan",
  "greaterThan",
];
export function validateAssertion(a: Assertion): string | null {
  if (!ASSERTION_SOURCES.includes(a.source) || !ASSERTION_OPERATORS.includes(a.operator))
    return "검증 형식이 올바르지 않습니다";
  if (a.source === "header" && !a.target.trim()) return "헤더 이름이 필요합니다";
  if (a.source === "jsonPath") {
    try {
      evaluateJsonPath(null, a.target);
    } catch {
      return "지원하지 않는 JSONPath입니다";
    }
  }
  if (a.operator === "matches") {
    try {
      new RegExp(a.expected, "u");
    } catch {
      return "정규식이 올바르지 않습니다";
    }
  }
  return null;
}
export function responseValue(response: ApiResponse, source: AssertionSource, target: string): string | null {
  if (source === "status") return String(response.status);
  if (source === "duration") return String(response.duration_ms);
  if (source === "header")
    return response.headers.find((header) => header.key.toLowerCase() === target.toLowerCase())?.value ?? null;
  if (response.binary) throw new Error("binary 응답");
  if (source === "body") return response.body;
  if (!response.is_json) throw new Error("JSON이 아닌 응답");
  let json: unknown;
  try {
    json = JSON.parse(response.body);
  } catch {
    throw new Error("JSON이 아닌 응답");
  }
  const values = evaluateJsonPath(json, target);
  if (!values.length) return null;
  return typeof values[0] === "string" ? values[0] : JSON.stringify(values[0]);
}
const numeric = (value: string) => value.trim() !== "" && Number.isFinite(Number(value));
export async function evaluateAssertions(
  assertions: Assertion[],
  response: ApiResponse,
  signal?: AbortSignal,
): Promise<AssertionResult[]> {
  const evaluate = async (a: Assertion): Promise<AssertionResult> => {
    let actual: string | null = null;
    try {
      if (signal?.aborted) throw new Error("검증을 취소했습니다");
      const invalid = validateAssertion(a);
      if (invalid) return { id: a.id, passed: false, actual, message: invalid };
      actual = responseValue(response, a.source, a.target);
      let passed = false;
      if (a.operator === "exists") passed = actual !== null;
      else if (a.operator === "notExists") passed = actual === null;
      else if (actual === null) return { id: a.id, passed: false, actual, message: "값을 찾지 못했습니다" };
      else {
        const numbers = numeric(actual) && numeric(a.expected);
        switch (a.operator) {
          case "equals":
            passed = numbers ? Number(actual) === Number(a.expected) : actual === a.expected;
            break;
          case "notEquals":
            passed = numbers ? Number(actual) !== Number(a.expected) : actual !== a.expected;
            break;
          case "contains":
            passed = actual.includes(a.expected);
            break;
          case "notContains":
            passed = !actual.includes(a.expected);
            break;
          case "matches":
            passed = await matchAssertionRegex(a.expected, actual, signal);
            break;
          case "lessThan":
          case "greaterThan":
            if (!numbers) return { id: a.id, passed: false, actual, message: "숫자가 아닌 값입니다" };
            passed =
              a.operator === "lessThan" ? Number(actual) < Number(a.expected) : Number(actual) > Number(a.expected);
            break;
        }
      }
      return { id: a.id, passed, actual, message: passed ? "" : `기대 ${a.expected}, 실제 ${actual ?? "없음"}` };
    } catch (cause) {
      return {
        id: a.id,
        passed: false,
        actual,
        message: cause instanceof Error ? cause.message : "검증에 실패했습니다",
      };
    }
  };
  const results: AssertionResult[] = [];
  // Serialize workers so a collection cannot start fifty regex processes at once.
  for (const assertion of assertions.slice(0, 50).filter((item) => item.enabled)) {
    results.push(await evaluate(assertion));
  }
  return results;
}

export function cleanAssertions(value: unknown): { assertions: Assertion[]; dropped: number } {
  if (value === undefined) return { assertions: [], dropped: 0 };
  if (!Array.isArray(value)) return { assertions: [], dropped: 1 };
  const assertions: Assertion[] = [];
  const ids = new Set<string>();
  let dropped = 0;
  for (const item of value) {
    if (
      !item ||
      typeof item !== "object" ||
      assertions.length >= 50 ||
      typeof item.id !== "string" ||
      !item.id ||
      item.id.length > 128 ||
      ids.has(item.id) ||
      typeof item.enabled !== "boolean" ||
      typeof item.target !== "string" ||
      item.target.length > 256 ||
      typeof item.expected !== "string" ||
      new TextEncoder().encode(item.expected).length > 64 * 1024 ||
      !ASSERTION_SOURCES.includes(item.source) ||
      !ASSERTION_OPERATORS.includes(item.operator)
    ) {
      dropped++;
      continue;
    }
    ids.add(item.id);
    assertions.push({
      id: item.id,
      enabled: item.enabled,
      source: item.source,
      target: item.target,
      operator: item.operator,
      expected: looksLikeSecret(item.expected) ? "[REDACTED]" : item.expected,
    });
  }
  return { assertions, dropped };
}
