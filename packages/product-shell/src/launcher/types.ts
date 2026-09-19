export interface SearchResult {
  id: string;
  revision: string;
  label: string;
  detail: string | null;
  source: string;
  targetApp: string;
  targetKind: string;
  stale: boolean;
  explicitPreview: boolean;
  favorite: boolean;
  recent: boolean;
  disabledReason?: string;
}

export interface SourceDiagnostic {
  producer: string;
  view: string;
  status: "fresh" | "stale" | "missing" | "corrupt" | "permission" | "linked";
}

export interface SearchResponse {
  results: SearchResult[];
  sources: SourceDiagnostic[];
}

export interface ShortcutConfig {
  accelerator: "Ctrl+Alt+Space" | "Ctrl+Alt+L" | "Ctrl+Alt+J";
  enabled: boolean;
  terminal?:boolean;capture?:boolean;project?:boolean;
}

export type ShortcutRegistration = "registered" | "unavailable" | "unsupported" | "disabled" | "pending";

export interface ShortcutStatus extends ShortcutConfig {
  issue?: string | null;
  registration: ShortcutRegistration;
  alternatives: ShortcutConfig["accelerator"][];
}

export interface LaunchResponse {
  status: "launched" | "installRequired" | "reviewPending";
  appId: string;
}
