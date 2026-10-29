import { invoke } from "@tauri-apps/api/core";
export interface NativePreferences {
  autostartAsked: boolean;
  autostart: boolean;
  trayNoticeShown: boolean;
}
export interface ShortcutStatus {
  palette: string;
  capture: string;
  captureError: string | null;
  bottomTerminal: string | null;
  paletteError: string | null;
  bottomTerminalError: string | null;
}
export const isNative = () => "__TAURI_INTERNALS__" in window;
export const appPreferences = () => invoke<NativePreferences>("app_preferences");
export const setAutostart = (enabled: boolean, firstRun = false) =>
  invoke<NativePreferences>("set_autostart", { enabled, firstRun });
/** [나중에] on the first-run dialog: saves that the question was asked and changes nothing about autostart. */
export const markAutostartAsked = () => invoke<void>("mark_autostart_asked");
export const shortcutStatus = () => invoke<ShortcutStatus>("shortcut_status");
export const configureShortcuts = (palette: string, bottomTerminal: string | null, capture: string) =>
  invoke<ShortcutStatus>("configure_shortcuts", { palette, bottomTerminal, capture });
