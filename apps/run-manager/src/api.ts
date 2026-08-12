import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { RuntimeStatus } from "./types";

export function loadRuntimeStatus(): Promise<RuntimeStatus> {
  if (!isTauri()) {
    return Promise.resolve({
      backgroundLaunch: false,
      schedulerRunning: true,
      shutdownRequested: false,
      databasePath: "%LOCALAPPDATA%\\com.workbench.runmanager\\data.db",
    });
  }
  return invoke<RuntimeStatus>("runtime_status");
}

export function hideMainWindow(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("hide_main_window");
}

export function quitApp(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("quit_app");
}
