export type { GraphqlError, GraphqlLocation, GraphqlRequest, GraphqlResponse } from "../template";

export const MAX_GRAPHQL_QUERY_BYTES = 512 * 1024;
export const MAX_GRAPHQL_VARIABLES_BYTES = 512 * 1024;
export const MAX_GRAPHQL_OPERATION_NAME_BYTES = 128;
export const MAX_GRAPHQL_OPERATIONS = 100;
export const MAX_GRAPHQL_TOKENS = 100_000;
export const MAX_GRAPHQL_VARIABLE_DEPTH = 32;
export const MAX_GRAPHQL_VARIABLE_NODES = 10_000;
export const MAX_GRAPHQL_VARIABLE_STRING_BYTES = 64 * 1024;
export const MAX_GRAPHQL_BODY_BYTES = 2 * 1024 * 1024;
export const MAX_GRAPHQL_RESPONSE_NODES = 10_000;
export const MAX_GRAPHQL_RESPONSE_DEPTH = 64;
export const MAX_GRAPHQL_RESPONSE_STRING_BYTES = 64 * 1024;
export const MAX_GRAPHQL_RESPONSE_ERRORS = 100;
export const MAX_GRAPHQL_ERROR_MESSAGE_BYTES = 4 * 1024;
export const MAX_GRAPHQL_ERROR_PATH_ITEMS = 20;
export const MAX_GRAPHQL_ERROR_PATH_ITEM_BYTES = 128;
export const MAX_GRAPHQL_URL_BYTES = 8 * 1024;
export const MIN_GRAPHQL_TIMEOUT_MS = 100;
export const MAX_GRAPHQL_TIMEOUT_MS = 120_000;
export const MAX_GRAPHQL_REQUEST_HEADERS = 100;
export const MAX_GRAPHQL_REQUEST_HEADER_BYTES = 128 * 1024;

export const GRAPHQL_INVALID_REQUEST = "GraphQL 요청 구성이 올바르지 않습니다";
export const GRAPHQL_INVALID_DOCUMENT = "GraphQL 문서 형식이 올바르지 않습니다";
export const GRAPHQL_QUERY_TOO_LARGE = "GraphQL query가 허용된 크기를 초과했습니다";
export const GRAPHQL_VARIABLES_TOO_LARGE = "GraphQL variables가 허용된 크기를 초과했습니다";
export const GRAPHQL_OPERATION_INVALID = "GraphQL operation 선택이 올바르지 않습니다";
export const GRAPHQL_VARIABLES_INVALID = "GraphQL variables는 유효한 JSON object여야 합니다";
export const GRAPHQL_VARIABLES_TOO_COMPLEX = "GraphQL variables 구조가 허용된 한계를 초과했습니다";
export const GRAPHQL_BODY_TOO_LARGE = "GraphQL 요청 본문이 허용된 크기를 초과했습니다";
export const GRAPHQL_UNSUPPORTED_INTROSPECTION = "GraphQL introspection 요청은 지원하지 않습니다";
export const GRAPHQL_UNSUPPORTED_SUBSCRIPTION = "GraphQL subscription은 지원하지 않습니다";
export const GRAPHQL_ENDPOINT_ERROR = "GraphQL endpoint URL이 올바르지 않습니다";
export const GRAPHQL_CREDENTIAL_QUERY_ERROR = "GraphQL endpoint query에 credential을 넣을 수 없습니다";
export const GRAPHQL_HEADER_ROWS_ERROR = "GraphQL header 행 수가 허용된 한계를 초과했습니다";
export const GRAPHQL_HEADER_BYTES_ERROR = "GraphQL header 크기가 허용된 한계를 초과했습니다";
export const GRAPHQL_URL_TOO_LARGE = "GraphQL URL이 허용된 크기를 초과했습니다";

const encoder = new TextEncoder();

export function bytes(value: string): number {
  return encoder.encode(value).byteLength;
}
