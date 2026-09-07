import { OPENAPI_DOCUMENT_LIMITS } from "@devbox/openapi/limits";

export const OPENAPI_LIMITS = Object.freeze({
  ...OPENAPI_DOCUMENT_LIMITS,
  maxPaths: 250,
  maxOperations: 1_000,
  maxServers: 20,
  maxParameters: 2_000,
  maxSecuritySchemes: 100,
  maxMediaTypes: 50,
  maxBodyBytes: 512 * 1024,
  maxRequestRows: 100,
  maxParameterNameLength: 256,
  maxFileNameLength: 120,
  maxCollectionNameLength: 120,
});
