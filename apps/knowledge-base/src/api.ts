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
  KnowledgeWatcherStatus,
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
  | { kind: "query"; text: string }
  | { kind: "task"; id: string }
  | { kind: "install"; appId: string }
  | { kind: "handoff"; handoffKind: string; id: string };

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

export interface KnowledgeDraftSummary {
  period: "day" | "week" | "month";
  startDate: string;
  endDate: string;
  timezone: string;
  filter: string | null;
  pcUsageMs: number;
  sessionCount: number;
  activeDays: number;
  totalDays: number;
  averageDailyUsageMs: number;
  gitCommits: number;
  topApp: string | null;
}

export interface KnowledgeDraftSource {
  id: string;
  available: boolean;
  schemaVersion: number | null;
  snapshotVersion: number | null;
  producerVersion: string | null;
  generatedAt: string | null;
  freshnessMs: number | null;
  view: string | null;
  scope: string;
  errorCode: string | null;
}

export interface KnowledgeDraftPreview {
  id: string;
  kind: "knowledge-draft/v1" | "knowledge-draft/v2";
  producerId: "life-log" | "developer-toolbox";
  expiresAtMs: number;
  leaseUntilMs: number;
  title: string;
  body: string;
  tags: string[];
  summary: KnowledgeDraftSummary | null;
  sources: KnowledgeDraftSource[];
}

export interface SaveKnowledgeDraftResult {
  saved: boolean;
  path: string;
  handoffDeleted: boolean;
  handoffStatusRecorded?: boolean;
}

export interface RenewKnowledgeDraftResult {
  leaseUntilMs: number;
}

export interface NoteTemplate {
  id: number;
  name: string;
  content: string;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface TemplateDraft {
  name: string;
  content: string;
}

export interface TemplateApplyInput {
  templateId: number;
  target: string;
  title: string;
  date: string;
  time: string;
}

export interface TemplatePreview {
  previewId: string;
  templateId: number;
  templateUpdatedAtMs: number;
  target: string;
  content: string;
  byteLength: number;
}

export interface SaveTemplateResult {
  saved: boolean;
  path: string;
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

const TEMPLATE_PLACEHOLDERS = ["{{title}}", "{{date}}", "{{time}}", "{{vault-relative-path}}"] as const;
const MAX_TEMPLATE_NAME_BYTES = 128;
const MAX_TEMPLATE_CONTENT_BYTES = 64 * 1024;
const MAX_TEMPLATE_OUTPUT_BYTES = 256 * 1024;
const MAX_TEMPLATE_TITLE_BYTES = 256;
const MAX_TEMPLATE_PATH_BYTES = 512;
const MAX_TEMPLATES = 100;
const TEMPLATE_PREVIEW_TTL_MS = 2 * 60 * 1_000;

function templateBytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function isTemplateControl(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return code <= 0x1f || (code >= 0x7f && code <= 0x9f);
}

function templateTextIsSafe(value: string, maxBytes: number, nonEmpty: boolean): boolean {
  return templateBytes(value) <= maxBytes
    && (!nonEmpty || value.trim().length > 0)
    && [...value].every((character) => !isTemplateControl(character)
      || "\n\r\t".includes(character));
}

function validateTemplatePlaceholders(content: string): void {
  let offset = 0;
  while (true) {
    const start = content.indexOf("{{", offset);
    if (start < 0) return;
    const end = content.indexOf("}}", start + 2);
    const token = end < 0 ? "" : content.slice(start, end + 2);
    if (!TEMPLATE_PLACEHOLDERS.includes(token as typeof TEMPLATE_PLACEHOLDERS[number])) {
      throw new Error("지원하지 않는 템플릿 변수가 있습니다");
    }
    offset = end + 2;
  }
}

function validateTemplateDraftInput(draft: TemplateDraft): void {
  if (!templateTextIsSafe(draft.name, MAX_TEMPLATE_NAME_BYTES, true)
    || /[\\/]/u.test(draft.name)
    || [...draft.name].some(isTemplateControl)) {
    throw new Error("템플릿 이름이 올바르지 않습니다");
  }
  if (!templateTextIsSafe(draft.content, MAX_TEMPLATE_CONTENT_BYTES, false)) {
    throw new Error("템플릿 본문이 올바르지 않습니다");
  }
  validateTemplatePlaceholders(draft.content);
}

function validateTemplateApplyInput(input: TemplateApplyInput, content: string): string {
  const normalized = input.target.split("\\").join("/");
  if (!templateTextIsSafe(input.target, MAX_TEMPLATE_PATH_BYTES, true)
    || [...input.target].some(isTemplateControl)
    || normalized !== input.target
    || normalized.startsWith("/")
    || normalized.includes("//")
    || normalized.includes(":")
    || normalized.split("/").some((part) => part === "" || part === "." || part === "..")
    || !normalized.toLowerCase().endsWith(".md")) {
    throw new Error("템플릿 저장 경로가 올바르지 않습니다");
  }
  if (!templateTextIsSafe(input.title, MAX_TEMPLATE_TITLE_BYTES, false)
    || [...input.title].some(isTemplateControl)) {
    throw new Error("템플릿 제목이 올바르지 않습니다");
  }
  if (!/^\d{4}-\d{2}-\d{2}$/u.test(input.date)) {
    throw new Error("템플릿 날짜가 올바르지 않습니다");
  }
  const date = new Date(`${input.date}T00:00:00Z`);
  if (Number(input.date.slice(0, 4)) < 1
    || Number.isNaN(date.getTime())
    || date.toISOString().slice(0, 10) !== input.date) {
    throw new Error("템플릿 날짜가 올바르지 않습니다");
  }
  if (!/^\d{2}:\d{2}$/u.test(input.time)
    || Number(input.time.slice(0, 2)) > 23
    || Number(input.time.slice(3, 5)) > 59) {
    throw new Error("템플릿 시간이 올바르지 않습니다");
  }
  validateTemplatePlaceholders(content);
  return renderTemplateContent(input, content);
}

function renderTemplateContent(input: TemplateApplyInput, content: string): string {
  const parts: string[] = [];
  let outputBytes = 0;
  let offset = 0;
  const append = (value: string) => {
    outputBytes += templateBytes(value);
    if (outputBytes > MAX_TEMPLATE_OUTPUT_BYTES) {
      throw new Error("템플릿 결과가 크기 제한을 초과했습니다");
    }
    parts.push(value);
  };
  while (true) {
    const start = content.indexOf("{{", offset);
    if (start < 0) {
      append(content.slice(offset));
      return parts.join("");
    }
    append(content.slice(offset, start));
    const end = content.indexOf("}}", start + 2);
    if (end < 0) throw new Error("지원하지 않는 템플릿 변수가 있습니다");
    const token = content.slice(start, end + 2);
    const value = token === "{{title}}" ? input.title
      : token === "{{date}}" ? input.date
      : token === "{{time}}" ? input.time
      : token === "{{vault-relative-path}}" ? input.target
      : null;
    if (value === null) throw new Error("지원하지 않는 템플릿 변수가 있습니다");
    append(value);
    offset = end + 2;
  }
}

let nextMockTemplateId = 2;
let mockTemplates: NoteTemplate[] = [
  {
    id: 1,
    name: "Daily note",
    content: "---\ntitle: {{title}}\ndate: {{date}}\n---\n\n# {{title}}\n\nCreated at {{time}} in {{vault-relative-path}}.\n",
    createdAtMs: Date.now(),
    updatedAtMs: Date.now(),
  },
];
let mockTemplatePreview: TemplatePreview | null = null;
let mockTemplatePreviewExpiresAtMs = 0;

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

export async function knowledgeWatcherStatus(): Promise<KnowledgeWatcherStatus> {
  if (!isTauri()) {
    return {
      sourceKind: "native",
      watchMode: "native",
      lastSyncedAt: Date.now(),
      error: null,
    };
  }
  return invoke<KnowledgeWatcherStatus>("knowledge_watcher_status");
}

export async function onKnowledgeWatcherStatus(
  cb: (status: KnowledgeWatcherStatus) => void,
): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<KnowledgeWatcherStatus>("knowledge-watcher-status", (event) => cb(event.payload));
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

/** Claim a native handoff for preview; no file is written at this stage. */
export async function previewKnowledgeDraft(
  id: string,
  kind: KnowledgeDraftPreview["kind"] = "knowledge-draft/v1",
): Promise<KnowledgeDraftPreview> {
  if (!isTauri()) throw new Error("Knowledge draft preview는 데스크톱 앱에서 사용할 수 없습니다");
  return invoke<KnowledgeDraftPreview>("preview_knowledge_draft", { id, kind });
}

/** Save a confirmed preview and acknowledge/delete the one-time handoff. */
export async function saveKnowledgeDraft(id: string): Promise<SaveKnowledgeDraftResult> {
  if (!isTauri()) throw new Error("Knowledge draft 저장은 데스크톱 앱에서 사용할 수 없습니다");
  return invoke<SaveKnowledgeDraftResult>("save_knowledge_draft", { id });
}

/** Restore a claimed draft without creating a note. */
export async function discardKnowledgeDraft(id: string): Promise<void> {
  if (!isTauri()) throw new Error("Knowledge draft 취소는 데스크톱 앱에서 사용할 수 없습니다");
  await invoke("discard_knowledge_draft", { id });
}

/** Keep a long-running preview within the bounded claim lease. */
export async function renewKnowledgeDraft(id: string): Promise<RenewKnowledgeDraftResult> {
  if (!isTauri()) throw new Error("Knowledge draft 갱신은 데스크톱 앱에서 사용할 수 없습니다");
  return invoke<RenewKnowledgeDraftResult>("renew_knowledge_draft", { id });
}

export async function listTemplates(): Promise<NoteTemplate[]> {
  if (!isTauri()) return mockTemplates.map((template) => ({ ...template }));
  return invoke<NoteTemplate[]>("list_templates");
}

export async function createTemplate(draft: TemplateDraft): Promise<NoteTemplate> {
  if (!isTauri()) {
    const normalized = { ...draft, name: draft.name.trim() };
    validateTemplateDraftInput(normalized);
    if (mockTemplates.length >= MAX_TEMPLATES) throw new Error("템플릿 개수가 제한을 초과했습니다");
    if (mockTemplates.some((template) => template.name.toLocaleLowerCase() === normalized.name.toLocaleLowerCase())) {
      throw new Error("템플릿 이름이 이미 있습니다");
    }
    const now = Date.now();
    const template = { id: nextMockTemplateId++, ...normalized, createdAtMs: now, updatedAtMs: now };
    mockTemplates = [...mockTemplates, template];
    return template;
  }
  return invoke<NoteTemplate>("create_template", { draft });
}

export async function updateTemplate(id: number, draft: TemplateDraft): Promise<NoteTemplate> {
  if (!isTauri()) {
    const normalized = { ...draft, name: draft.name.trim() };
    validateTemplateDraftInput(normalized);
    const index = mockTemplates.findIndex((template) => template.id === id);
    if (index < 0) throw new Error("템플릿을 찾을 수 없습니다");
    if (mockTemplates.some((template) => template.id !== id
      && template.name.toLocaleLowerCase() === normalized.name.toLocaleLowerCase())) {
      throw new Error("템플릿 이름이 이미 있습니다");
    }
    const template = { ...mockTemplates[index], ...normalized, updatedAtMs: Date.now() };
    mockTemplates = mockTemplates.map((item, itemIndex) => itemIndex === index ? template : item);
    return template;
  }
  return invoke<NoteTemplate>("update_template", { id, draft });
}

export async function deleteTemplate(id: number): Promise<void> {
  if (!isTauri()) {
    if (!mockTemplates.some((template) => template.id === id)) {
      throw new Error("템플릿을 찾을 수 없습니다");
    }
    mockTemplates = mockTemplates.filter((template) => template.id !== id);
    return;
  }
  await invoke("delete_template", { id });
}

export async function previewTemplate(input: TemplateApplyInput): Promise<TemplatePreview> {
  if (!isTauri()) {
    const template = mockTemplates.find((item) => item.id === input.templateId);
    if (!template) throw new Error("템플릿을 찾을 수 없습니다");
    const content = validateTemplateApplyInput(input, template.content);
    mockTemplatePreview = {
      previewId: `tpl-${Date.now()}`,
      templateId: input.templateId,
      templateUpdatedAtMs: template.updatedAtMs,
      target: input.target,
      content,
      byteLength: new TextEncoder().encode(content).byteLength,
    };
    mockTemplatePreviewExpiresAtMs = Date.now() + TEMPLATE_PREVIEW_TTL_MS;
    return mockTemplatePreview;
  }
  return invoke<TemplatePreview>("preview_template", { approval: input });
}

export async function saveTemplate(previewId: string): Promise<SaveTemplateResult> {
  if (!isTauri()) {
    if (!mockTemplatePreview || mockTemplatePreview.previewId !== previewId) {
      throw new Error("템플릿 미리보기가 없습니다");
    }
    if (Date.now() >= mockTemplatePreviewExpiresAtMs) {
      mockTemplatePreview = null;
      mockTemplatePreviewExpiresAtMs = 0;
      throw new Error("템플릿 미리보기가 오래되어 다시 확인하세요");
    }
    const template = mockTemplates.find((item) => item.id === mockTemplatePreview?.templateId);
    if (!template || template.updatedAtMs !== mockTemplatePreview.templateUpdatedAtMs) {
      mockTemplatePreview = null;
      mockTemplatePreviewExpiresAtMs = 0;
      throw new Error("템플릿 미리보기가 오래되어 다시 확인하세요");
    }
    // Browser mode deliberately has no vault filesystem.  Treat approval as
    // consumed preview state, not as a fabricated note creation.
    const result = { saved: false, path: mockTemplatePreview.target };
    mockTemplatePreview = null;
    mockTemplatePreviewExpiresAtMs = 0;
    return result;
  }
  return invoke<SaveTemplateResult>("save_template", { previewId });
}

export async function discardTemplatePreview(previewId: string): Promise<void> {
  if (!isTauri()) {
    if (mockTemplatePreview?.previewId === previewId) {
      mockTemplatePreview = null;
      mockTemplatePreviewExpiresAtMs = 0;
    }
    return;
  }
  await invoke("discard_template_preview", { previewId });
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
