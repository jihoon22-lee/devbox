import { type Assertion, cleanAssertions } from "../assertions";
import { type Capture, cleanCaptures } from "../captures";
import {
  MAX_EXPORTED_COLLECTIONS,
  MAX_EXPORTED_ENVIRONMENTS,
  MAX_EXPORTED_VARIABLES,
  MAX_TRANSFER_NAME_CHARS,
} from "../limits";
import { isSensitiveName, looksLikeSecret } from "../secretPatterns";
import { sanitizeRequestForPersistence } from "../storage";
import type { PersistedRequest } from "../template";
import { type ImportBundle, ImportError } from "./model";

/** 만들 요청 파일 하나(경로·직렬화는 `model/requestFile.ts`가 정한다). */
export interface PreviewRequest {
  name: string;
  folder: string;
  request: PersistedRequest;
  assertions: Assertion[];
  captures: Capture[];
  requiresSecretReview: boolean;
}

/** 만들 환경 파일 하나. 비밀 변수는 값을 비우고 표시만 남긴다(사용자가 비밀로 옮긴다). */
export interface PreviewEnvironment {
  name: string;
  variables: { key: string; value: string; secret: boolean }[];
}

export interface ImportPreview {
  requests: PreviewRequest[];
  environments: PreviewEnvironment[];
  warnings: string[];
}

const name = (text: string, fallback: string) =>
  looksLikeSecret(text)
    ? fallback
    : text
        .replace(/[\u0000-\u001f\u007f]/g, " ")
        .trim()
        .slice(0, MAX_TRANSFER_NAME_CHARS) || fallback;

/** 가져온 묶음을 저장 전 미리보기로 바꾼다: 인증 가림·재검토 표시·잘림 경고. */
export function toImportPreview(bundle: ImportBundle): ImportPreview {
  const warnings = [...bundle.warnings];
  if (bundle.requests.length > MAX_EXPORTED_COLLECTIONS)
    warnings.push(`요청이 많아 처음 ${MAX_EXPORTED_COLLECTIONS}개만 가져왔습니다.`);
  if (bundle.environments.length > MAX_EXPORTED_ENVIRONMENTS)
    warnings.push(`환경이 많아 처음 ${MAX_EXPORTED_ENVIRONMENTS}개만 가져왔습니다.`);
  const requests = bundle.requests.slice(0, MAX_EXPORTED_COLLECTIONS).map((item): PreviewRequest => {
    const request = sanitizeRequestForPersistence(item.request);
    const assertions = cleanAssertions(item.assertions).assertions;
    request.requiresSecretReview ||=
      item.requiresSecretReview === true || assertions.some((check) => check.expected.includes("[REDACTED]"));
    return {
      name: name(item.name, "untitled"),
      folder: name(item.folder, ""),
      request,
      assertions,
      captures: cleanCaptures(item.captures).captures,
      requiresSecretReview: request.requiresSecretReview,
    };
  });
  const environments = bundle.environments.slice(0, MAX_EXPORTED_ENVIRONMENTS).map((environment) => {
    if (environment.variables.length > MAX_EXPORTED_VARIABLES)
      warnings.push(`환경 변수가 많아 처음 ${MAX_EXPORTED_VARIABLES}개만 가져왔습니다.`);
    return {
      name: metadata(name(environment.name, "untitled"), "untitled"),
      variables: environment.variables.slice(0, MAX_EXPORTED_VARIABLES).map((variable) => {
        if (!isEnvironmentKey(variable.key) || !safeValue(variable.value))
          throw new ImportError("가져올 수 없는 형식입니다.");
        const secret = variable.secret === true || isSensitiveName(variable.key) || looksLikeSecret(variable.value);
        return { key: variable.key, value: secret ? "" : variable.value, secret };
      }),
    };
  });
  if (!environments.every(isSafePreviewEnvironment)) throw new ImportError("가져올 수 없는 형식입니다.");
  return { requests, environments, warnings };
}

// v0.9.0 `transfer.ts` 환경 내보내기 검증(키 형식·토큰 모양 키·64KiB·제어 문자·U+2028/2029·중복 키)을
// 미리보기에 옮겼다. 통과하지 못하면 가져오기 전체를 거절한다.
const MAX_FIELD_BYTES = 64 * 1024;
const encoder = new TextEncoder();
const unsafeChars = (value: string) => /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(value);
const hasKnownSecret = (value: string) =>
  (value.includes("-----BEGIN") && value.includes("PRIVATE KEY-----")) || looksLikeSecret(value);
const KNOWN_TOKEN =
  /(?:sk[_-]|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/gu;

function isEnvironmentKey(value: string): boolean {
  return value.length > 0 && value.length <= 128 && /^[A-Za-z0-9_.-]+$/u.test(value) && !hasKnownSecret(value);
}

function safeValue(value: string): boolean {
  return encoder.encode(value).byteLength <= MAX_FIELD_BYTES && !unsafeChars(value);
}

/** 표시 이름: 알려진 토큰을 가리고 한 줄·120자로 줄인다. */
function metadata(value: string, fallback: string): string {
  const normalized = value
    .replace(KNOWN_TOKEN, "[REDACTED]")
    .replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]+/gu, " ")
    .trim();
  return Array.from(normalized).slice(0, MAX_TRANSFER_NAME_CHARS).join("") || fallback;
}

/** v0.9.0 `cleanEnvironment`과 같은 규칙: 비밀 변수는 값이 없고, 민감 이름·토큰 모양 값은 비밀이어야 한다. */
export function isSafePreviewEnvironment(environment: PreviewEnvironment): boolean {
  if (
    Array.from(environment.name).length > MAX_TRANSFER_NAME_CHARS ||
    unsafeChars(environment.name) ||
    environment.name.replace(KNOWN_TOKEN, "") !== environment.name ||
    environment.variables.length > MAX_EXPORTED_VARIABLES
  )
    return false;
  const keys = new Set<string>();
  for (const variable of environment.variables) {
    if (!isEnvironmentKey(variable.key) || keys.has(variable.key)) return false;
    keys.add(variable.key);
    if (variable.secret) {
      if (variable.value !== "") return false;
      continue;
    }
    if (!safeValue(variable.value) || isSensitiveName(variable.key) || looksLikeSecret(variable.value)) return false;
  }
  return true;
}
