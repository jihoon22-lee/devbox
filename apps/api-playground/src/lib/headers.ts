import type { RequestHeader } from "../types";

export const MAX_REQUEST_HEADER_ROWS = 100;

export function isRequestHeader(value: unknown): value is RequestHeader {
  if (!value || typeof value !== "object") return false;
  const header = value as Partial<RequestHeader>;
  return (
    typeof header.key === "string"
    && typeof header.value === "string"
    && (header.enabled === undefined || typeof header.enabled === "boolean")
  );
}

export function isHeaderEnabled(header: RequestHeader): boolean {
  return header.enabled !== false;
}

export function normalizeHeader(header: RequestHeader): RequestHeader {
  return { key: header.key, value: header.value, enabled: isHeaderEnabled(header) };
}

export function normalizeHeaders(headers: readonly RequestHeader[]): RequestHeader[] {
  return headers.slice(0, MAX_REQUEST_HEADER_ROWS).map(normalizeHeader);
}

export function addHeader(headers: readonly RequestHeader[]): RequestHeader[] {
  if (headers.length >= MAX_REQUEST_HEADER_ROWS) return normalizeHeaders(headers);
  return [...normalizeHeaders(headers), { key: "", value: "", enabled: true }];
}

export function updateHeader(
  headers: readonly RequestHeader[],
  index: number,
  patch: Partial<RequestHeader>,
): RequestHeader[] {
  return normalizeHeaders(headers).map((header, current) => (
    current === index ? normalizeHeader({ ...header, ...patch }) : header
  ));
}

export function removeHeader(headers: readonly RequestHeader[], index: number): RequestHeader[] {
  return normalizeHeaders(headers).filter((_, current) => current !== index);
}

export function duplicateHeader(headers: readonly RequestHeader[], index: number): RequestHeader[] {
  const normalized = normalizeHeaders(headers);
  if (normalized.length >= MAX_REQUEST_HEADER_ROWS || !normalized[index]) return normalized;
  return [
    ...normalized.slice(0, index + 1),
    { ...normalized[index]! },
    ...normalized.slice(index + 1),
  ];
}

export function secretReference(name: string): string | null {
  return /^[A-Za-z0-9_.-]+$/u.test(name) ? `\${${name}}` : null;
}

export function availableSecretNames(names: readonly string[]): string[] {
  return [...new Set(names.filter((name) => secretReference(name) !== null))]
    .sort((left, right) => left < right ? -1 : left > right ? 1 : 0);
}

export function duplicateHeaderNameCount(headers: readonly RequestHeader[]): number {
  const counts = new Map<string, number>();
  for (const header of headers) {
    const name = header.key.trim().toLowerCase();
    if (name) counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  return [...counts.values()].filter((count) => count > 1).length;
}
