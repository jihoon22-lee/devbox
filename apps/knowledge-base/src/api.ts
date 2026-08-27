import { invoke } from "@tauri-apps/api/core";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { isTauri } from "./lib/isTauri";
import {
  bytesToBase64,
  IMAGE_DESKTOP_ONLY_ERROR,
  IMAGE_RESULT_ERROR,
  IMAGE_TOO_LARGE_ERROR,
  MAX_IMAGE_ASSET_BYTES,
  relativeAssetDestination,
  validateImageAssetResult,
} from "./lib/imageAssets";
import type {
  Backlink,
  RenderedDoc,
  QuickCaptureInput,
  QuickCapturePreview,
  QuickCaptureSaved,
  QuickCaptureShortcutStatus,
  SearchResult,
  TreeEntry,
  WikilinkCandidate,
  WikilinkOccurrence,
  ImageAsset,
} from "./types";
import {
  isSafeQuickCapturePath,
  isSafeQuickCapturePreviewId,
  isQuickCaptureUtf8Within,
  normalizeQuickCapture,
  MAX_QUICK_CAPTURE_PREVIEW_ID_BYTES,
  MAX_QUICK_CAPTURE_TAGS,
  MAX_QUICK_CAPTURE_TAG_ITEM_BYTES,
  QUICK_CAPTURE_TARGET,
} from "./lib/quickCapture";

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

export interface InboundNote {
  path: string;
  content: string;
}

export interface KnowledgeOpenTarget {
  id: string;
  displayName: string;
}

export interface RenameDiffItem {
  path: string;
  before: string;
  after: string;
  meta: string;
}

export interface RenamePreview {
  planId: string;
  from: string;
  to: string;
  isDir: boolean;
  items: RenameDiffItem[];
}

export interface RenameApplied {
  from: string;
  to: string;
}

const MOCK_OPEN_TARGETS: KnowledgeOpenTarget[] = [
  { id: "code-pad", displayName: "Code Pad" },
  { id: "workbench", displayName: "Workbench" },
];

const MOCK_TREE: TreeEntry[] = [
  { path: "Projects", is_dir: true },
  { path: "Projects/FamilyCard.md", is_dir: false },
  { path: "Notes", is_dir: true },
  { path: "Notes/tauri-study.md", is_dir: false },
  { path: "Journal", is_dir: true },
  { path: "Journal/2026-08-11.md", is_dir: false },
];

export async function getRoot(): Promise<string> {
  if (!isTauri()) return "C:\\Users\\me\\Documents\\Knowledge";
  return invoke<string>("get_root");
}

export async function listTree(): Promise<TreeEntry[]> {
  if (!isTauri()) {
    return [
      { path: "Notes/2026-08-14.md", is_dir: false },
      { path: "Projects/devbox.md", is_dir: false },
    ];
  }
  return invoke<TreeEntry[]>("list_tree");
}

export async function onDocsChanged(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen("docs-changed", () => cb());
}

export async function readFile(rel: string): Promise<string> {
  if (!isTauri()) {
    return rel.endsWith(".md") ? "# Mock note\n\nEdit me." : "";
  }
  return invoke<string>("read_file", { rel });
}

export async function openInboundNote(path: string): Promise<InboundNote> {
  if (!isTauri()) {
    const normalized = path.replace(/\\/g, "/");
    return { path: normalized.split("/Knowledge/").pop() ?? normalized, content: "# Mock note\n" };
  }
  return invoke<InboundNote>("open_inbound_note", { path });
}

/** Cold start 또는 같은 실행 중 instance가 남긴 요청을 한 번 가져온다. */
export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

/** Hot-instance relaunch 알림. payload 대신 pending pull을 authoritative하게 사용한다. */
export async function onOpenRequest(cb: (request: OpenRequest) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<OpenRequest>("devbox://open", (event) => cb(event.payload));
}

export async function writeFile(rel: string, content: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("write_file", { rel, content });
}

export async function createFile(rel: string, content?: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("create_file", { rel, content });
}

export async function createDirectory(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("create_directory", { rel });
}

export async function previewRename(from: string, to: string): Promise<RenamePreview> {
  if (!isTauri()) {
    return {
      planId: "mock-rename",
      from,
      to,
      isDir: false,
      items: [{ path: `이름 변경 · ${from}`, before: from, after: to, meta: "파일 이동" }],
    };
  }
  return invoke<RenamePreview>("preview_rename", { from, to });
}

export async function applyRename(planId: string): Promise<RenameApplied> {
  if (!isTauri()) {
    return { from: "", to: "" };
  }
  return invoke<RenameApplied>("apply_rename", { planId });
}

export async function discardRenamePreview(planId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("discard_rename_preview", { planId });
}

export async function deleteFile(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("delete_file", { rel });
}

export async function entryPath(rel: string): Promise<string> {
  if (!isTauri()) return rel;
  return invoke<string>("entry_path", { rel });
}

export async function revealEntry(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("reveal_entry", { rel });
}

export async function openTargets(): Promise<KnowledgeOpenTarget[]> {
  if (!isTauri()) return MOCK_OPEN_TARGETS;
  return invoke<KnowledgeOpenTarget[]>("open_targets");
}

export async function openIn(appId: string, rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_in", { appId, rel });
}

/** 편집기 메뉴에서 사용자가 Paste를 선택한 순간에만 plain text를 읽는다. */
export async function readClipboardText(maxBytes?: number): Promise<string> {
  const text = !isTauri() ? await navigator.clipboard.readText() : await readText();
  if (maxBytes !== undefined && !isQuickCaptureUtf8Within(text, maxBytes)) {
    throw new Error("본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요");
  }
  return text;
}

/**
 * Read an image only from an explicit Paste menu action. The native clipboard
 * plugin currently exposes text reliably across the supported desktop targets;
 * Clipboard API image reads preserve the original PNG/JPEG bytes instead of
 * converting them to an unbounded raw RGBA buffer. Keyboard paste and drop use
 * their browser events and do not call this helper.
 */
export async function readClipboardImage(): Promise<File | null> {
  const clipboard = navigator.clipboard;
  if (!clipboard || typeof clipboard.read !== "function") return null;
  const items = await clipboard.read();
  for (const item of items) {
    const type = item.types.find((candidate) => candidate.trim().toLowerCase().startsWith("image/"));
    if (!type) continue;
    const blob = await item.getType(type);
    if (!Number.isFinite(blob.size) || blob.size <= 0) continue;
    if (blob.size > MAX_IMAGE_ASSET_BYTES) throw new Error(IMAGE_TOO_LARGE_ERROR);
    return new File([blob], "clipboard-image", { type });
  }
  return null;
}

export async function saveImageAsset(noteRel: string, bytes: Uint8Array): Promise<ImageAsset> {
  if (!relativeAssetDestination(noteRel, `assets/${"0".repeat(64)}.png`)) {
    throw new Error(IMAGE_RESULT_ERROR);
  }
  if (bytes.byteLength > MAX_IMAGE_ASSET_BYTES) throw new Error(IMAGE_TOO_LARGE_ERROR);
  if (!isTauri()) throw new Error(IMAGE_DESKTOP_ONLY_ERROR);
  const result = await invoke<ImageAsset>("save_image_asset", {
    request: { noteRel, bytesBase64: bytesToBase64(bytes) },
  });
  return validateImageAssetResult(noteRel, result);
}

export async function searchDocs(query: string): Promise<SearchResult[]> {
  if (!isTauri()) {
    return MOCK_TREE.filter((t) => !t.is_dir && t.path.toLowerCase().includes(query.toLowerCase())).map((t) => ({ path: t.path, title: t.path.split("/").pop() ?? t.path }));
  }
  return invoke<SearchResult[]>("search_docs", { query });
}

export async function listTags(): Promise<string[]> {
  if (!isTauri()) return ["rust", "tauri", "daily"];
  return invoke<string[]>("list_tags");
}

export async function analyzeWikilinks(content: string): Promise<WikilinkOccurrence[]> {
  if (!isTauri()) return [];
  return invoke<WikilinkOccurrence[]>("analyze_wikilinks", { content });
}

export async function wikilinkCandidates(query: string): Promise<WikilinkCandidate[]> {
  if (!isTauri()) {
    const normalized = query.trim().toLowerCase();
    return MOCK_TREE
      .filter((entry) => !entry.is_dir && entry.path.toLowerCase().endsWith(".md"))
      .filter((entry) => entry.path.toLowerCase().includes(normalized))
      .slice(0, 100)
      .map((entry) => ({
        path: entry.path,
        title: entry.path.split("/").pop()?.replace(/\.md$/iu, "") ?? entry.path,
        link_target: entry.path.replace(/\.md$/iu, ""),
      }));
  }
  return invoke<WikilinkCandidate[]>("wikilink_candidates", { query });
}

export async function backlinks(rel: string): Promise<Backlink[]> {
  if (!isTauri()) return [];
  return invoke<Backlink[]>("backlinks", { rel });
}

export async function dailyNote(): Promise<[string, string]> {
  if (!isTauri()) return ["Journal/2026-08-11.md", "# Today\n"];
  return invoke<[string, string]>("daily_note");
}

const QUICK_CAPTURE_UNAVAILABLE = "빠른 캡처 저장은 Knowledge 앱에서만 사용할 수 있습니다";
const QUICK_CAPTURE_PREVIEW_FAILED = "빠른 캡처 미리보기를 만들 수 없습니다";
const QUICK_CAPTURE_SAVE_FAILED = "빠른 캡처를 저장하지 못했습니다";
const QUICK_CAPTURE_SHORTCUT = "Ctrl+Alt+K";
const QUICK_CAPTURE_SHORTCUT_STATES: QuickCaptureShortcutStatus["state"][] = [
  "registering",
  "registered",
  "conflict",
  "unsupported",
  "unavailable",
];

function safeQuickCaptureShortcutStatus(value: unknown): QuickCaptureShortcutStatus {
  if (typeof value === "object" && value !== null) {
    const candidate = value as { shortcut?: unknown; state?: unknown };
    if (
      candidate.shortcut === QUICK_CAPTURE_SHORTCUT
      && typeof candidate.state === "string"
      && QUICK_CAPTURE_SHORTCUT_STATES.includes(candidate.state as QuickCaptureShortcutStatus["state"])
    ) {
      return {
        shortcut: QUICK_CAPTURE_SHORTCUT,
        state: candidate.state as QuickCaptureShortcutStatus["state"],
      };
    }
  }
  return { shortcut: QUICK_CAPTURE_SHORTCUT, state: "unavailable" };
}

function safeQuickCaptureError(error: unknown, fallback: string): Error {
  // Tauri command rejections are strings in production, while browser mocks
  // commonly reject Error instances. Treat both shapes as untrusted and only
  // preserve an explicitly allowlisted fixed message.
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  // Native commands only expose these stable validation/storage messages.  A
  // defensive allowlist keeps an unexpected OS/IPC string out of the UI.
  if (
    message === "빠른 캡처 본문을 입력하세요"
    || message === "민감한 정보가 포함되어 있어 저장하지 않았습니다"
    || message === "빠른 캡처 입력이 올바르지 않습니다"
    || message === "제목은 UTF-8 800바이트·200자 이내로 입력하세요"
    || message === "본문은 LF 기준 64 KiB(원문 128 KiB) 이내로 입력하세요"
    || message === "태그는 최대 20개까지 입력하세요"
    || message === "태그 하나는 UTF-8 192바이트·48자 이내로 입력하세요"
    || message === "태그 전체는 UTF-8 1 KiB 이내로 입력하세요"
    || message === "태그에 줄바꿈·쉼표·대괄호·따옴표를 사용할 수 없습니다"
    || message === "빠른 캡처 미리보기가 오래되어 다시 확인하세요"
  ) {
    return new Error(message);
  }
  return new Error(fallback);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseQuickCapturePreview(value: unknown): QuickCapturePreview {
  if (!isRecord(value)) throw new Error("invalid quick capture preview");
  const previewId = value.previewId;
  const target = value.target;
  const title = value.title;
  const body = value.body;
  const tags = value.tags;
  if (
    !isSafeQuickCapturePreviewId(previewId)
    || previewId.length > MAX_QUICK_CAPTURE_PREVIEW_ID_BYTES
    || target !== QUICK_CAPTURE_TARGET
    || typeof title !== "string"
    || typeof body !== "string"
    || !Array.isArray(tags)
    || tags.length > MAX_QUICK_CAPTURE_TAGS
    || tags.some((tag) => typeof tag !== "string" || !isQuickCaptureUtf8Within(tag, MAX_QUICK_CAPTURE_TAG_ITEM_BYTES))
  ) {
    throw new Error("invalid quick capture preview");
  }
  const normalized = normalizeQuickCapture({ title, body, tags });
  return { previewId, target: QUICK_CAPTURE_TARGET, ...normalized };
}

function safeQuickCaptureApprovalId(value: string): string {
  if (!isSafeQuickCapturePreviewId(value)) {
    throw new Error("빠른 캡처 미리보기가 오래되어 다시 확인하세요");
  }
  return value;
}

export async function previewQuickCapture(input: QuickCaptureInput): Promise<QuickCapturePreview> {
  const normalized = normalizeQuickCapture(input);
  if (!isTauri()) {
    return { previewId: "qc-1", target: QUICK_CAPTURE_TARGET, ...normalized };
  }
  try {
    const preview = await invoke<unknown>("preview_quick_capture", { input: normalized });
    return parseQuickCapturePreview(preview);
  } catch (error) {
    throw safeQuickCaptureError(error, QUICK_CAPTURE_PREVIEW_FAILED);
  }
}

export async function saveQuickCapture(previewId: string): Promise<QuickCaptureSaved> {
  const safePreviewId = safeQuickCaptureApprovalId(previewId);
  if (!isTauri()) throw new Error(QUICK_CAPTURE_UNAVAILABLE);
  try {
    const saved = await invoke<unknown>("save_quick_capture", {
      approval: { previewId: safePreviewId },
    });
    if (!isRecord(saved) || !isSafeQuickCapturePath(saved.path)) {
      throw new Error("unexpected save path");
    }
    return { path: saved.path as string };
  } catch (error) {
    throw safeQuickCaptureError(error, QUICK_CAPTURE_SAVE_FAILED);
  }
}

export async function discardQuickCapturePreview(previewId: string): Promise<void> {
  if (!isTauri() || !isSafeQuickCapturePreviewId(previewId)) return;
  try {
    await invoke("discard_quick_capture_preview", {
      approval: { previewId },
    });
  } catch {
    // Discard is best-effort during modal teardown.  The native slot is
    // one-shot and issuing a new preview also replaces any older slot.
  }
}

export async function quickCaptureShortcutStatus(): Promise<QuickCaptureShortcutStatus> {
  if (!isTauri()) {
    return { shortcut: QUICK_CAPTURE_SHORTCUT, state: "unsupported" };
  }
  try {
    return safeQuickCaptureShortcutStatus(await invoke<QuickCaptureShortcutStatus>("shortcut_status"));
  } catch {
    return { shortcut: QUICK_CAPTURE_SHORTCUT, state: "unavailable" };
  }
}

export async function onQuickCaptureRequested(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen("knowledge://quick-capture", () => cb());
}

export async function onQuickCaptureShortcutStatusChanged(
  cb: (status: QuickCaptureShortcutStatus) => void,
): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<QuickCaptureShortcutStatus>("knowledge://quick-capture-shortcut-status", (event) => {
    cb(safeQuickCaptureShortcutStatus(event.payload));
  });
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export async function renderMarkdown(rel: string, content: string): Promise<RenderedDoc> {
  if (!isTauri()) {
    // 목업: 실제 마크다운 파서 없이도 프리뷰 레이아웃을 확인할 수 있게 원문을 그대로 보여준다.
    return { title: null, tags: [], html: `<pre>${escapeHtml(content)}</pre>`, mermaid: [] };
  }
  return invoke<RenderedDoc>("render_markdown", { rel, content });
}

/** 외부 URL을 기본 브라우저로 연다. Tauri 밖(브라우저 미리보기)에서는 새 탭으로 대신 연다. */
export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}
