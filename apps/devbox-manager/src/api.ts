import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { InstalledApp, LatestRelease } from "./types";

const MOCK_LATEST: LatestRelease = {
  tag: "v0.1.1",
  published_at: "2026-08-11T00:00:00Z",
  assets: [
    { name: "port-manager.exe", url: "https://example.com/port-manager.exe" },
    { name: "PortManager_0.1.1_x64-setup.exe", url: "https://example.com/PortManager-setup.exe" },
    { name: "wsl-desktop.exe", url: "https://example.com/wsl-desktop.exe" },
    { name: "WSLDesktop_0.1.1_x64-setup.exe", url: "https://example.com/WSLDesktop-setup.exe" },
  ],
};

export async function latest(): Promise<LatestRelease> {
  if (!isTauri()) return MOCK_LATEST;
  return invoke<LatestRelease>("latest");
}

export async function installed(): Promise<InstalledApp[]> {
  if (!isTauri()) return [{ app: "port-manager", version: "0.1.0", mode: "portable", exe_path: "" }];
  return invoke<InstalledApp[]>("installed");
}

export async function installApp(name: string, version: string, url: string, mode: string): Promise<string> {
  if (!isTauri()) return `installed (${mode})`;
  return invoke<string>("install", { name, version, url, mode });
}

export async function launchApp(name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("launch", { name });
}
