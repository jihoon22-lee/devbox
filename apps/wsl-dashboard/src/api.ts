import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { ContainerInfo, DistroInfo, GitStatus } from "./types";

const MOCK_DISTROS: DistroInfo[] = [
  { name: "Ubuntu", version: 2, default: true },
  { name: "docker-desktop", version: 2, default: false },
];

const MOCK_CONTAINERS: ContainerInfo[] = [
  { id: "abc123", name: "postgres", image: "postgres:16", status: "Up 2 hours", ports: "0.0.0.0:5432->5432/tcp" },
  { id: "def456", name: "redis", image: "redis:7", status: "Up 2 hours", ports: "0.0.0.0:6379->6379/tcp" },
  { id: "ghi789", name: "nginx", image: "nginx:1.25", status: "Exited (0) 3 days ago", ports: "80/tcp" },
];

const MOCK_PROJECTS: GitStatus[] = [
  { path: "C:\\projects\\devbox", branch: "main", changes: 0, clean: true },
  { path: "C:\\projects\\FamilyCard", branch: "dev", changes: 3, clean: false },
];

export async function listDistros(): Promise<DistroInfo[]> {
  if (!isTauri()) return MOCK_DISTROS;
  return invoke<DistroInfo[]>("list_distros");
}

export async function dockerPs(distro: string): Promise<ContainerInfo[]> {
  if (!isTauri()) return MOCK_CONTAINERS;
  return invoke<ContainerInfo[]>("docker_ps", { distro });
}

export async function dockerAction(distro: string, containerId: string, action: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("docker_action", { distro, containerId, action });
}

export async function gitStatus(projects: string[]): Promise<GitStatus[]> {
  if (!isTauri()) return MOCK_PROJECTS;
  return invoke<GitStatus[]>("git_status", { projects });
}

export async function openTerminal(distro: string): Promise<void> {
  if (!isTauri()) {
    window.open("https://learn.microsoft.com/windows/wsl", "_blank");
    return;
  }
  await invoke("open_terminal", { distro });
}
