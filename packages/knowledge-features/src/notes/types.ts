export type TreeEntry = import("../generated/TreeEntry").TreeEntry;

export type KnowledgeWatcherStatus = import("../generated/KnowledgeWatcherStatus").KnowledgeWatcherStatus;

export interface SearchResult {
  path: string;
  title: string;
}

/** `render_markdown` 커맨드의 응답. Rust `RenderedDoc`과 1:1. */
export type RenderedDoc = import("../generated/RenderedDoc").RenderedDoc;

export type WikilinkStatus = import("../generated/WikilinkOccurrence").WikilinkOccurrence["status"];

export type WikilinkOccurrence = import("../generated/WikilinkOccurrence").WikilinkOccurrence;

export type WikilinkCandidate = import("../generated/WikilinkCandidate").WikilinkCandidate;

export type Backlink = import("../generated/Backlink").Backlink;

export type ImageAsset = import("../generated/SavedImageAsset").SavedImageAsset;

export interface EditorCursorRequest {
  line: number;
  column: number;
  token: number;
}

export type QuickCaptureInput = import("../generated/QuickCaptureInput").QuickCaptureInput;

export type QuickCapturePreview = import("../generated/QuickCapturePreview").QuickCapturePreview;

export type QuickCaptureSaved = import("../generated/QuickCaptureSaved").QuickCaptureSaved;

export type QuickCaptureShortcutState = import("../generated/ShortcutRegistration").ShortcutRegistration;

export type QuickCaptureShortcutStatus = import("../generated/ShortcutStatus").ShortcutStatus;
