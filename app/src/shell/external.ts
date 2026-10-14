import { openUrl } from "@tauri-apps/plugin-opener";
/** Links from terminal output may only open ordinary web URLs. */
export async function openExternal(uri: string): Promise<void> {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return;
  if ("__TAURI_INTERNALS__" in window) await openUrl(url.href);
  else window.open(url.href, "_blank", "noopener,noreferrer");
}
