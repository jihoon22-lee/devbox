import { invoke } from "@tauri-apps/api/core";

export type PopoutKind = "terminal" | "agent" | "agents-grid" | "note" | "code" | "capture";
const pending = new Map<string, Promise<void>>();
const browserWindows = new Map<string, Window>();
// Hex-encode UTF-8 rather than replacing punctuation, which would collide for IDs.
export function popoutLabel(kind: PopoutKind, id?: string) {
  const encoded =
    id === undefined
      ? "main"
      : Array.from(new TextEncoder().encode(id), (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `popout-${kind}-${encoded}`;
}
export function openPopout(kind: PopoutKind, id?: string): Promise<void> {
  const label = popoutLabel(kind, id);
  const existing = pending.get(label);
  if (existing) return existing;
  const query = new URLSearchParams(location.search);
  const search = new URLSearchParams();
  const gateway = query.get("gateway");
  if (gateway) search.set("gateway", gateway);
  if (query.get("renderer") === "dom") search.set("renderer", "dom");
  const path = `/popout/${kind}${id === undefined ? "" : `/${encodeURIComponent(id)}`}${search.size ? `?${search}` : ""}`;
  const operation = (async () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      const previous = browserWindows.get(label);
      const popup = previous && !previous.closed ? previous : window.open(path, label, "width=1000,height=700");
      if (!popup) throw new Error("Popout blocked");
      browserWindows.set(label, popup);
      popup.focus();
      return;
    }
    try {
      const created = await invoke<unknown>("popout_create", { kind, id: id ?? null });
      if (created !== label) throw new Error("Unexpected popout identity");
    } catch {
      throw new Error("새 창을 열지 못했습니다. 앱 상태를 확인하고 다시 시도하세요.");
    }
  })();
  pending.set(label, operation);
  void operation
    .finally(() => {
      if (pending.get(label) === operation) pending.delete(label);
    })
    .catch(() => {});
  return operation;
}
