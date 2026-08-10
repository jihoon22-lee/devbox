import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { PortRow } from "./types";

/** Tauri 없이 브라우저에서 UI를 미리 볼 수 있게 하는 샘플 데이터 */
const MOCK_PORTS: PortRow[] = [
  { proto: "TCP", local_addr: "0.0.0.0:3000", port: 3000, state: "LISTENING", pid: 18231, process_name: "node.exe" },
  { proto: "TCP", local_addr: "127.0.0.1:5173", port: 5173, state: "LISTENING", pid: 21324, process_name: "node.exe" },
  { proto: "TCP", local_addr: "0.0.0.0:5432", port: 5432, state: "LISTENING", pid: 9812, process_name: "postgres.exe" },
  { proto: "TCP", local_addr: "0.0.0.0:8080", port: 8080, state: "LISTENING", pid: 12415, process_name: "java.exe" },
  { proto: "TCP", local_addr: "10.0.0.5:50261", port: 50261, state: "ESTABLISHED", pid: 1044, process_name: "chrome.exe" },
  { proto: "UDP", local_addr: "0.0.0.0:5353", port: 5353, state: "", pid: 1829, process_name: "svchost.exe" },
];

export async function listPorts(): Promise<PortRow[]> {
  if (!isTauri()) {
    return MOCK_PORTS;
  }
  return invoke<PortRow[]>("list_ports");
}

export async function killProcess(pid: number): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await invoke("kill_process", { pid });
}

export async function openBrowser(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank");
    return;
  }
  await invoke("open_browser", { url });
}
