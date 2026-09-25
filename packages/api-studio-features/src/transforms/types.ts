export type RegexMatch = import("../generated/RegexMatch").RegexMatch;

export type DiffHunk = import("../generated/DiffHunk").DiffHunk;

export interface ApiHandoffDispatch {
  handoffId: string;
  producerId: string;
  consumerId: string;
  createdAtMs: number;
  expiresAtMs: number;
}

/** Native result for the explicit Developer Toolbox → Knowledge publisher. */
export interface KnowledgeDraftHandoffDispatch {
  handoffId: string;
  redacted: boolean;
}

/** Short alias matching the native command's Knowledge draft terminology. */
export type KnowledgeDraftDispatch = KnowledgeDraftHandoffDispatch;

/**
 * The native AppLink envelope is deliberately the only value carried by the
 * open event.  Text payloads stay in the one-time handoff store and are read
 * only after the receiver has shown an explicit preview.
 */
export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string; filter?: unknown }
  | { kind: "task"; id: string }
  | { kind: "install"; appId: string }
  | { kind: "handoff"; handoffKind: string; id: string };

export type OpenRequest = import("../generated/OpenRequest").OpenRequest;

/** Safe, bounded projection returned by `preview_toolbox_text`. */
export interface ToolboxTextHandoffPreview {
  handoffId: string;
  producerId: string;
  expiresAtMs: number;
  text: string;
  redacted: boolean;
}

/** Native renewal returns the updated process-local claim lease only. */
export interface ToolboxTextRenewResult {
  leaseUntilMs: number;
}

/** Short alias for callers that refer to the preview by its native name. */
export type ToolboxTextPreview = ToolboxTextHandoffPreview;
