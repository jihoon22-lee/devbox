// Parser-independent bounds for consumers that do not parse a document.
export const OPENAPI_DOCUMENT_LIMITS = Object.freeze({
  maxBytes: 4 * 1024 * 1024,
  maxDepth: 40,
  maxNodes: 50_000,
  maxStringLength: 16_384,
  maxAliases: 50,
});
