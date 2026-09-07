import type { MultipartPart } from "../types";

export const MAX_MULTIPART_PARTS = 50;
export const MAX_MULTIPART_TEXT_BYTES = 1_000_000;
export const MAX_MULTIPART_FILE_BYTES = 25 * 1024 * 1024;
export const MAX_MULTIPART_TOTAL_FILE_BYTES = 50 * 1024 * 1024;

const PART_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const CONTENT_TYPE = /^[A-Za-z0-9!#$&^_.+-]+\/[A-Za-z0-9!#$&^_.+-]+$/;
const textEncoder = new TextEncoder();

export interface MultipartValidationIssue {
  index: number;
  field: "parts" | "name" | "value" | "file" | "content_type";
  message: string;
}

export interface PickedMultipartFile {
  path: string;
  name: string;
}

export function emptyMultipartPart(kind: MultipartPart["kind"] = "text"): MultipartPart {
  return {
    kind,
    name: "",
    value: "",
    file_path: "",
    file_name: "",
    content_type: "",
    enabled: true,
  };
}

export function isMultipartPart(value: unknown): value is MultipartPart {
  if (!value || typeof value !== "object") return false;
  const part = value as Partial<MultipartPart>;
  return (
    (part.kind === "text" || part.kind === "file") &&
    typeof part.name === "string" &&
    typeof part.value === "string" &&
    typeof part.file_path === "string" &&
    typeof part.file_name === "string" &&
    typeof part.content_type === "string" &&
    (part.enabled === undefined || typeof part.enabled === "boolean")
  );
}

export function isMultipartPartEnabled(part: MultipartPart): boolean {
  return part.enabled !== false;
}

export function isMultipartDerivedHeader(name: string): boolean {
  return ["content-type", "content-length", "transfer-encoding"].includes(
    name.trim().toLowerCase().replace(/_/g, "-"),
  );
}

export function normalizeMultipartParts(
  parts: readonly MultipartPart[] | undefined,
): MultipartPart[] {
  return (parts ?? []).slice(0, MAX_MULTIPART_PARTS).map((part) => ({
    kind: part.kind,
    name: part.name,
    value: part.kind === "text" ? part.value : "",
    file_path: part.kind === "file" ? part.file_path : "",
    file_name: part.kind === "file" ? safeMultipartFileName(part.file_name) : "",
    content_type: part.content_type,
    enabled: isMultipartPartEnabled(part),
  }));
}

export function addMultipartPart(
  parts: readonly MultipartPart[],
  kind: MultipartPart["kind"] = "text",
): MultipartPart[] {
  if (parts.length >= MAX_MULTIPART_PARTS) return normalizeMultipartParts(parts);
  return [...normalizeMultipartParts(parts), emptyMultipartPart(kind)];
}

export function updateMultipartPart(
  parts: readonly MultipartPart[],
  index: number,
  patch: Partial<MultipartPart>,
): MultipartPart[] {
  return normalizeMultipartParts(parts).map((part, candidate) => {
    if (candidate !== index) return part;
    const next = { ...part, ...patch };
    if (next.kind === part.kind) return next;
    return { ...emptyMultipartPart(next.kind), name: part.name, enabled: part.enabled };
  });
}

export function setMultipartFile(
  parts: readonly MultipartPart[],
  index: number,
  file: PickedMultipartFile,
): MultipartPart[] {
  return updateMultipartPart(parts, index, {
    kind: "file",
    file_path: file.path,
    file_name: safeMultipartFileName(file.name || file.path),
    value: "",
  });
}

export function duplicateMultipartPart(
  parts: readonly MultipartPart[],
  index: number,
): MultipartPart[] {
  const normalized = normalizeMultipartParts(parts);
  if (normalized.length >= MAX_MULTIPART_PARTS || !normalized[index]) return normalized;
  return [
    ...normalized.slice(0, index + 1),
    { ...normalized[index] },
    ...normalized.slice(index + 1),
  ];
}

export function removeMultipartPart(
  parts: readonly MultipartPart[],
  index: number,
): MultipartPart[] {
  return normalizeMultipartParts(parts).filter((_, candidate) => candidate !== index);
}

export function multipartPartHasContent(part: MultipartPart): boolean {
  return Boolean(
    part.name || part.value || part.file_path || part.file_name || part.content_type,
  );
}

export function validateMultipartParts(
  parts: readonly MultipartPart[],
): MultipartValidationIssue[] {
  const issues: MultipartValidationIssue[] = [];
  if (parts.length > MAX_MULTIPART_PARTS) {
    issues.push({
      index: MAX_MULTIPART_PARTS,
      field: "parts",
      message: "multipart는 최대 50개 part까지 사용할 수 있습니다.",
    });
  }

  let textBytes = 0;
  normalizeMultipartParts(parts).forEach((part, index) => {
    if (!isMultipartPartEnabled(part) || !multipartPartHasContent(part)) return;
    if (!part.name) {
      issues.push({ index, field: "name", message: "part 이름이 필요합니다." });
    } else if (!PART_NAME.test(part.name) || part.name.length > 120) {
      issues.push({
        index,
        field: "name",
        message: "part 이름은 120자 이하의 HTTP token이어야 합니다.",
      });
    }
    if (part.content_type &&
      (part.content_type.length > 127 || !CONTENT_TYPE.test(part.content_type))) {
      issues.push({
        index,
        field: "content_type",
        message: "Content-Type은 type/subtype 형식이어야 합니다.",
      });
    }
    if (part.kind === "file") {
      if (!part.file_path) {
        issues.push({
          index,
          field: "file",
          message: part.file_name
            ? `'${part.file_name}' 파일을 다시 선택하세요.`
            : "전송할 파일을 선택하세요.",
        });
      } else if (part.file_path.length > 32_768 || /[\0\r\n]/.test(part.file_path)) {
        issues.push({ index, field: "file", message: "선택한 파일 경로가 올바르지 않습니다." });
      }
    } else {
      textBytes += textEncoder.encode(part.value).length;
    }
  });

  if (textBytes > MAX_MULTIPART_TEXT_BYTES) {
    issues.push({
      index: 0,
      field: "value",
      message: "활성 text part 전체는 UTF-8 기준 1,000,000바이트 이하여야 합니다.",
    });
  }
  return issues;
}

export function safeMultipartFileName(path: string): string {
  return (path.split(/[\\/]/).pop() ?? "")
    .replace(/[\0-\x1F\x7F]/g, "")
    .slice(0, 255);
}
