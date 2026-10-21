export { OPENAPI_LIMITS } from "./limits";

import { OPENAPI_LIMITS } from "./limits";
import { operationPreview } from "./operations";
import { applyPathParameterExamples } from "./parameters";
import {
  detectOpenApiFormat,
  isOpenApiIssue,
  isRecord,
  isSafePath,
  issue,
  joinServerPath,
  METHOD_SET,
  OpenApiFormat,
  OpenApiImportPreview,
  OpenApiImportResult,
  OpenApiIssue,
  OpenApiOperationPreview,
  OpenApiServerPreview,
  OpenApiSource,
  own,
  PATH_ITEM_METADATA,
  parseSource,
  safeFileName,
  serverUrlWithVariables,
  sortedMethods,
  versionOf,
} from "./shared";

export function parsePreview(source: unknown, format: OpenApiFormat): OpenApiImportPreview {
  if (!isRecord(source)) throw issue("ROOT_INVALID", "document");
  const version = versionOf(own(source, "openapi"));
  if (!version)
    throw issue(typeof own(source, "openapi") === "string" ? "VERSION_UNSUPPORTED" : "ROOT_INVALID", "document");
  const info = own(source, "info");
  if (
    !isRecord(info) ||
    typeof own(info, "title") !== "string" ||
    typeof own(info, "version") !== "string" ||
    !isRecord(own(source, "paths"))
  ) {
    throw issue("ROOT_INVALID", "document");
  }

  const rawServers = own(source, "servers");
  const servers: OpenApiServerPreview[] = [];
  const errors: OpenApiIssue[] = [];
  if (rawServers !== undefined && !Array.isArray(rawServers)) errors.push(issue("SERVER_INVALID", "document"));
  if (Array.isArray(rawServers)) {
    if (rawServers.length > OPENAPI_LIMITS.maxServers) throw issue("SERVER_LIMIT", "document");
    rawServers.forEach((raw, index) => {
      try {
        const url = serverUrlWithVariables(raw);
        if (!url) throw new Error("invalid");
        servers.push({ index, url });
      } catch (cause) {
        errors.push(isOpenApiIssue(cause) ? cause : issue("SERVER_INVALID", "document"));
      }
    });
  }

  const components = own(source, "components");
  const securitySchemes =
    isRecord(components) && isRecord(own(components, "securitySchemes"))
      ? (own(components, "securitySchemes") as Record<string, unknown>)
      : (Object.create(null) as Record<string, unknown>);
  if (Object.keys(securitySchemes).length > OPENAPI_LIMITS.maxSecuritySchemes)
    throw issue("SECURITY_SCHEME_LIMIT", "document");
  const paths = own(source, "paths") as Record<string, unknown>;
  const pathKeys = Object.keys(paths)
    .filter((key) => key.startsWith("/") || key === "")
    .sort();
  if (pathKeys.length > OPENAPI_LIMITS.maxPaths) throw issue("PATH_LIMIT", "document");
  const allPathKeys = Object.keys(paths);
  if (allPathKeys.some((key) => !key.startsWith("/") && !key.startsWith("x-")))
    errors.push(issue("PATH_INVALID", "document"));

  let operationCount = 0;
  let parameterCount = 0;
  const operations: OpenApiOperationPreview[] = [];
  const rootSecurity = own(source, "security");
  for (const path of pathKeys) {
    if (!isSafePath(path)) {
      errors.push(issue("PATH_INVALID", "document"));
      continue;
    }
    const pathItem = paths[path];
    if (!isRecord(pathItem)) {
      errors.push(issue("PATH_INVALID", "document", path));
      continue;
    }
    const operationKeys = Object.keys(pathItem).filter((key) => !PATH_ITEM_METADATA.has(key) && !key.startsWith("x-"));
    operationCount += operationKeys.length;
    if (operationCount > OPENAPI_LIMITS.maxOperations) throw issue("OPERATION_LIMIT", "document");
    const methods = sortedMethods(pathItem);
    for (const method of methods) {
      const { declaredParameterCount, ...operation } = operationPreview(
        path,
        method,
        pathItem,
        pathItem[method],
        servers,
        securitySchemes,
        rootSecurity,
        operations.length,
      );
      parameterCount += declaredParameterCount;
      if (parameterCount > OPENAPI_LIMITS.maxParameters) throw issue("PARAMETER_LIMIT", "document");
      operations.push(operation);
    }
    const unsupportedMethods = Object.keys(pathItem).filter(
      (key) => !METHOD_SET.has(key) && !PATH_ITEM_METADATA.has(key) && !key.startsWith("x-"),
    );
    for (const unsupportedMethod of unsupportedMethods) {
      errors.push(issue("METHOD_UNSUPPORTED", "operation", path, unsupportedMethod.toUpperCase()));
    }
  }
  return { format, version, servers, operations, errors, sourceName: undefined };
}

export function parseOpenApi(text: string, format: OpenApiFormat = "yaml"): OpenApiImportResult {
  try {
    const source = parseSource(text, format);
    return { ok: true, preview: parsePreview(source, format) };
  } catch (cause) {
    return { ok: false, error: isOpenApiIssue(cause) ? cause : issue("PARSER_ERROR", "document") };
  }
}

/** Alias kept explicit for callers that prefer the document-oriented name. */
export const parseOpenApiDocument = parseOpenApi;

export function parseOpenApiSource(source: OpenApiSource): OpenApiImportResult {
  const format = source.kind === "url" ? source.format : (source.format ?? detectOpenApiFormat(source.name));
  const result = parseOpenApi(source.text, format);
  return result.ok
    ? {
        ok: true,
        preview: {
          ...result.preview,
          sourceName:
            source.kind === "file"
              ? safeFileName(source.name)
              : `remote-openapi.${format === "json" ? "json" : "yaml"}`,
        },
      }
    : result;
}

export function selectOpenApiServer(preview: OpenApiImportPreview, serverIndex: number): OpenApiImportPreview {
  const server = preview.servers.find((candidate) => candidate.index === serverIndex);
  if (!server) return preview;
  return {
    ...preview,
    operations: preview.operations.map((operation) => {
      const url = joinServerPath(server.url, operation.path);
      if (!url) {
        const errors = [
          ...operation.errors.filter((entry) => entry.code !== "SERVER_INVALID"),
          issue("SERVER_INVALID", "operation", operation.path, operation.method),
        ];
        return {
          ...operation,
          serverIndex,
          request: { ...operation.request, url: operation.path },
          errors,
          applyable: false,
        };
      }
      const errors = operation.errors.filter((entry) => entry.code !== "NO_SERVER" && entry.code !== "SERVER_INVALID");
      return {
        ...operation,
        serverIndex,
        request: { ...operation.request, url: applyPathParameterExamples(url, operation.parameters) },
        errors,
        applyable: errors.length === 0,
      };
    }),
  };
}
