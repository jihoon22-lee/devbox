import { WorkspaceOperationError } from "../../transport";
import { normalizeBookmarkLines } from "../editor/bookmarks";
import { docIdForPath } from "../store/documentStore";
import type { Doc, EditorState, OpenedFile, SessionState, LspRenameApplyResult } from "../types";

export function docFromOpenedFile(file: OpenedFile, metadata?: SessionState["docs"][number]): Doc {
  return {
    ...(file.nativeRevision !== undefined ? { nativeRevision: file.nativeRevision } : {}),
    id: metadata?.id ?? docIdForPath(file.path),
    path: file.path,
    text: file.text,
    encoding: file.encoding,
    lineEnding: file.lineEnding,
    readOnly: file.readOnly,
    size: file.size,
    mtimeNanos: file.mtimeNanos,
    contentHash: file.contentHash,
    lossy: file.lossy,
    durabilityWarning: file.durabilityWarning ?? null,
    dirty: false,
    revision: 0,
    cursor: Math.min(metadata?.cursor ?? 0, file.text.length),
    bookmarks: normalizeBookmarkLines(file.text, metadata?.bookmarks ?? []),
  };
}

export function activeDocForState(state: EditorState): Doc | null {
  const activeId = state.activeDocByView[state.activeView];
  return state.docs.find((doc) => doc.id === activeId) ?? null;
}

export function isPreviewable(path: string): boolean {
  const normalized = path.split("\\").join("/").toLowerCase();
  return normalized.endsWith(".md") || normalized.endsWith(".markdown") || normalized.endsWith(".mmd");
}

export function fileNameForPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function relativeWorkspacePath(path: string, workspaceRoot: string): string | null {
  const normalize = (value: string) => {
    const slashValue = value.replace(/\\/gu, "/");
    const prefix = slashValue.startsWith("//") ? "//" : "";
    const normalized = `${prefix}${slashValue.slice(prefix.length).replace(/\/{2,}/gu, "/")}`;
    if (normalized === "/" || /^[A-Za-z]:\/$/u.test(normalized)) return normalized;
    return normalized.replace(/\/$/u, "");
  };
  const normalizedPath = normalize(path);
  const normalizedRoot = normalize(workspaceRoot);
  const windowsPath =
    /^[A-Za-z]:\//u.test(normalizedPath) ||
    /^[A-Za-z]:\//u.test(normalizedRoot) ||
    normalizedPath.startsWith("//") ||
    normalizedRoot.startsWith("//");
  const candidate = windowsPath ? normalizedPath.toLowerCase() : normalizedPath;
  const root = windowsPath ? normalizedRoot.toLowerCase() : normalizedRoot;
  if (candidate === root) return "";
  const prefix = root === "/" || /^[a-z]:\/$/u.test(root) ? root : `${root}/`;
  if (!candidate.startsWith(prefix)) return null;
  return normalizedPath.slice(prefix.length);
}

export function renameFileStatusLabel(status: LspRenameApplyResult["files"][number]["status"]): string {
  switch (status) {
    case "applied":
      return "적용됨";
    case "rolledBack":
      return "되돌림";
    case "failed":
      return "실패";
    case "notApplied":
      return "미적용";
    case "conflict":
      return "충돌";
    case "rollbackFailed":
      return "되돌리기 실패";
  }
}

export const SAFE_CODE_PAD_ERRORS = new Set([
  "파일 이름을 변경할 수 없습니다.",
  "파일 이름 변경 작업이 중단되었습니다.",
  "파일을 삭제할 수 없습니다.",
  "파일 삭제 작업이 중단되었습니다.",
  "파일 위치를 열 수 없습니다.",
  "클립보드 처리 중 선택 영역이 변경되어 잘라내기를 취소했습니다.",
  "클립보드 처리 중 편집 위치가 변경되어 붙여넣기를 취소했습니다.",
]);

export function safeCodePadError(cause: unknown, fallback: string): string {
  if (cause instanceof WorkspaceOperationError) return cause.message;
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  const message = raw.replace(/^Error:\s*/u, "").trim();
  if (SAFE_CODE_PAD_ERRORS.has(message)) return message;

  const normalized = message.toLowerCase();
  if (normalized.includes("read-only") || normalized.includes("read only")) {
    return "읽기 전용 파일이라 저장할 수 없습니다.";
  }
  if (normalized.includes("file changed on disk") || normalized.includes("changed during read")) {
    return "디스크의 파일이 변경되었습니다. 다시 불러온 뒤 시도하세요.";
  }
  if (normalized.includes("destination already exists")) {
    return "같은 이름의 파일이 이미 있습니다.";
  }
  if (normalized.includes("not a regular file") || normalized.includes("invalid sibling file name")) {
    return "일반 파일과 올바른 파일 이름만 사용할 수 있습니다.";
  }
  if (normalized.includes("larger than") || normalized.includes("size limit")) {
    return "파일이 안전 처리 크기 제한을 초과했습니다.";
  }
  if (normalized.includes("lossy fallback")) {
    return "손실 디코딩된 내용은 저장할 수 없습니다. 원본 인코딩으로 다시 여세요.";
  }
  if (normalized.includes("decode") || normalized.includes("invalid utf") || normalized.includes("invalid encoding")) {
    return "선택한 인코딩으로 파일을 읽지 못했습니다.";
  }
  if (normalized.includes("encode") || normalized.includes("unrepresentable")) {
    return "선택한 인코딩으로 저장할 수 없는 문자가 있습니다.";
  }
  return fallback;
}

export function snapshotMatches(
  doc: Doc | undefined,
  snapshot: Pick<
    Doc,
    "revision" | "text" | "mtimeNanos" | "size" | "contentHash" | "dirty" | "encoding" | "lineEnding" | "nativeRevision"
  >,
): boolean {
  return (
    doc !== undefined &&
    doc.dirty === snapshot.dirty &&
    doc.nativeRevision === snapshot.nativeRevision &&
    doc.revision === snapshot.revision &&
    doc.text === snapshot.text &&
    doc.mtimeNanos === snapshot.mtimeNanos &&
    doc.size === snapshot.size &&
    doc.contentHash === snapshot.contentHash &&
    doc.lineEnding === snapshot.lineEnding &&
    doc.encoding.encodingKind === snapshot.encoding.encodingKind &&
    doc.encoding.bom === snapshot.encoding.bom
  );
}

export function isWslContext(context: string): boolean {
  try {
    return JSON.parse(context)?.target?.kind === "wsl";
  } catch {
    return false;
  }
}
