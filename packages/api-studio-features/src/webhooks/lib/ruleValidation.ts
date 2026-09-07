import type { ResponseRule, ResponseSequenceStep } from "../api";

/**
 * These limits mirror `src-tauri/src/core/rules.rs`. The Rust command remains
 * authoritative; keeping the same checks here preserves the raw draft and
 * avoids sending an input that storage will reject.
 */
export const MAX_RULES = 200;
export const MAX_RULE_ID_CHARS = 128;
export const MAX_RULE_ID_BYTES = 128;
export const MAX_METHOD_CHARS = 16;
export const MAX_METHOD_BYTES = 16;
export const MAX_PATH_CHARS = 4_096;
export const MAX_PATH_BYTES = 16_384;
export const MAX_RULE_HEADERS = 100;
export const MAX_HEADER_NAME_CHARS = 256;
export const MAX_HEADER_NAME_BYTES = 256;
export const MAX_HEADER_VALUE_CHARS = 16_384;
export const MAX_HEADER_VALUE_BYTES = 65_536;
export const MAX_HEADER_TOTAL_CHARS = 64_000;
export const MAX_HEADER_TOTAL_BYTES = 256_000;
export const MAX_BODY_CHARS = 256_000;
export const MAX_BODY_BYTES = 1_024_000;
export const MAX_RULE_COLLECTION_CHARS = 2_000_000;
export const MAX_RULE_COLLECTION_BYTES = 8_000_000;
export const MIN_RESPONSE_STATUS = 100;
export const MAX_RESPONSE_STATUS = 599;
export const MAX_RESPONSE_DELAY_MS = 60_000;
export const MIN_RULE_PRIORITY = -1_000;
export const MAX_RULE_PRIORITY = 1_000;
export const MAX_RESPONSE_SEQUENCE = 16;

export type RuleValidationField =
  | "id"
  | "method"
  | "path"
  | "priority"
  | "status"
  | "headers"
  | "body"
  | "delayMs"
  | "sequence"
  | "collection";

export interface RuleValidationIssue {
  field: RuleValidationField;
  message: string;
}

export type RuleValidationInput =
  & Pick<ResponseRule, "path" | "status" | "delayMs">
  & Partial<Pick<ResponseRule, "id" | "method" | "priority" | "headers" | "body" | "sequence">>;

interface StringMetrics {
  chars: number;
  bytes: number;
}

const GENERATED_RULE_ID = "00000000-0000-0000-0000-000000000000";
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f-\u009f]/;
const UTF8_ENCODER = new TextEncoder();
const TRANSPORT_HEADERS = new Set([
  "connection",
  "content-length",
  "expect",
  "host",
  "proxy-connection",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
]);

function hasUnpairedSurrogate(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (Number.isNaN(next) || next < 0xdc00 || next > 0xdfff) return true;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return true;
    }
  }
  return false;
}

function stringMetrics(value: unknown): StringMetrics | null {
  if (typeof value !== "string") return null;
  if (hasUnpairedSurrogate(value)) return null;
  return {
    chars: Array.from(value).length,
    bytes: UTF8_ENCODER.encode(value).byteLength,
  };
}

function within(value: unknown, maxChars: number, maxBytes: number): boolean {
  const metrics = stringMetrics(value);
  return metrics !== null && metrics.chars <= maxChars && metrics.bytes <= maxBytes;
}

function isMethod(value: string): boolean {
  return /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(value);
}

function isHeaderName(value: string): boolean {
  return /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(value);
}

function isAscii(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    if (value.charCodeAt(index) > 0x7f) return false;
  }
  return true;
}

function isTransportHeader(value: string): boolean {
  return TRANSPORT_HEADERS.has(value.toLowerCase());
}

function addIssue(
  issues: RuleValidationIssue[],
  field: RuleValidationField,
  message: string,
): void {
  issues.push({ field, message });
}

function normalizedStrings(rule: RuleValidationInput): {
  id: unknown;
  method: unknown;
  path: unknown;
  headers: unknown;
  body: unknown;
  sequence: unknown;
} {
  return {
    id: rule.id ?? "",
    method: rule.method ?? null,
    path: rule.path,
    headers: rule.headers ?? [],
    body: rule.body ?? "",
    sequence: rule.sequence === undefined ? [] : rule.sequence,
  };
}

function validHeaderList(headers: unknown): headers is Array<[string, string]> {
  if (!Array.isArray(headers) || headers.length > MAX_RULE_HEADERS) return false;
  let headerChars = 0;
  let headerBytes = 0;
  for (const header of headers) {
    if (
      !Array.isArray(header)
      || header.length !== 2
      || typeof header[0] !== "string"
      || !within(header[0], MAX_HEADER_NAME_CHARS, MAX_HEADER_NAME_BYTES)
      || !isHeaderName(header[0])
      || isTransportHeader(header[0])
      || typeof header[1] !== "string"
      || !within(header[1], MAX_HEADER_VALUE_CHARS, MAX_HEADER_VALUE_BYTES)
      || !isAscii(header[1])
      || CONTROL_CHARACTERS.test(header[1])
    ) {
      return false;
    }
    const nameMetrics = stringMetrics(header[0]);
    const valueMetrics = stringMetrics(header[1]);
    if (!nameMetrics || !valueMetrics) return false;
    headerChars += nameMetrics.chars + valueMetrics.chars;
    headerBytes += nameMetrics.bytes + valueMetrics.bytes;
  }
  return headerChars <= MAX_HEADER_TOTAL_CHARS && headerBytes <= MAX_HEADER_TOTAL_BYTES;
}

function validResponseStep(value: unknown): value is ResponseSequenceStep {
  if (!value || typeof value !== "object") return false;
  const step = value as Partial<ResponseSequenceStep>;
  return (
    typeof step.status === "number"
    && Number.isInteger(step.status)
    && step.status >= MIN_RESPONSE_STATUS
    && step.status <= MAX_RESPONSE_STATUS
    && validHeaderList(step.headers)
    && typeof step.body === "string"
    && within(step.body, MAX_BODY_CHARS, MAX_BODY_BYTES)
    && typeof step.delayMs === "number"
    && Number.isInteger(step.delayMs)
    && step.delayMs >= 0
    && step.delayMs <= MAX_RESPONSE_DELAY_MS
  );
}

function ruleMetrics(rule: RuleValidationInput): StringMetrics | null {
  const normalized = normalizedStrings(rule);
  const metrics: StringMetrics = { chars: 0, bytes: 0 };
  const add = (value: unknown): boolean => {
    const current = stringMetrics(value);
    if (!current) return false;
    metrics.chars += current.chars;
    metrics.bytes += current.bytes;
    return true;
  };

  if (!add(normalized.id === "" ? GENERATED_RULE_ID : normalized.id)) return null;
  if (normalized.method !== null && !add(normalized.method)) return null;
  if (!add(normalized.path) || !Array.isArray(normalized.headers)) return null;
  for (const header of normalized.headers) {
    if (!Array.isArray(header) || header.length !== 2 || !add(header[0]) || !add(header[1])) {
      return null;
    }
  }
  if (!Array.isArray(normalized.sequence)) return null;
  for (const step of normalized.sequence) {
    if (!validResponseStep(step)) return null;
    for (const header of step.headers) {
      if (!add(header[0]) || !add(header[1])) return null;
    }
    if (!add(step.body)) return null;
  }
  return add(normalized.body) ? metrics : null;
}

/**
 * Validate one draft using the same scalar, header, and UTF-8 bounds as the
 * Rust storage boundary. Empty id/method are valid only as new/all-method
 * editor values; the backend assigns an id before inserting a new rule.
 */
export function validateRule(
  rule: RuleValidationInput,
): RuleValidationIssue[] {
  const issues: RuleValidationIssue[] = [];
  const normalized = normalizedStrings(rule);

  if (
    typeof normalized.id !== "string"
    || (!within(normalized.id, MAX_RULE_ID_CHARS, MAX_RULE_ID_BYTES)
      && normalized.id !== "")
    || (normalized.id !== "" && CONTROL_CHARACTERS.test(normalized.id))
  ) {
    addIssue(issues, "id", "rule id의 길이 또는 문자가 유효하지 않습니다.");
  }

  if (
    normalized.method !== null
    && (typeof normalized.method !== "string"
      || !within(normalized.method, MAX_METHOD_CHARS, MAX_METHOD_BYTES)
      || !isMethod(normalized.method))
  ) {
    addIssue(issues, "method", "method는 ASCII HTTP token이어야 합니다 (최대 16자).");
  }

  if (
    typeof normalized.path !== "string"
    || !within(normalized.path, MAX_PATH_CHARS, MAX_PATH_BYTES)
    || !normalized.path.startsWith("/")
    || !isAscii(normalized.path)
    || CONTROL_CHARACTERS.test(normalized.path)
  ) {
    addIssue(issues, "path", "path는 ASCII / 경로여야 하며 제어 문자를 포함할 수 없습니다.");
  }

  if (
    typeof rule.status !== "number"
    || !Number.isInteger(rule.status)
    || rule.status < MIN_RESPONSE_STATUS
    || rule.status > MAX_RESPONSE_STATUS
  ) {
    addIssue(issues, "status", `status는 ${MIN_RESPONSE_STATUS}~${MAX_RESPONSE_STATUS} 범위의 정수여야 합니다.`);
  }

  const priority = rule.priority ?? 0;
  if (
    typeof priority !== "number"
    || !Number.isInteger(priority)
    || priority < MIN_RULE_PRIORITY
    || priority > MAX_RULE_PRIORITY
  ) {
    addIssue(issues, "priority", `priority는 ${MIN_RULE_PRIORITY}~${MAX_RULE_PRIORITY} 범위의 정수여야 합니다.`);
  }

  if (
    typeof normalized.headers !== "object"
    || !Array.isArray(normalized.headers)
    || normalized.headers.length > MAX_RULE_HEADERS
  ) {
    addIssue(issues, "headers", "response headers의 개수 또는 형식이 유효하지 않습니다.");
  } else {
    let headerChars = 0;
    let headerBytes = 0;
    for (const header of normalized.headers) {
      if (
        !Array.isArray(header)
        || header.length !== 2
        || typeof header[0] !== "string"
        || !within(header[0], MAX_HEADER_NAME_CHARS, MAX_HEADER_NAME_BYTES)
        || !isHeaderName(header[0])
        || isTransportHeader(header[0])
        || typeof header[1] !== "string"
        || !within(header[1], MAX_HEADER_VALUE_CHARS, MAX_HEADER_VALUE_BYTES)
        || !isAscii(header[1])
        || CONTROL_CHARACTERS.test(header[1])
      ) {
        addIssue(issues, "headers", "response header의 이름·값 또는 제어 문자가 유효하지 않습니다.");
        continue;
      }
      const nameMetrics = stringMetrics(header[0]);
      const valueMetrics = stringMetrics(header[1]);
      if (!nameMetrics || !valueMetrics) continue;
      headerChars += nameMetrics.chars + valueMetrics.chars;
      headerBytes += nameMetrics.bytes + valueMetrics.bytes;
    }
    if (headerChars > MAX_HEADER_TOTAL_CHARS || headerBytes > MAX_HEADER_TOTAL_BYTES) {
      addIssue(issues, "headers", "response headers의 전체 크기가 허용 범위를 초과했습니다.");
    }
  }

  if (
    typeof normalized.body !== "string"
    || !within(normalized.body, MAX_BODY_CHARS, MAX_BODY_BYTES)
  ) {
    addIssue(issues, "body", `response body는 ${MAX_BODY_CHARS}자/${MAX_BODY_BYTES}바이트 이하여야 합니다.`);
  }

  if (
    typeof rule.delayMs !== "number"
    || !Number.isInteger(rule.delayMs)
    || rule.delayMs < 0
    || rule.delayMs > MAX_RESPONSE_DELAY_MS
  ) {
    addIssue(issues, "delayMs", `delay는 0~${MAX_RESPONSE_DELAY_MS}ms 범위의 정수여야 합니다.`);
  }

  if (!Array.isArray(normalized.sequence) || normalized.sequence.length > MAX_RESPONSE_SEQUENCE) {
    addIssue(issues, "sequence", `response sequence는 최대 ${MAX_RESPONSE_SEQUENCE}단계까지 추가할 수 있습니다.`);
  } else {
    for (const step of normalized.sequence) {
      if (!validResponseStep(step)) {
        addIssue(issues, "sequence", "response sequence의 status·headers·body·delay 형식이 유효하지 않습니다.");
        break;
      }
    }
  }

  const metrics = ruleMetrics(rule);
  if (
    metrics
    && (metrics.chars > MAX_RULE_COLLECTION_CHARS || metrics.bytes > MAX_RULE_COLLECTION_BYTES)
  ) {
    addIssue(issues, "collection", "rule 문자열 전체 크기가 허용 범위를 초과했습니다.");
  }

  return issues;
}

/** Validate rule count and aggregate UTF-8/string budgets before persistence. */
export function validateRuleCollection(rules: readonly ResponseRule[]): RuleValidationIssue[] {
  const issues: RuleValidationIssue[] = [];
  if (rules.length > MAX_RULES) {
    addIssue(issues, "collection", `rule은 최대 ${MAX_RULES}개까지 저장할 수 있습니다.`);
  }

  let chars = 0;
  let bytes = 0;
  for (let index = 0; index < rules.length && index < MAX_RULES; index += 1) {
    const rule = rules[index];
    issues.push(...validateRule(rule));
    const metrics = ruleMetrics(rule);
    if (!metrics) continue;
    chars += metrics.chars;
    bytes += metrics.bytes;
    if (chars > MAX_RULE_COLLECTION_CHARS || bytes > MAX_RULE_COLLECTION_BYTES) break;
  }
  if (chars > MAX_RULE_COLLECTION_CHARS || bytes > MAX_RULE_COLLECTION_BYTES) {
    addIssue(issues, "collection", "rule 문자열 전체 크기가 허용 범위를 초과했습니다.");
  }
  return issues;
}
