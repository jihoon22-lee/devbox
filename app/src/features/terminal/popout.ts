export type PopoutKind = "terminal" | "agent" | "agents-grid" | "note" | "capture";
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
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const previous = await WebviewWindow.getByLabel(label);
    if (previous) {
      await previous.setFocus();
      return;
    }
    const popup = new WebviewWindow(label, {
      url: path,
      title: "devbox",
      width: 1000,
      height: 700,
      minWidth: 480,
      minHeight: 320,
    });
    await new Promise<void>((resolve, reject) => {
      let done = false;
      const unlisten: (() => void)[] = [];
      const finish = (error?: unknown) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        unlisten.forEach((stop) => stop());
        if (error) reject(error);
        else resolve();
      };
      const timer = setTimeout(() => finish(new Error("Popout creation timed out")), 15_000);
      const listen = (event: "tauri://created" | "tauri://error") => {
        void popup
          .once(event, (message) => finish(event === "tauri://error" ? new Error(String(message.payload)) : undefined))
          .then((stop) => {
            if (done) stop();
            else unlisten.push(stop);
          })
          .catch(finish);
      };
      listen("tauri://created");
      listen("tauri://error");
    });
    await popup.setFocus();
  })();
  pending.set(label, operation);
  void operation
    .finally(() => {
      if (pending.get(label) === operation) pending.delete(label);
    })
    .catch(() => {});
  return operation;
}
