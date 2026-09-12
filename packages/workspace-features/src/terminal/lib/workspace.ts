import type {
  Layout,
  MultiplexerKind,
  Pane,
  PaneSizing,
  Tab,
  WorkspaceDefinition,
  WorkspacePaneDefinition,
  WorkspaceProfile,
  WorkspaceTabDefinition,
} from "../types";
import { normalizePaneSizing, paneTrackCounts } from "./paneSizing";

const LAST_LAYOUT_KEY = "wsl-desktop:last-layout";
const LAYOUT_VERSION = 2;
export const MAX_WORKSPACE_TABS = 16;
export const MAX_WORKSPACE_PANES = 32;
export const MAX_START_COMMAND_CHARACTERS = 4096;
const MAX_ID_CHARACTERS = 128;
const MAX_NAME_BYTES = 120;
const MAX_PATH_BYTES = 4096;

interface PersistedLayout extends WorkspaceDefinition {
  version: 2;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validId(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value.length <= MAX_ID_CHARACTERS
    && /^[A-Za-z0-9_-]+$/u.test(value);
}

function validName(value: unknown): value is string {
  return typeof value === "string"
    && value.trim().length > 0
    && new TextEncoder().encode(value).length <= MAX_NAME_BYTES
    && !/[\u0000-\u001f\u007f]/u.test(value);
}

function validLayout(value: unknown): value is Layout {
  return value === "grid" || value === "cols" || value === "rows";
}

function validMultiplexer(value: unknown): value is MultiplexerKind {
  return value === "native" || value === "tmux" || value === "zellij";
}

/** Backend의 parse_safe_project_path와 같은 보수적 절대 경로 경계. */
export function isSafeWorkspacePath(value: string): boolean {
  const path = value.trim();
  if (!path || new TextEncoder().encode(path).length > MAX_PATH_BYTES || /[\u0000-\u001f\u007f]/u.test(path)) return false;
  if (/^(?:\\\\[?.]\\|\/\/[?.]\/)/u.test(path)) return false;

  let parts: string[];
  let windows = false;
  if (/^[A-Za-z]:[\\/]/u.test(path)) {
    windows = true;
    parts = path.slice(3).split(/[\\/]+/u).filter(Boolean);
    if (parts.length < 1) return false;
  } else if (/^(?:\\\\|\/\/)/u.test(path)) {
    windows = true;
    parts = path.slice(2).split(/[\\/]+/u).filter(Boolean);
    if (parts.length < 3) return false;
  } else if (path.startsWith("/")) {
    parts = path.slice(1).split("/").filter(Boolean);
    if (parts.length < 1) return false;
  } else {
    return false;
  }

  if (parts.some((part) => part === "." || part === "..")) return false;
  if (!windows) return true;
  return parts.every((part) => {
    if (/[<>:"|?*]/u.test(part) || /[ .]$/u.test(part)) return false;
    const stem = part.split(".")[0]?.toUpperCase() ?? "";
    return !/^(?:CON|PRN|AUX|NUL|CLOCK\$|CONIN\$|CONOUT\$|COM[1-9]|LPT[1-9])$/u.test(stem);
  });
}

function credentialReference(value: string): boolean {
  const candidate = value.trim().replace(/^['"]|['"]$/gu, "");
  return candidate.startsWith("$")
    || (/^%[^%]+%$/u.test(candidate))
    || (/^\{\{[^{}]+\}\}$/u.test(candidate));
}

export function startCommandError(command: string): string | null {
  const value = command.trim();
  if (!value || value.length > MAX_START_COMMAND_CHARACTERS || /[\u0000-\u001f\u007f]/u.test(value)) {
    return "시작 명령은 4,096자 이하의 한 줄이어야 합니다.";
  }
  const lower = value.toLowerCase();
  if (/-----begin [^-]{0,40}private key-----|(?:^|[\s'"=:])(?:sk-|ghp_|github_pat_|xox[bp]-)[a-z0-9_-]{12,}/u.test(lower)) {
    return "시작 명령에 평문 자격증명을 저장할 수 없습니다.";
  }
  const markers = value.matchAll(/(authorization:\s*bearer\s+|--password(?:=|\s+)|--token(?:=|\s+)|api_?key=|client_secret=|access_token=)/giu);
  for (const marker of markers) {
    const candidate = value.slice((marker.index ?? 0) + marker[0].length).trimStart().split(/\s/u)[0] ?? "";
    if (candidate && !credentialReference(candidate)) {
      return "시작 명령에 평문 자격증명을 저장할 수 없습니다.";
    }
  }
  return null;
}

function normalizePane(value: unknown): WorkspacePaneDefinition | null {
  if (!isRecord(value) || !validId(value.key) || !validName(value.distro)) return null;
  const cwd = value.cwd === null || value.cwd === undefined ? null : value.cwd;
  const startCommand = value.startCommand === null || value.startCommand === undefined ? null : value.startCommand;
  const multiplexer = value.multiplexer ?? "native";
  if ((cwd !== null && (typeof cwd !== "string" || !isSafeWorkspacePath(cwd)))
    || (startCommand !== null && (typeof startCommand !== "string" || startCommandError(startCommand) !== null))
    || !validMultiplexer(multiplexer)) return null;
  return {
    key: value.key,
    distro: value.distro.trim(),
    cwd: cwd === null ? null : cwd.trim(),
    startCommand: startCommand === null ? null : startCommand.trim(),
    multiplexer,
  };
}

function normalizeTab(value: unknown): WorkspaceTabDefinition | null {
  if (!isRecord(value)
    || !validId(value.id)
    || !validName(value.title)
    || !validLayout(value.layout)
    || !Array.isArray(value.paneKeys)
    || value.paneKeys.length === 0
    || value.paneKeys.length > MAX_WORKSPACE_PANES
    || !value.paneKeys.every(validId)) return null;
  const paneKeys = [...value.paneKeys];
  if (new Set(paneKeys).size !== paneKeys.length) return null;
  const tracks = paneTrackCounts(value.layout, paneKeys.length);
  let sizing: PaneSizing;
  if (value.sizing === undefined) {
    // version 1 local layouts and profile stores did not persist split ratios.
    sizing = normalizePaneSizing(undefined, value.layout, paneKeys.length);
  } else {
    if (!isRecord(value.sizing)
      || !Array.isArray(value.sizing.columns)
      || !Array.isArray(value.sizing.rows)
      || value.sizing.columns.length !== tracks.columns
      || value.sizing.rows.length !== tracks.rows
      || ![...value.sizing.columns, ...value.sizing.rows].every(
        (fraction) => typeof fraction === "number" && Number.isFinite(fraction) && fraction > 0,
      )) return null;
    sizing = normalizePaneSizing({
      columns: value.sizing.columns,
      rows: value.sizing.rows,
    }, value.layout, paneKeys.length);
  }
  return {
    id: value.id,
    title: value.title.trim(),
    customTitle: value.customTitle === true,
    layout: value.layout,
    paneKeys,
    sizing,
  };
}

export function normalizeWorkspace(value: unknown): WorkspaceDefinition | null {
  if (!isRecord(value)
    || !Array.isArray(value.tabs)
    || !Array.isArray(value.panes)
    || value.tabs.length === 0
    || value.tabs.length > MAX_WORKSPACE_TABS
    || value.panes.length === 0
    || value.panes.length > MAX_WORKSPACE_PANES
    || !validId(value.activeTabId)) return null;
  const tabs = value.tabs.map(normalizeTab);
  const panes = value.panes.map(normalizePane);
  if (tabs.some((tab) => tab === null) || panes.some((pane) => pane === null)) return null;
  const normalizedTabs = tabs as WorkspaceTabDefinition[];
  const normalizedPanes = panes as WorkspacePaneDefinition[];
  const tabIds = new Set(normalizedTabs.map((tab) => tab.id));
  const paneKeys = new Set(normalizedPanes.map((pane) => pane.key));
  if (tabIds.size !== normalizedTabs.length || paneKeys.size !== normalizedPanes.length) return null;
  const references = normalizedTabs.flatMap((tab) => tab.paneKeys);
  if (references.length !== normalizedPanes.length
    || new Set(references).size !== references.length
    || references.some((key) => !paneKeys.has(key))
    || !tabIds.has(value.activeTabId)) return null;
  const activePaneKey = value.activePaneKey === null || value.activePaneKey === undefined
    ? null
    : value.activePaneKey;
  if (activePaneKey !== null && (!validId(activePaneKey) || !paneKeys.has(activePaneKey))) return null;
  const activeTab = normalizedTabs.find((tab) => tab.id === value.activeTabId);
  if (!activeTab || (activePaneKey !== null && !activeTab.paneKeys.includes(activePaneKey))) return null;
  return {
    tabs: normalizedTabs,
    panes: normalizedPanes,
    activeTabId: value.activeTabId,
    activePaneKey,
  };
}

export function normalizeProfile(value: unknown): WorkspaceProfile | null {
  if (!isRecord(value) || !validId(value.id) || !validName(value.name)) return null;
  const workspace = normalizeWorkspace(value);
  return workspace ? { id: value.id, name: value.name.trim(), ...workspace } : null;
}

export function workspaceFromRuntime(
  tabs: readonly Tab[],
  panes: readonly Pane[],
  activeTabId: string,
  activePaneId: string | null,
): WorkspaceDefinition | null {
  if (tabs.length === 0 || panes.length === 0) return null;
  const identityToKey = new Map(
    panes.map((pane) => [pane.sessionId ?? pane.key, pane.key] as const),
  );
  const definition: WorkspaceDefinition = {
    tabs: tabs.map((tab) => ({
      id: tab.id,
      title: tab.title,
      customTitle: tab.customTitle === true,
      layout: tab.layout,
      paneKeys: tab.paneIds.map((id) => identityToKey.get(id)).filter((key): key is string => Boolean(key)),
      sizing: normalizePaneSizing(tab.sizing, tab.layout, tab.paneIds.length),
    })).filter((tab) => tab.paneKeys.length > 0),
    panes: panes.map((pane) => ({
      key: pane.key,
      distro: pane.distro,
      cwd: pane.cwd && isSafeWorkspacePath(pane.cwd) ? pane.cwd : null,
      startCommand: pane.startCommand ?? null,
      multiplexer: pane.requestedMultiplexer ?? pane.multiplexer,
    })),
    activeTabId,
    activePaneKey: activePaneId ? (identityToKey.get(activePaneId) ?? null) : null,
  };
  return normalizeWorkspace(definition);
}

export function loadLastWorkspace(): WorkspaceDefinition | null {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(LAST_LAYOUT_KEY) ?? "null");
    if (!isRecord(value) || (value.version !== 1 && value.version !== LAYOUT_VERSION)) return null;
    return normalizeWorkspace(value);
  } catch {
    return null;
  }
}

export function saveLastWorkspace(workspace: WorkspaceDefinition | null): void {
  const normalized = workspace ? normalizeWorkspace(workspace) : null;
  if (!normalized) {
    localStorage.removeItem(LAST_LAYOUT_KEY);
    return;
  }
  const persisted: PersistedLayout = { version: LAYOUT_VERSION, ...normalized };
  localStorage.setItem(LAST_LAYOUT_KEY, JSON.stringify(persisted));
}
