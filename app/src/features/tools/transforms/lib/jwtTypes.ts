/**
 * The compact JWT boundary is intentionally small and deterministic.  These
 * limits apply before JSON parsing or cryptographic work and are duplicated by
 * the native command so a browser preview cannot widen the packaged-app
 * contract.
 */
export const JWT_LIMITS = Object.freeze({
  maxTokenBytes: 256 * 1024,
  maxSegmentBytes: 96 * 1024,
  maxJsonBytes: 64 * 1024,
  maxJsonDepth: 32,
  maxJsonNodes: 10_000,
  maxJsonStringBytes: 16 * 1024,
  maxOutputBytes: 256 * 1024,
  maxSignatureTextBytes: 128,
  maxKeyTextBytes: 2_100_000,
  maxKeyBytes: 1_000_000,
  clockSkewSeconds: 60,
});

export type JwtAlgorithm = "HS256" | "HS384" | "HS512";
export type JwtKeyEncoding = "utf8" | "hex" | "base64" | "base64url";

export type JwtErrorCode =
  | "invalid_input"
  | "invalid_structure"
  | "invalid_base64url"
  | "invalid_base64"
  | "invalid_utf8"
  | "invalid_json"
  | "duplicate_json_key"
  | "json_bounds"
  | "invalid_header"
  | "algorithm_not_allowed"
  | "invalid_signature"
  | "invalid_key"
  | "key_too_short"
  | "invalid_claims"
  | "verification_unavailable"
  | "verification_failed";

const JWT_ERROR_MESSAGES: Readonly<Record<JwtErrorCode, string>> = Object.freeze({
  invalid_input: "JWT 입력을 처리할 수 없습니다.",
  invalid_structure: "JWT 형식이 올바르지 않습니다.",
  invalid_base64url: "JWT Base64URL을 처리할 수 없습니다.",
  invalid_base64: "JWT 키 인코딩을 처리할 수 없습니다.",
  invalid_utf8: "JWT에 유효하지 않은 UTF-8이 포함되어 있습니다.",
  invalid_json: "JWT JSON을 처리할 수 없습니다.",
  duplicate_json_key: "JWT JSON에 중복된 키가 있습니다.",
  json_bounds: "JWT JSON이 허용된 안전 한도를 초과했습니다.",
  invalid_header: "JWT 헤더가 올바르지 않습니다.",
  algorithm_not_allowed: "허용되지 않은 JWT 알고리즘입니다.",
  invalid_signature: "JWT 서명 형식이 올바르지 않습니다.",
  invalid_key: "JWT 검증 키를 처리할 수 없습니다.",
  key_too_short: "JWT 검증 키가 알고리즘의 최소 길이보다 짧습니다.",
  invalid_claims: "JWT 시간 클레임을 검증할 수 없습니다.",
  verification_unavailable: "JWT 검증 기능을 사용할 수 없습니다.",
  verification_failed: "JWT 검증을 처리할 수 없습니다.",
});

/** Fixed-message error used at every untrusted-input boundary. */
export class JwtError extends Error {
  readonly code: JwtErrorCode;

  constructor(code: JwtErrorCode) {
    super(JWT_ERROR_MESSAGES[code]);
    this.name = "JwtError";
    this.code = code;
  }
}

export interface JwtTransformResult {
  output: string;
  error?: string;
  errorCode?: JwtErrorCode;
}

export interface JwtTemporalClaim {
  name: "exp" | "nbf" | "iat";
  value: number | null;
  iso8601: string | null;
  valid: boolean;
}

export interface ParsedJwt {
  algorithm: JwtAlgorithm;
  header: Record<string, unknown>;
  payload: unknown;
  headerJson: string;
  payloadJson: string;
  signingInput: string;
  signature: string;
  signatureBytes: Uint8Array;
  temporalClaims: JwtTemporalClaim[];
}

export type JwtVerificationStatus = "unverified" | "verified" | "invalid_signature" | "invalid_claims" | "error";

export interface JwtDisplayOptions {
  status?: JwtVerificationStatus;
  verifiedAtSeconds?: number;
}

export type JwtVerifyRequest = import("./browserTypes").JwtVerifyRequest;
