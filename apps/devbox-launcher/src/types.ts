export interface SearchResult {
  id: string;
  label: string;
  detail: string | null;
  source: string;
  targetApp: string;
  targetKind: string;
  stale: boolean;
  explicitPreview: boolean;
}

export interface SourceDiagnostic {
  producer: string;
  view: string;
  status: "fresh" | "stale" | "missing" | "corrupt" | "permission";
}

export interface SearchResponse {
  results: SearchResult[];
  sources: SourceDiagnostic[];
}

export interface ShortcutConfig {
  accelerator: "Ctrl+Alt+Space" | "Ctrl+Alt+L" | "Ctrl+Alt+J";
  enabled: boolean;
}

export type ShortcutRegistration = "registered" | "unavailable" | "unsupported" | "disabled" | "pending";

export interface ShortcutStatus extends ShortcutConfig {
  registration: ShortcutRegistration;
  alternatives: ShortcutConfig["accelerator"][];
}

export interface LaunchResponse {
  status: "launched" | "installRequired";
  appId: string;
}
