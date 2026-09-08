import type { ImageAsset } from "../types";

export const MAX_IMAGE_ASSET_BYTES = 2 * 1024 * 1024;

export const IMAGE_ERROR = "이미지 자산을 저장할 수 없습니다";
export const IMAGE_TOO_LARGE_ERROR = "이미지가 너무 큽니다";
export const IMAGE_FORMAT_ERROR = "지원하지 않는 이미지 형식입니다";
export const IMAGE_CLIPBOARD_ERROR = "클립보드에서 이미지를 읽지 못했습니다";
export const IMAGE_BUSY_ERROR = "이미지 처리가 진행 중입니다";
export const IMAGE_STALE_ERROR = "현재 문서가 변경되어 이미지를 삽입하지 못했습니다";
export const IMAGE_RESULT_ERROR = "이미지 자산 결과가 올바르지 않습니다";
export const IMAGE_DESKTOP_ONLY_ERROR = "이미지 자산 저장은 데스크톱 앱에서만 사용할 수 있습니다";
export const IMAGE_MULTIPLE_ERROR = "한 번에 하나의 이미지만 처리할 수 있습니다";

const SAFE_IMAGE_ERRORS = new Set([
  IMAGE_ERROR,
  IMAGE_TOO_LARGE_ERROR,
  IMAGE_FORMAT_ERROR,
  IMAGE_CLIPBOARD_ERROR,
  IMAGE_BUSY_ERROR,
  IMAGE_STALE_ERROR,
  IMAGE_RESULT_ERROR,
  IMAGE_DESKTOP_ONLY_ERROR,
  IMAGE_MULTIPLE_ERROR,
]);

function isPotentialImageMime(type: string): boolean {
  const normalized = type.trim().toLowerCase();
  return normalized === "" || normalized === "application/octet-stream" || normalized.startsWith("image/");
}

export function isPotentialImageFile(file: File): boolean {
  const normalized = file.type.trim().toLowerCase();
  // File drops have a reliable MIME in supported desktop browsers. Do not
  // hijack an unknown/empty-type directory or text drop; clipboard items use
  // the broader heuristic above because some WebViews omit their MIME hint.
  return normalized === "application/octet-stream" || normalized.startsWith("image/");
}

export function clipboardImageFiles(data: DataTransfer | null): File[] {
  if (!data) return [];
  const files: File[] = [];
  for (const item of data.items) {
    if (item.kind !== "file" || !isPotentialImageMime(item.type)) continue;
    const file = item.getAsFile();
    if (file && !files.includes(file)) {
      files.push(file);
      // The editor only accepts one image. Retaining two is sufficient to
      // report a multi-file action while bounding a hostile DataTransfer.
      if (files.length === 2) break;
    }
  }
  return files;
}

export function droppedImageFiles(data: DataTransfer | null): File[] {
  if (!data) return [];
  const files: File[] = [];
  for (const file of data.files) {
    if (!isPotentialImageFile(file)) continue;
    files.push(file);
    if (files.length === 2) break;
  }
  return files;
}

export async function readImageBytes(file: File): Promise<Uint8Array> {
  if (!Number.isFinite(file.size) || file.size <= 0) {
    throw new Error(IMAGE_FORMAT_ERROR);
  }
  if (file.size > MAX_IMAGE_ASSET_BYTES) {
    throw new Error(IMAGE_TOO_LARGE_ERROR);
  }
  try {
    const buffer = await file.arrayBuffer();
    const bytes = new Uint8Array(buffer);
    if (bytes.length === 0) throw new Error(IMAGE_FORMAT_ERROR);
    if (bytes.length > MAX_IMAGE_ASSET_BYTES) throw new Error(IMAGE_TOO_LARGE_ERROR);
    return bytes;
  } catch (cause) {
    if (cause instanceof Error && SAFE_IMAGE_ERRORS.has(cause.message)) throw cause;
    throw new Error(IMAGE_CLIPBOARD_ERROR);
  }
}

export function bytesToBase64(bytes: Uint8Array): string {
  if (bytes.byteLength > MAX_IMAGE_ASSET_BYTES) throw new Error(IMAGE_TOO_LARGE_ERROR);
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

const SAFE_ASSET_PATH = /^assets\/[0-9a-f]{64}\.(?:png|jpg|gif|webp)$/u;

function noteDirectoryDepth(noteRel: string): number | null {
  if (
    !noteRel
    || noteRel.length > 4 * 1024
    || !noteRel.endsWith(".md")
    || noteRel.includes("\\")
    || noteRel.includes("\0")
    || /[\u0000-\u001f\u007f]/u.test(noteRel)
  ) {
    return null;
  }
  const parts = noteRel.split("/");
  if (parts.some((part) => !part || part === "." || part === ".." || part.includes(":"))) {
    return null;
  }
  return Math.max(0, parts.length - 1);
}

export function relativeAssetDestination(noteRel: string, assetRel: string): string | null {
  const depth = noteDirectoryDepth(noteRel);
  if (depth === null || !SAFE_ASSET_PATH.test(assetRel)) return null;
  return "../".repeat(depth) + assetRel;
}

export function validateImageAssetResult(noteRel: string, result: ImageAsset): ImageAsset {
  if (
    typeof result?.relativePath !== "string"
    || typeof result.markdown !== "string"
    || typeof result.reused !== "boolean"
  ) {
    throw new Error(IMAGE_RESULT_ERROR);
  }
  const destination = relativeAssetDestination(noteRel, result.relativePath);
  if (!destination || result.markdown !== "![image](" + destination + ")") {
    throw new Error(IMAGE_RESULT_ERROR);
  }
  return result;
}

export function imageErrorMessage(cause: unknown, fallback = IMAGE_ERROR): string {
  const message = cause instanceof Error ? cause.message : "";
  return SAFE_IMAGE_ERRORS.has(message) ? message : fallback;
}
