import { invoke } from "@tauri-apps/api/core";
import { readText as nativeReadText } from "@tauri-apps/plugin-clipboard-manager";
import catalogJson from "../../catalog.json";
import { isTauri } from "./lib/isTauri";
import type { LaunchResponse, SearchResponse, SearchResult, ShortcutConfig, ShortcutStatus } from "./types";

export const CLIPBOARD_PREVIEW_ID = "builtin/clipboard-preview";
const MOCK_REVISION = "0".repeat(64);

const MOCK_RESULTS: SearchResult[] = catalogJson.apps.filter((app) => app.managerVisible && app.id !== "devbox-launcher").flatMap((app) => [
  {
    id: `catalog/app/${app.id}`,
    revision: MOCK_REVISION,
    label: app.displayName,
    detail: "Devbox 앱",
    source: "catalog",
    targetApp: app.id,
    targetKind: "app",
    stale: false,
    explicitPreview: false,
    favorite: false,
    recent: false,
  },
]);
MOCK_RESULTS.push({
  id: CLIPBOARD_PREVIEW_ID,
  revision: MOCK_REVISION,
  label: "Clipboard 미리보기",
  detail: "현재 선택 영역, 없으면 clipboard · 전달하지 않음",
  source: "launcher",
  targetApp: "devbox-launcher",
  targetKind: "clipboard-preview",
  stale: false,
  explicitPreview: true,
  favorite: false,
  recent: false,
});

export function search(query: string): Promise<SearchResponse> {
  if (!isTauri()) {
    const needle = query.trim().toLocaleLowerCase();
    return Promise.resolve({
      results: MOCK_RESULTS.filter((result) => !needle || `${result.label} ${result.detail ?? ""}`.toLocaleLowerCase().includes(needle)).slice(0, 256),
      sources: [
        { producer: "workbench", view: "profiles", status: "missing" },
        { producer: "repo-manager", view: "repositories", status: "missing" },
        { producer: "run-manager", view: "jobs-services", status: "missing" },
        { producer: "everything-plus", view: "saved-queries", status: "missing" },
        { producer: "wsl-desktop", view: "profiles", status: "missing" },
      ],
    });
  }
  return invoke<SearchResponse>("search", { request: { query } });
}

export function launchResult(result: SearchResult, allowStale = false): Promise<LaunchResponse> {
  if (!isTauri()) return Promise.resolve({ status: "launched", appId: result.targetApp });
  return invoke<LaunchResponse>("launch_result", {
    request: {
      resultId: result.id,
      expectedRevision: result.revision,
      allowStale,
    },
  });
}

export function previewTextAction(result: SearchResult): Promise<{ actionId: string; kind: string; maxBytes: number }> {
  if (!isTauri()) {
    if (result.id !== CLIPBOARD_PREVIEW_ID) return Promise.reject(new Error("unsupported preview"));
    return Promise.resolve({ actionId: result.id, kind: "clipboard-preview/v1", maxBytes: 64 * 1024 });
  }
  return invoke("preview_text_action", {
    request: {
      actionId: result.id,
      expectedRevision: result.revision,
    },
  });
}

export function performTextAction(result: SearchResult, text: string): Promise<LaunchResponse> {
  if (!isTauri()) return Promise.reject(new Error("text handoff receiver unavailable"));
  return invoke<LaunchResponse>("perform_text_action", {
    request: {
      actionId: result.id,
      expectedRevision: result.revision,
      text,
    },
  });
}

export function setFavorite(result: SearchResult, favorite: boolean): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("set_favorite", {
    request: {
      resultId: result.id,
      expectedRevision: result.revision,
      favorite,
    },
  });
}

export function clearRecents(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("clear_recents");
}

export async function readCurrentText(): Promise<string> {
  // Selection is intentionally sampled only at the explicit action boundary.
  const selected = typeof window !== "undefined" ? window.getSelection()?.toString() ?? "" : "";
  if (selected.trim()) return selected;
  if (isTauri()) return nativeReadText();
  if (typeof navigator !== "undefined" && navigator.clipboard) return navigator.clipboard.readText();
  return "";
}

export function getShortcut(): Promise<ShortcutStatus> {
  if (!isTauri()) return Promise.resolve({ accelerator: "Ctrl+Alt+Space", enabled: true, registration: "unsupported", alternatives: ["Ctrl+Alt+L", "Ctrl+Alt+J"] });
  return invoke<ShortcutStatus>("shortcut_config");
}

export function setShortcut(config: ShortcutConfig): Promise<ShortcutStatus> {
  if (!isTauri()) return Promise.resolve({ ...config, registration: "unsupported", alternatives: ["Ctrl+Alt+Space", "Ctrl+Alt+L", "Ctrl+Alt+J"].filter((shortcut) => shortcut !== config.accelerator) as ShortcutConfig["accelerator"][] });
  return invoke<ShortcutStatus>("set_shortcut", { config });
}
