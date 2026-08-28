import { convertByteEncoding } from "../tools/byteCodec";
import { parseJwt } from "../tools/jwt";

/**
 * Smart detection is deliberately a classifier, not an executor.  It only
 * inspects the bounded value in memory and returns stable IDs; it never opens
 * URLs, reads files, verifies credentials, or copies the input elsewhere.
 */
export const SMART_DETECTION_LIMITS = Object.freeze({
  maxInputBytes: 1_000_000,
  maxInputCodeUnits: 2_100_000,
  maxJsonDepth: 64,
  maxJsonNodes: 100_000,
  maxCandidates: 6,
});

export type SmartInputType =
  | "text"
  | "json"
  | "jwt"
  | "url"
  | "base64"
  | "base64url"
  | "hex";

export type SmartDetectionStatus =
  | "empty"
  | "detected"
  | "ambiguous"
  | "unsupported"
  | "too_large";

export interface SmartCandidate {
  readonly kind: Exclude<SmartInputType, "text">;
  readonly inputType: SmartInputType;
  readonly transformerId: string;
  readonly toolId: string;
  readonly label: string;
  readonly reason: string;
  readonly confidence: number;
  readonly sensitive: boolean;
}

export interface SmartDetectionResult {
  readonly status: SmartDetectionStatus;
  readonly inputBytes: number;
  readonly candidates: readonly SmartCandidate[];
  readonly recommendedTransformerId: string | null;
  readonly sensitive: boolean;
}

const FIXED_NO_MATCH_REASON = "지원되는 안전한 구조를 찾지 못했습니다.";
const FIXED_SENSITIVE_REASON = "민감할 수 있는 입력입니다. 원문은 저장하거나 전송하지 않습니다.";

const JSON_REASON = "JSON 구조를 확인했습니다. 로컬 formatter에서만 처리합니다.";
const JWT_REASON = "JWT compact 구조를 확인했습니다. 서명은 검증하지 않습니다.";
const URL_REASON = "HTTP(S) URL 문자열입니다. 열거나 요청하지 않고 component 도구로만 처리합니다.";
const BASE64_REASON = "검증된 Base64 byte 표현입니다. 결과는 메모리에서만 처리합니다.";
const BASE64URL_REASON = "검증된 Base64URL byte 표현입니다. 결과는 메모리에서만 처리합니다.";
const HEX_REASON = "검증된 Hex byte 표현입니다. 결과는 메모리에서만 처리합니다.";
const BINARY_REASON = "byte 표현을 확인했지만 UTF-8 text로 가정하지 않습니다.";

const SECRET_ASSIGNMENT = /(?:^|[\n\r;,\{])\s*["']?(?:authorization|auth|api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|secret|password|passwd|private[_-]?key|token)["']?\s*[:=]\s*["']?\S+/iu;
const AUTHORIZATION_VALUE = /^(?:basic|bearer)\s+\S+/iu;
const COMMON_TOKEN_PREFIX = /^(?:sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9_]{16,}|github_pat_[A-Za-z0-9_]{16,}|xox[baprs]-[A-Za-z0-9-]{16,}|AIza[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16})$/u;
const SECRET_QUERY_KEY = /(?:^|[-_])(token|secret|password|passwd|api[-_]?key|access[-_]?token|refresh[-_]?token|authorization|credential|signature|sig)(?:$|[-_])/iu;
const CONTROL_CHARACTER = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/u;
const WINDOWS_PATH = /^(?:[A-Za-z]:[\\/]|\\\\|file:\/\/)/u;
const POSIX_PATH = /^(?:\/|~\/|\.\.?(?:[\\/]|$))/u;
const URL_PREFIX = /^https?:\/\//iu;
const BASE64_STANDARD = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}(?:==)?|[A-Za-z0-9+/]{3}=?)?$/u;
const BASE64_URL = /^(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2}|[A-Za-z0-9_-]{3})?$/u;
const HEX = /^(?:[0-9A-Fa-f]{2})+$/u;
const CANDIDATE_ORDER: Readonly<Record<SmartCandidate["kind"], number>> = Object.freeze({
  jwt: 0,
  json: 1,
  url: 2,
  base64: 3,
  base64url: 4,
  hex: 5,
});

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function hasWellFormedUnicode(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function emptyResult(status: SmartDetectionStatus, inputBytes = 0): SmartDetectionResult {
  return {
    status,
    inputBytes,
    candidates: [],
    recommendedTransformerId: null,
    sensitive: false,
  };
}

/**
 * A cheap pre-scan keeps JSON.parse away from pathological nesting and gives
 * detection the same practical depth/node ceiling as the other local tools.
 */
function hasBoundedJsonShape(value: string): boolean {
  let depth = 0;
  let nodes = 0;
  let inString = false;
  let escaped = false;
  let inToken = false;

  for (let index = 0; index < value.length; index += 1) {
    const character = value[index]!;
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === '"') {
        inString = false;
      }
      continue;
    }
    if (character === '"') {
      nodes += 1;
      if (nodes > SMART_DETECTION_LIMITS.maxJsonNodes) return false;
      inString = true;
      inToken = false;
      continue;
    }
    if (character === "{" || character === "[") {
      depth += 1;
      nodes += 1;
      if (depth > SMART_DETECTION_LIMITS.maxJsonDepth || nodes > SMART_DETECTION_LIMITS.maxJsonNodes) {
        return false;
      }
      inToken = false;
    } else if (character === "}" || character === "]") {
      depth -= 1;
      if (depth < 0) return false;
      inToken = false;
    } else if (character === "," || character === ":" || /\s/u.test(character)) {
      inToken = false;
    } else if (!inToken) {
      nodes += 1;
      if (nodes > SMART_DETECTION_LIMITS.maxJsonNodes) return false;
      inToken = true;
    }
  }
  return !inString && !escaped && depth === 0;
}

function isJson(value: string): boolean {
  const trimmed = value.trim();
  // Primitive JSON values overlap too easily with Base64/hex.  Smart
  // detection intentionally recommends structured JSON only.
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return false;
  if (!hasBoundedJsonShape(trimmed)) return false;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    return parsed !== null && typeof parsed === "object";
  } catch {
    return false;
  }
}

/** Strict JSON validation for a typed pipeline stage, including primitives. */
export function isBoundedJsonText(value: string): boolean {
  if (typeof value !== "string" || !hasBoundedJsonShape(value.trim())) return false;
  try {
    JSON.parse(value);
    return true;
  } catch {
    return false;
  }
}

function safeHttpUrl(value: string): boolean {
  if (!URL_PREFIX.test(value) || CONTROL_CHARACTER.test(value)) return false;
  try {
    const parsed = new URL(value);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return false;
    // URL parsing is local only.  Userinfo, fragments, and credential-shaped
    // query keys are not presented as a safe recommendation.
    if (parsed.username || parsed.password || parsed.hash) return false;
    for (const [key, queryValue] of parsed.searchParams) {
      if (SECRET_QUERY_KEY.test(key) || hasSecretLikeShape(queryValue)) return false;
    }
    return true;
  } catch {
    return false;
  }
}

function looksLikeUnsafePath(value: string): boolean {
  return WINDOWS_PATH.test(value) || POSIX_PATH.test(value);
}

function hasSecretLikeShape(value: string): boolean {
  const trimmed = value.trim();
  return SECRET_ASSIGNMENT.test(trimmed)
    || AUTHORIZATION_VALUE.test(trimmed)
    || COMMON_TOKEN_PREFIX.test(trimmed);
}

function validEncodedInput(input: string, source: "base64" | "base64url" | "hex"): boolean {
  return convertByteEncoding(input, source, "hex").error === null;
}

function validUtf8Output(input: string, source: "base64" | "base64url" | "hex"): boolean {
  const result = convertByteEncoding(input, source, "utf8");
  return result.error === null && result.output.length > 0;
}

function candidate(
  values: Omit<SmartCandidate, "sensitive" | "reason"> & { reason: string },
  sensitive: boolean,
): SmartCandidate {
  return {
    ...values,
    reason: sensitive ? FIXED_SENSITIVE_REASON : values.reason,
    sensitive,
  };
}

function sortCandidates(candidates: SmartCandidate[]): SmartCandidate[] {
  return candidates.sort((left, right) => {
    const score = right.confidence - left.confidence;
    return score !== 0 ? score : CANDIDATE_ORDER[left.kind] - CANDIDATE_ORDER[right.kind];
  });
}

function detectCandidates(trimmed: string, sensitive: boolean): SmartCandidate[] {
  const candidates: SmartCandidate[] = [];

  if (isJson(trimmed)) {
    candidates.push(candidate({
      kind: "json",
      inputType: "json",
      transformerId: "json-format",
      toolId: "json-format",
      label: "JSON Formatter",
      reason: JSON_REASON,
      confidence: 0.98,
    }, sensitive));
  }

  try {
    parseJwt(trimmed);
    candidates.push(candidate({
      kind: "jwt",
      inputType: "jwt",
      transformerId: "jwt-decode",
      toolId: "jwt",
      label: "JWT Decoder",
      reason: JWT_REASON,
      confidence: 0.99,
    }, true));
  } catch {
    // A compact-looking token with an unsupported algorithm is not a safe
    // recommendation; the normal auth tool will show its fixed error when
    // explicitly opened by the user.
  }

  if (safeHttpUrl(trimmed)) {
    const hasEscape = /%[0-9A-Fa-f]{2}/u.test(trimmed);
    candidates.push(candidate({
      kind: "url",
      inputType: "url",
      transformerId: "url-decode",
      toolId: "url-decode",
      label: "URL Component Decoder",
      reason: URL_REASON,
      confidence: hasEscape ? 0.91 : 0.72,
    }, sensitive));
  }

  const standardBase64 = trimmed.length >= 4
    && BASE64_STANDARD.test(trimmed)
    && validEncodedInput(trimmed, "base64");
  const urlBase64 = trimmed.length >= 4
    && BASE64_URL.test(trimmed)
    && validEncodedInput(trimmed, "base64url");
  const hasUrlAlphabet = /[-_]/u.test(trimmed);
  if (standardBase64 && !hasUrlAlphabet) {
    const textOutput = validUtf8Output(trimmed, "base64");
    candidates.push(candidate({
      kind: "base64",
      inputType: "base64",
      transformerId: textOutput ? "base64-decode" : "base64-to-hex",
      toolId: "byte-codec",
      label: textOutput ? "Base64 Decoder" : "Base64 → Hex",
      reason: textOutput ? BASE64_REASON : `${BASE64_REASON} ${BINARY_REASON}`,
      confidence: /[+/=]/u.test(trimmed) ? 0.84 : 0.61,
    }, sensitive));
  }
  if (urlBase64 && (hasUrlAlphabet || !standardBase64 || !/[+/=]/u.test(trimmed))) {
    const textOutput = validUtf8Output(trimmed, "base64url");
    candidates.push(candidate({
      kind: "base64url",
      inputType: "base64url",
      transformerId: textOutput ? "base64url-decode" : "base64url-to-hex",
      toolId: "byte-codec",
      label: textOutput ? "Base64URL Decoder" : "Base64URL → Hex",
      reason: textOutput ? BASE64URL_REASON : `${BASE64URL_REASON} ${BINARY_REASON}`,
      confidence: hasUrlAlphabet ? 0.86 : 0.60,
    }, sensitive));
  }

  if (trimmed.length >= 4 && HEX.test(trimmed) && validEncodedInput(trimmed, "hex")) {
    const textOutput = validUtf8Output(trimmed, "hex");
    candidates.push(candidate({
      kind: "hex",
      inputType: "hex",
      transformerId: textOutput ? "hex-decode" : "hex-to-base64",
      toolId: "byte-codec",
      label: textOutput ? "Hex Decoder" : "Hex → Base64",
      reason: textOutput ? HEX_REASON : `${HEX_REASON} ${BINARY_REASON}`,
      confidence: 0.86,
    }, sensitive));
  }

  return sortCandidates(candidates).slice(0, SMART_DETECTION_LIMITS.maxCandidates);
}

/** Detect only bounded, local representations and return no user text. */
export function detectSmartInput(input: string): SmartDetectionResult {
  if (typeof input !== "string") return emptyResult("unsupported");
  if (input.length > SMART_DETECTION_LIMITS.maxInputCodeUnits) {
    return emptyResult("too_large", input.length);
  }
  const inputBytes = utf8ByteLength(input);
  if (
    inputBytes > SMART_DETECTION_LIMITS.maxInputBytes
  ) {
    return emptyResult("too_large", inputBytes);
  }
  if (!hasWellFormedUnicode(input)) return emptyResult("unsupported", inputBytes);

  const trimmed = input.trim();
  if (!trimmed) return emptyResult("empty", inputBytes);
  const sensitive = hasSecretLikeShape(trimmed);
  if (looksLikeUnsafePath(trimmed)) return { ...emptyResult("unsupported", inputBytes), sensitive };

  const candidates = detectCandidates(trimmed, sensitive);
  const sensitiveResult = sensitive || candidates.some((item) => item.sensitive);
  if (candidates.length === 0) {
    return { ...emptyResult("unsupported", inputBytes), sensitive: sensitiveResult };
  }
  const [first, second] = candidates;
  const ambiguous = second !== undefined
    && first !== undefined
    && first.confidence - second.confidence < 0.12;
  return {
    status: ambiguous ? "ambiguous" : "detected",
    inputBytes,
    candidates,
    recommendedTransformerId: ambiguous ? null : first?.transformerId ?? null,
    sensitive: sensitiveResult,
  };
}

export { FIXED_NO_MATCH_REASON, FIXED_SENSITIVE_REASON };
