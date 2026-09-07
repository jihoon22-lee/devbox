import type { BinaryResponse } from "../types";

/** Maximum response bytes retained for either text or binary projection. */
export const MAX_RESPONSE_BODY_BYTES = 16 * 1024 * 1024;
export const MAX_BINARY_PREVIEW_BYTES = 4 * 1024;
export const MAX_BINARY_TEXT_PREVIEW_BYTES = 4 * 1024;

const TEXT_MEDIA_TYPE = /^(?:text\/|application\/(?:json|[^;]+\+json|xml|[^;]+\+xml|javascript|x-javascript|yaml|x-yaml|graphql|csv|x-www-form-urlencoded))/u;

export function isTextMediaType(mediaType: string): boolean {
  return TEXT_MEDIA_TYPE.test(mediaType.trim().toLowerCase());
}

function strictUtf8(bytes: Uint8Array): string | null {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

function hasBinaryMarkers(bytes: Uint8Array): boolean {
  let controls = 0;
  for (const byte of bytes) {
    if (byte === 0) return true;
    if (byte < 9 || (byte > 13 && byte < 32)) controls += 1;
  }
  return controls > Math.max(2, bytes.length / 20);
}

export function isBinaryResponse(mediaType: string, bytes: Uint8Array): boolean {
  if (isTextMediaType(mediaType)) return strictUtf8(bytes) === null || hasBinaryMarkers(bytes);
  if (mediaType.trim()) return true;
  return strictUtf8(bytes) === null || hasBinaryMarkers(bytes);
}

function hex(bytes: Uint8Array): string {
  let output = "";
  for (const byte of bytes) output += byte.toString(16).padStart(2, "0");
  return output;
}

function redactKnownTokens(value: string): string {
  return value.replace(
    /(?:sk-|ghp_|github_pat_|glpat-|xox[bprsa]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/gu,
    "[REDACTED]",
  );
}

function byteTruncate(value: string, maxBytes: number): { value: string; truncated: boolean } {
  const encoded = new TextEncoder().encode(value);
  if (encoded.byteLength <= maxBytes) return { value, truncated: false };
  let end = Math.max(0, maxBytes);
  while (end > 0 && (encoded[end] & 0xc0) === 0x80) end -= 1;
  return { value: new TextDecoder().decode(encoded.slice(0, end)) + "…", truncated: true };
}

export type BinaryTextRedactor = (value: string) => string;

/** Create a bounded, masked projection. The input bytes are never returned. */
export function projectBinaryResponse(
  mediaType: string,
  bytes: Uint8Array,
  redactText: BinaryTextRedactor = redactKnownTokens,
  saveAvailable = false,
): BinaryResponse {
  const decoded = strictUtf8(bytes);
  const lossy = new TextDecoder().decode(bytes);
  const safeLossy = redactKnownTokens(redactText(lossy));
  const safeText = decoded === null ? null : redactKnownTokens(redactText(decoded));
  const containsSecret = safeLossy !== lossy || (decoded !== null && safeText !== decoded);
  const shownBytes = bytes.slice(0, MAX_BINARY_PREVIEW_BYTES);
  const hexPreview = containsSecret ? "[REDACTED]" : `${hex(shownBytes)}${bytes.length > shownBytes.length ? "…" : ""}`;
  const text = safeText === null ? null : byteTruncate(safeText, MAX_BINARY_TEXT_PREVIEW_BYTES);
  return {
    media_type: mediaType || "application/octet-stream",
    size_bytes: bytes.byteLength,
    hex_preview: hexPreview,
    text_preview: text?.value ?? null,
    hex_truncated: bytes.length > shownBytes.length,
    text_truncated: text?.truncated ?? false,
    save_available: saveAvailable,
  };
}
