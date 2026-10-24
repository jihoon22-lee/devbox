import { MAX_MULTIPART_PARTS } from "../multipart";
import type { MultipartPart, RequestHeader } from "../template";
import { OPENAPI_LIMITS } from "./limits";
import {
  byteLength,
  exampleFrom,
  HTTP_TOKEN,
  isRecord,
  isSensitiveName,
  isSensitiveValue,
  mediaType,
  OpenApiIssueCode,
  own,
  safeScalar,
} from "./shared";

export function sanitizeBodyValue(
  value: unknown,
  propertyName?: string,
): { value: unknown; redacted: boolean; safe: boolean } {
  if (propertyName && isSensitiveName(propertyName)) return { value: "", redacted: true, safe: true };
  if (typeof value === "string") {
    if (isSensitiveValue(value)) return { value: "", redacted: true, safe: true };
    if (/[\u0000-\u001f\u007f]/.test(value)) return { value: "", redacted: false, safe: false };
    return value.length <= OPENAPI_LIMITS.maxStringLength
      ? { value, redacted: false, safe: true }
      : { value: "", redacted: false, safe: false };
  }
  if (typeof value === "number" || typeof value === "boolean" || value === null) {
    return typeof value === "number" && !Number.isFinite(value)
      ? { value: "", redacted: false, safe: false }
      : { value, redacted: false, safe: true };
  }
  if (Array.isArray(value)) {
    const result: unknown[] = [];
    let redacted = false;
    for (const entry of value) {
      const safe = sanitizeBodyValue(entry);
      if (!safe.safe) return { value: [], redacted: false, safe: false };
      result.push(safe.value);
      redacted ||= safe.redacted;
    }
    return { value: result, redacted, safe: true };
  }
  if (!isRecord(value)) return { value: "", redacted: false, safe: false };
  const result = Object.create(null) as Record<string, unknown>;
  let redacted = false;
  for (const key of Object.keys(value).sort()) {
    if (key.length > OPENAPI_LIMITS.maxStringLength || /[\u0000-\u001f\u007f]/.test(key))
      return { value: Object.create(null), redacted: false, safe: false };
    const safe = sanitizeBodyValue(value[key], key);
    if (!safe.safe) return { value: Object.create(null), redacted: false, safe: false };
    result[key] = safe.value;
    redacted ||= safe.redacted;
  }
  return { value: result, redacted, safe: true };
}

export function schemaExample(schema: Record<string, unknown>, depth = 0): unknown {
  if (depth > 12) return undefined;
  if (Object.prototype.hasOwnProperty.call(schema, "example")) return own(schema, "example");
  if (Object.prototype.hasOwnProperty.call(schema, "default")) return own(schema, "default");
  const enumValues = own(schema, "enum");
  if (Array.isArray(enumValues) && enumValues.length > 0) return enumValues[0];
  const type = own(schema, "type");
  if (type === "object" || isRecord(own(schema, "properties"))) {
    const properties = own(schema, "properties");
    if (!isRecord(properties)) return {};
    const result = Object.create(null) as Record<string, unknown>;
    for (const key of Object.keys(properties).sort()) {
      const property = properties[key];
      if (!isRecord(property)) continue;
      const example = schemaExample(property, depth + 1);
      if (example !== undefined) result[key] = example;
    }
    return result;
  }
  if (type === "array") {
    const items = own(schema, "items");
    return isRecord(items) ? [schemaExample(items, depth + 1)].filter((entry) => entry !== undefined) : [];
  }
  if (type === "boolean") return false;
  if (type === "integer" || type === "number") return 0;
  return type === "string" ? "" : undefined;
}

export function bodySample(content: Record<string, unknown>): { value: unknown; found: boolean } {
  const direct = exampleFrom(content);
  if (direct.found) return { value: direct.value, found: true };
  const schema = own(content, "schema");
  if (isRecord(schema)) {
    const sample = schemaExample(schema);
    return { value: sample, found: sample !== undefined };
  }
  return { value: "", found: false };
}

export function sortedObjectEntries(value: Record<string, unknown>): [string, unknown][] {
  return Object.keys(value)
    .sort()
    .map((key) => [key, value[key]]);
}

export function bodyForMedia(
  media: string,
  content: Record<string, unknown>,
): {
  kind: string;
  body: string;
  multipart: MultipartPart[];
  form_fields?: RequestHeader[];
  exampleIncluded: boolean;
  redacted: boolean;
  warning: OpenApiIssueCode | null;
} {
  const normalized = media.split(";", 1)[0]!.trim().toLowerCase();
  const sample = bodySample(content);
  if (!sample.found) {
    return {
      kind:
        normalized === "multipart/form-data"
          ? "multipart"
          : normalized === "application/x-www-form-urlencoded"
            ? "form"
            : normalized.includes("json")
              ? "json"
              : "raw",
      body: "",
      multipart: [],
      exampleIncluded: false,
      redacted: false,
      warning: null,
    };
  }
  // A scalar document-level example has no property name to classify. Treat a
  // non-empty string as opaque so a bearer/password literal cannot be copied
  // into a draft merely because it was documented as an example.
  if ((typeof sample.value === "string" && sample.value.length > 0) || Array.isArray(sample.value)) {
    return {
      kind: "none",
      body: "",
      multipart: [],
      exampleIncluded: false,
      redacted: false,
      warning: "BODY_EXAMPLE_OMITTED",
    };
  }
  const sanitized = sanitizeBodyValue(sample.value);
  if (!sanitized.safe) {
    return {
      kind: "none",
      body: "",
      multipart: [],
      exampleIncluded: false,
      redacted: false,
      warning: "BODY_EXAMPLE_OMITTED",
    };
  }
  if (normalized === "multipart/form-data") {
    const multipart: MultipartPart[] = [];
    let multipartBytes = 0;
    if (isRecord(sanitized.value)) {
      for (const [name, value] of sortedObjectEntries(sanitized.value)) {
        const scalar = safeScalar(value);
        if (scalar === null || name.length > 120 || !HTTP_TOKEN.test(name)) {
          return {
            kind: "multipart",
            body: "",
            multipart: [],
            exampleIncluded: false,
            redacted: sanitized.redacted,
            warning: "BODY_EXAMPLE_OMITTED",
          };
        }
        multipartBytes += byteLength(name) + byteLength(scalar);
        if (multipart.length >= MAX_MULTIPART_PARTS || multipartBytes > OPENAPI_LIMITS.maxBodyBytes) {
          return {
            kind: "multipart",
            body: "",
            multipart: [],
            exampleIncluded: false,
            redacted: sanitized.redacted,
            warning: "BODY_TOO_LARGE",
          };
        }
        multipart.push({
          kind: "text",
          name,
          value: scalar,
          file_path: "",
          file_name: "",
          content_type: "",
          enabled: true,
        });
      }
    }
    return {
      kind: "multipart",
      body: "",
      multipart,
      exampleIncluded: true,
      redacted: sanitized.redacted,
      warning: null,
    };
  }
  if (normalized === "application/x-www-form-urlencoded") {
    if (!isRecord(sanitized.value))
      return {
        kind: "form",
        body: "",
        multipart: [],
        exampleIncluded: false,
        redacted: sanitized.redacted,
        warning: "BODY_EXAMPLE_OMITTED",
      };
    const form = new URLSearchParams();
    for (const [name, value] of sortedObjectEntries(sanitized.value)) {
      const scalar = safeScalar(value);
      if (scalar !== null) form.set(name, scalar);
    }
    const body = form.toString();
    return byteLength(body) > OPENAPI_LIMITS.maxBodyBytes
      ? {
          kind: "form",
          body: "",
          multipart: [],
          exampleIncluded: false,
          redacted: sanitized.redacted,
          warning: "BODY_TOO_LARGE",
        }
      : {
          kind: "form",
          body,
          form_fields: Array.from(form, ([key, value]) => ({ key, value })),
          multipart: [],
          exampleIncluded: true,
          redacted: sanitized.redacted,
          warning: null,
        };
  }
  if (normalized === "application/json" || normalized.endsWith("+json")) {
    let body: string;
    try {
      body = JSON.stringify(sanitized.value, null, 2);
    } catch {
      return {
        kind: "json",
        body: "",
        multipart: [],
        exampleIncluded: false,
        redacted: sanitized.redacted,
        warning: "BODY_EXAMPLE_OMITTED",
      };
    }
    if (byteLength(body) > OPENAPI_LIMITS.maxBodyBytes) {
      return {
        kind: "json",
        body: "",
        multipart: [],
        exampleIncluded: false,
        redacted: sanitized.redacted,
        warning: "BODY_TOO_LARGE",
      };
    }
    return { kind: "json", body, multipart: [], exampleIncluded: true, redacted: sanitized.redacted, warning: null };
  }
  // Non-JSON raw examples can be legitimate documentation, but may also be a
  // bearer/password value. Leave them out unless the source was structured.
  return {
    kind: "raw",
    body: "",
    multipart: [],
    exampleIncluded: false,
    redacted: sanitized.redacted,
    warning: "BODY_EXAMPLE_OMITTED",
  };
}

export function chooseMedia(content: Record<string, unknown>): string | null {
  const keys = Object.keys(content)
    .filter((key) => mediaType(key))
    .sort();
  if (keys.length === 0) return null;
  const json = keys.find((key) => key.toLowerCase().split(";", 1)[0] === "application/json");
  return (
    json ??
    keys.find((key) => key.toLowerCase().split(";", 1)[0]!.endsWith("+json")) ??
    keys.find((key) => key.toLowerCase().split(";", 1)[0] === "application/x-www-form-urlencoded") ??
    keys.find((key) => key.toLowerCase().split(";", 1)[0] === "multipart/form-data") ??
    keys[0]!
  );
}
