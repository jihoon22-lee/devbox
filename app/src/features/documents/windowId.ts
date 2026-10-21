import { getCurrentWindow } from "@tauri-apps/api/window";
import { isNative } from "../settings/nativePreferences";

/** The lease owner name: the Tauri window label, or the browser window name. */
export function windowId(): string {
  if (isNative()) return getCurrentWindow().label;
  return window.name || "main";
}
