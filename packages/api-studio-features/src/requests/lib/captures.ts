import type { ApiResponse } from "../types";
import { responseValue } from "./assertions";
export const VARIABLE_NAME = /^[A-Za-z_][A-Za-z0-9_.-]{0,63}$/;
export interface Capture {
  id: string;
  enabled: boolean;
  variable: string;
  source: "jsonPath" | "header" | "status";
  target: string;
}
export interface CaptureOutcome {
  values: Map<string, string>;
  missing: string[];
  errors: string[];
}
export function applyCaptures(captures: Capture[], response: ApiResponse): CaptureOutcome {
  const values = new Map<string, string>();
  const missing = new Set<string>();
  const errors: string[] = [];
  for (const capture of captures.slice(0, 20)) {
    if (!capture.enabled) continue;
    if (!VARIABLE_NAME.test(capture.variable)) {
      errors.push(`변수 이름이 올바르지 않습니다: ${capture.variable}`);
      continue;
    }
    try {
      const value = responseValue(response, capture.source, capture.target);
      if (value === null) {
        values.delete(capture.variable);
        missing.add(capture.variable);
        continue;
      }
      if (new TextEncoder().encode(value).length > 64 * 1024) throw new Error("캡처 값이 64KiB 한도를 넘었습니다");
      values.set(capture.variable, value);
      missing.delete(capture.variable);
    } catch (cause) {
      values.delete(capture.variable);
      missing.add(capture.variable);
      errors.push(cause instanceof Error ? cause.message : "값을 캡처하지 못했습니다");
    }
  }
  return { values, missing: [...missing], errors };
}

export function cleanCaptures(value: unknown): { captures: Capture[]; dropped: number } {
  if (value === undefined) return { captures: [], dropped: 0 };
  if (!Array.isArray(value)) return { captures: [], dropped: 1 };
  const captures: Capture[] = [];
  const ids = new Set<string>();
  let dropped = 0;
  for (const item of value) {
    if (
      !item ||
      typeof item !== "object" ||
      captures.length >= 20 ||
      typeof item.id !== "string" ||
      !item.id ||
      item.id.length > 128 ||
      ids.has(item.id) ||
      typeof item.enabled !== "boolean" ||
      typeof item.variable !== "string" ||
      !VARIABLE_NAME.test(item.variable) ||
      !["jsonPath", "header", "status"].includes(item.source) ||
      typeof item.target !== "string" ||
      item.target.length > 256 ||
      (item.source === "header" && !item.target.trim())
    ) {
      dropped++;
      continue;
    }
    ids.add(item.id);
    captures.push({
      id: item.id,
      enabled: item.enabled,
      variable: item.variable,
      source: item.source,
      target: item.target,
    });
  }
  return { captures, dropped };
}
