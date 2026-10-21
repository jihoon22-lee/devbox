import { MAX_REQUEST_COOKIE_ROWS } from "../cookies";
import { MAX_REQUEST_HEADER_ROWS } from "../headers";
import { bodyForMedia, chooseMedia } from "./bodies";
import { OPENAPI_LIMITS } from "./limits";
import { applyPathParameterExamples, parametersFor, pathTemplateParameters } from "./parameters";
import { InternalOperationPreview, securityFor } from "./security";
import {
  compareText,
  emptyRequest,
  hasRef,
  isRecord,
  issue,
  joinServerPath,
  OpenApiIssue,
  OpenApiRequestBodyPreview,
  OpenApiServerPreview,
  own,
} from "./shared";

export function operationPreview(
  path: string,
  method: string,
  pathItem: Record<string, unknown>,
  operation: unknown,
  servers: OpenApiServerPreview[],
  securitySchemes: Record<string, unknown>,
  rootSecurity: unknown,
  operationIndex: number,
): InternalOperationPreview {
  const errors: OpenApiIssue[] = [];
  const warnings: OpenApiIssue[] = [];
  const server = servers[0];
  const url = server ? (joinServerPath(server.url, path) ?? path) : path;
  const request = emptyRequest(method.toUpperCase(), url);
  // Inspect only the path item's own `$ref`. Recursing through the full path
  // item would see references inside sibling methods and break operation-level
  // error isolation.
  const pathItemHasRef = Object.prototype.hasOwnProperty.call(pathItem, "$ref");
  if (!isRecord(operation) || hasRef(operation) || pathItemHasRef) {
    errors.push(
      issue(hasRef(operation) || pathItemHasRef ? "UNSUPPORTED_REF" : "OPERATION_INVALID", "operation", path, method),
    );
  }
  const operationRecord = isRecord(operation) ? operation : (Object.create(null) as Record<string, unknown>);
  if (!server) errors.push(issue("NO_SERVER", "operation", path, method));
  if (server && !joinServerPath(server.url, path)) errors.push(issue("SERVER_INVALID", "operation", path, method));
  // Existing RequestTemplate has one document-wide server selector. Applying
  // a path/operation override without representing its precedence would make
  // the preview point at a different endpoint than the source describes.
  if (
    Object.prototype.hasOwnProperty.call(pathItem, "servers") ||
    Object.prototype.hasOwnProperty.call(operationRecord, "servers")
  ) {
    errors.push(issue("SERVER_OVERRIDE_UNSUPPORTED", "operation", path, method));
  }

  const parameterResult = parametersFor(pathItem, operationRecord, path, method);
  warnings.push(...parameterResult.warnings);
  errors.push(...parameterResult.errors);
  request.headers.push(...parameterResult.headers);
  request.cookies.push(...parameterResult.cookies);
  request.params.push(...parameterResult.params);
  const declaredPathParameters = new Set(
    parameterResult.previews.filter((parameter) => parameter.location === "path").map((parameter) => parameter.name),
  );
  const templateParameters = pathTemplateParameters(path);
  if (new Set(templateParameters).size !== templateParameters.length) {
    errors.push(issue("DUPLICATE_PARAMETER", "operation", path, method));
  }
  for (const name of templateParameters) {
    if (!declaredPathParameters.has(name)) errors.push(issue("PARAMETER_INVALID", "operation", path, method));
  }
  request.url = applyPathParameterExamples(request.url, parameterResult.previews);

  const rawBody = own(operationRecord, "requestBody");
  let requestBody: OpenApiRequestBodyPreview | null = null;
  if (rawBody !== undefined) {
    if (!isRecord(rawBody) || hasRef(rawBody)) {
      errors.push(issue(hasRef(rawBody) ? "UNSUPPORTED_REF" : "REQUEST_BODY_INVALID", "operation", path, method));
    } else {
      const content = own(rawBody, "content");
      if (!isRecord(content)) {
        errors.push(issue("REQUEST_BODY_INVALID", "operation", path, method));
      } else {
        const mediaKeys = Object.keys(content);
        if (mediaKeys.length > OPENAPI_LIMITS.maxMediaTypes)
          errors.push(issue("MEDIA_TYPE_LIMIT", "operation", path, method));
        const media = chooseMedia(content);
        if (media) {
          const mediaContent = content[media];
          if (!isRecord(mediaContent) || hasRef(mediaContent)) {
            errors.push(
              issue(hasRef(mediaContent) ? "UNSUPPORTED_REF" : "REQUEST_BODY_INVALID", "operation", path, method),
            );
          } else {
            const body = bodyForMedia(media, mediaContent);
            request.body_kind = body.kind;
            request.body = body.body;
            request.multipart = body.multipart;
            requestBody = { mediaType: media, exampleIncluded: body.exampleIncluded, redacted: body.redacted };
            if (body.warning) warnings.push(issue(body.warning, "operation", path, method));
            if (
              body.kind !== "none" &&
              body.kind !== "multipart" &&
              !request.headers.some((header) => header.key.toLowerCase() === "content-type")
            ) {
              request.headers.push({ key: "Content-Type", value: media, enabled: true });
            }
          }
        } else {
          errors.push(issue("REQUEST_BODY_INVALID", "operation", path, method));
        }
      }
    }
  }

  const operationSecurity = Object.prototype.hasOwnProperty.call(operationRecord, "security")
    ? own(operationRecord, "security")
    : rootSecurity;
  const security = securityFor(operationSecurity, securitySchemes, path, method);
  request.auth = security.auth;
  request.params.push(...security.params);
  request.headers.push(...security.headers);
  request.cookies.push(...security.cookies);
  warnings.push(...security.warnings);
  errors.push(...security.errors);

  // Body and security metadata can add derived rows after the parameter
  // pass. Apply the cap again at the final request boundary so an otherwise
  // valid document cannot produce an oversized editor or request payload.
  if (
    request.headers.length > MAX_REQUEST_HEADER_ROWS ||
    request.cookies.length > MAX_REQUEST_COOKIE_ROWS ||
    request.params.length > OPENAPI_LIMITS.maxRequestRows
  ) {
    errors.push(issue("REQUEST_ROW_LIMIT", "operation", path, method));
    request.headers = request.headers.slice(0, MAX_REQUEST_HEADER_ROWS);
    request.cookies = request.cookies.slice(0, MAX_REQUEST_COOKIE_ROWS);
    request.params = request.params.slice(0, OPENAPI_LIMITS.maxRequestRows);
  }

  request.headers.sort((left, right) => compareText(left.key, right.key));
  request.params.sort((left, right) => compareText(left.key, right.key));
  request.cookies.sort((left, right) => compareText(left.name, right.name));
  const uniqueIssues = (entries: OpenApiIssue[]) =>
    entries.filter((entry, index) => entries.findIndex((candidate) => candidate.code === entry.code) === index);
  const uniqueErrors = uniqueIssues(errors);
  const uniqueWarnings = uniqueIssues(
    warnings.filter((entry) => !uniqueErrors.some((error) => error.code === entry.code)),
  );
  const id = `openapi-${operationIndex + 1}`;
  return {
    id,
    path,
    method: method.toUpperCase(),
    label: `${method.toUpperCase()} ${path}`,
    serverIndex: server?.index ?? null,
    request,
    parameters: parameterResult.previews,
    requestBody,
    security: security.metadata,
    warnings: uniqueWarnings,
    errors: uniqueErrors,
    applyable: uniqueErrors.length === 0,
    declaredParameterCount: parameterResult.declaredCount,
  };
}
