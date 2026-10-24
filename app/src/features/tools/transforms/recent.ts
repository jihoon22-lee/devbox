import { CATALOG, type ToolId } from "./catalog";

const KEY = "devbox.tools.recent";
const ids = new Set<string>(CATALOG.map((tool) => tool.id));
export function recentTools(): ToolId[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    if (!Array.isArray(value)) return [];
    return [...new Set(value.filter((v): v is ToolId => typeof v === "string" && ids.has(v)))].slice(0, 8);
  } catch {
    return [];
  }
}
/** Only identifiers are accepted; there is no payload/output persistence API. */
export function rememberTool(id: string): ToolId[] {
  if (!ids.has(id)) return recentTools();
  const next = [id as ToolId, ...recentTools().filter((value) => value !== id)].slice(0, 8);
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    /* A blocked browser store does not disable local tools. */
  }
  return next;
}
