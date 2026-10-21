// Parser-independent bounds for consumers that do not parse a document.
export const OPENAPI_DOCUMENT_LIMITS = Object.freeze({
  maxBytes: 4 * 1024 * 1024,
  maxDepth: 40,
  maxNodes: 50_000,
  maxStringLength: 16_384,
  maxAliases: 50,
});

/** 가져오기 미리보기 한도(v0.9.0 `openapiLimits.ts`). */
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
