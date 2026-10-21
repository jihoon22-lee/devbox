/**
 * v0.9.0 API Studio 생성 타입 중 이식한 lib이 쓰는 모양만 다시 선언한다("타입 재결합").
 * 데몬 RPC 타입(`app/src/rpc/gen`)과는 별개이며 초안·파일 형식으로 옮기는 것은 `model/`이 맡는다.
 */
export type KeyValue = { key: string; value: string };

/** `enabled`가 없으면 활성 행으로 본다. */
export type RequestHeader = { key: string; value: string; enabled?: boolean };

/** 요청 Cookie 헤더로 합쳐지는 name/value 행(쿠키 저장소가 아니다). */
export type RequestCookie = { name: string; value: string; enabled?: boolean };

export type MultipartPart = {
  kind: string;
  name: string;
  value: string;
  file_path: string;
  file_name: string;
  content_type: string;
  enabled?: boolean;
};

export type GrantType = "authorizationCode" | "clientCredentials";

export type OAuth2Config = {
  grantType: GrantType;
  authorizationUrl: string;
  tokenUrl: string;
  clientId: string;
  clientSecret: string;
  scopes: string;
};

export type AuthConfig = {
  kind: string;
  username: string;
  password: string;
  token: string;
  api_key: string;
  api_value: string;
  oauth2?: OAuth2Config;
};

export type GraphqlRequest = { query: string; variables: string; operation_name: string };
export type GraphqlLocation = { line: number; column: number };
export type GraphqlError = { message: string; locations: Array<GraphqlLocation>; path: Array<string> };

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]?: JsonValue } | null;

export type GraphqlResponse = {
  /** valid | not_json | invalid | oversized */
  envelope: string;
  data: JsonValue | null;
  errors: Array<GraphqlError>;
  errors_truncated: boolean;
};

export type RequestTls = { credentialId: string | null; verify: boolean };

/** 사용자가 편집하는 요청 원본. 변수 참조는 해석하지 않은 채 둔다. */
export type RequestTemplate = {
  method: string;
  url: string;
  headers: Array<RequestHeader>;
  cookies: Array<RequestCookie>;
  multipart: Array<MultipartPart>;
  params: Array<KeyValue>;
  /** none | json | form | multipart | raw | graphql */
  body_kind: string;
  body: string;
  auth?: AuthConfig | null;
  timeout_ms: number;
  graphql?: GraphqlRequest | null;
  tls?: RequestTls | null;
};

/** 저장 직전에 직접 입력한 민감 값을 지운 요청. */
export interface PersistedRequest extends RequestTemplate {
  requiresSecretReview: boolean;
}

export type BinaryResponse = {
  media_type: string;
  size_bytes: number;
  hex_preview: string;
  text_preview: string | null;
  hex_truncated: boolean;
  text_truncated: boolean;
  save_available: boolean;
};

export type ResponseCookie = { name: string; value: string; attributes: Array<KeyValue> };
export type RedirectHop = { status: number; location: string };

/** assertion·캡처가 읽는 응답 모양. */
export type ApiResponse = {
  status: number;
  status_text: string;
  headers: Array<KeyValue>;
  duration_ms: number;
  size_bytes: number;
  body: string;
  is_json: boolean;
  final_url: string;
  redirects: Array<RedirectHop>;
  cookies: Array<ResponseCookie>;
  response_id: string | null;
  raw_headers_available: boolean;
  headers_truncated: boolean;
  binary?: BinaryResponse;
  graphql?: GraphqlResponse;
};

/** 코드 생성이 쓰는 환경 변수 행. `secret`이면 값을 코드에 넣지 않는다. */
export interface EnvVariable {
  key: string;
  value: string;
  secret: boolean;
}

/** typed 경계를 넘기 전에 JSON 표현으로 정규화한다. */
export function toJsonValue(value: unknown): JsonValue {
  const encoded = JSON.stringify(value);
  if (encoded === undefined) throw new Error("JSON value is required");
  return JSON.parse(encoded) as JsonValue;
}
