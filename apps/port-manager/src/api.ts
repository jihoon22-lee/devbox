import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type {
  ContainerStopHandoff,
  ListenerActionResult,
  ListenerKillRequest,
  LogStream,
  PortManagerPreferences,
  PortLogDispatch,
  PortObservationSnapshot,
  PortRow,
  ProcessInfo,
} from "./types";
import { DEFAULT_PREFERENCES } from "./refresh";

/** Tauri 없이 브라우저에서 UI를 미리 볼 수 있게 하는 샘플 데이터 */
const MOCK_RUN_ACTION_KEY = "port-action-" + "a".repeat(64);
const MOCK_WORKBENCH_ACTION_KEY = "port-action-" + "b".repeat(64);

const MOCK_PORTS: PortRow[] = [
  {
    proto: "TCP",
    local_addr: "0.0.0.0:3000",
    port: 3000,
    state: "LISTENING",
    pid: 18231,
    process_name: "node.exe",
    source: "windows",
    process_start_time: "638000000000000000",
    executable_path: "C:\\Program Files\\nodejs\\node.exe",
    command_line: "node server.js --port 3000",
    identity: { kind: "windows", pid: 18231, start_time: "638000000000000000" },
    correlations: [
      {
        source_app: "run-manager",
        target_kind: "task",
        target_id: "api-service",
        label: "API service",
        confidence: "verified",
        action_key: MOCK_RUN_ACTION_KEY,
        logs_available: true,
      },
    ],
  },
  {
    proto: "TCP",
    local_addr: "127.0.0.1:5173",
    port: 5173,
    state: "LISTENING",
    pid: 21324,
    process_name: "node.exe",
    source: "windows",
    process_start_time: "638000000100000000",
    executable_path: "C:\\Program Files\\nodejs\\node.exe",
    command_line: "node dev-server.js --port 5173",
    identity: { kind: "windows", pid: 21324, start_time: "638000000100000000" },
    correlations: [
      {
        source_app: "workbench",
        target_kind: "profile",
        target_id: "frontend-profile",
        label: "Frontend profile",
        confidence: "expected",
        action_key: MOCK_WORKBENCH_ACTION_KEY,
        logs_available: false,
      },
    ],
  },
  {
    proto: "TCP",
    local_addr: "0.0.0.0:5432",
    port: 5432,
    state: "LISTENING",
    pid: 9812,
    process_name: "postgres.exe",
    source: "windows",
    process_start_time: "638000000200000000",
    identity: { kind: "windows", pid: 9812, start_time: "638000000200000000" },
  },
  {
    proto: "TCP",
    local_addr: "0.0.0.0:8080",
    port: 8080,
    state: "LISTENING",
    pid: 12415,
    process_name: "java.exe",
    source: "windows",
    process_start_time: "638000000300000000",
    identity: { kind: "windows", pid: 12415, start_time: "638000000300000000" },
  },
  {
    proto: "TCP",
    local_addr: "10.0.0.5:50261",
    port: 50261,
    state: "ESTABLISHED",
    pid: 1044,
    process_name: "chrome.exe",
    source: "windows",
    process_start_time: "638000000400000000",
    identity: { kind: "windows", pid: 1044, start_time: "638000000400000000" },
  },
  {
    proto: "UDP",
    local_addr: "0.0.0.0:5353",
    port: 5353,
    state: "",
    pid: 1829,
    process_name: "svchost.exe",
    source: "windows",
    process_start_time: "638000000500000000",
    identity: { kind: "windows", pid: 1829, start_time: "638000000500000000" },
  },
];

const MOCK_SOURCES = [
  { producer: "run-manager", state: "available" as const, freshness_ms: 48 },
  { producer: "workbench", state: "available" as const, freshness_ms: 96 },
];

const MOCK_OBSERVATIONS: PortObservationSnapshot = {
  rows: MOCK_PORTS,
  sources: MOCK_SOURCES,
  correlations_truncated: false,
};

let mockPreferences: PortManagerPreferences = {
  ...DEFAULT_PREFERENCES,
  favorite_ports: [],
  favorite_processes: [],
};

export async function listPorts(): Promise<PortRow[]> {
  if (!isTauri()) {
    return MOCK_PORTS;
  }
  return invoke<PortRow[]>("list_ports");
}

export async function listPortObservations(): Promise<PortObservationSnapshot> {
  if (!isTauri()) {
    return {
      rows: MOCK_OBSERVATIONS.rows,
      sources: [...MOCK_OBSERVATIONS.sources],
      correlations_truncated: MOCK_OBSERVATIONS.correlations_truncated,
    };
  }
  return invoke<PortObservationSnapshot>("list_port_observations");
}

export async function loadPortManagerPreferences(): Promise<PortManagerPreferences> {
  if (!isTauri()) {
    return {
      ...mockPreferences,
      favorite_ports: [...mockPreferences.favorite_ports],
      favorite_processes: [...mockPreferences.favorite_processes],
    };
  }
  return invoke<PortManagerPreferences>("load_port_manager_preferences");
}

export async function savePortManagerPreferences(
  preferences: PortManagerPreferences,
): Promise<void> {
  if (!isTauri()) {
    mockPreferences = {
      ...preferences,
      favorite_ports: [...preferences.favorite_ports],
      favorite_processes: [...preferences.favorite_processes],
    };
    return;
  }
  await invoke("save_port_manager_preferences", { preferences });
}

export async function killListener(
  request: ListenerKillRequest,
): Promise<ListenerActionResult> {
  if (!isTauri()) {
    return { kind: "terminated" };
  }
  return invoke<ListenerActionResult>("kill_listener", { request });
}

export async function handoffContainerStop(
  request: ListenerKillRequest,
): Promise<ContainerStopHandoff> {
  if (!isTauri()) {
    if (request.identity.kind !== "container") {
      throw new Error("container handoff is unavailable");
    }
    return {
      target_app: "wsl-desktop",
      action: "stop-container",
      engine: request.identity.engine,
      container_id: request.identity.container_id,
      distro: request.identity.distro,
    };
  }
  return invoke<ContainerStopHandoff>("handoff_container_stop", { request });
}

export async function openBrowser(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank");
    return;
  }
  await invoke("open_browser", { url });
}

export async function getProcessInfo(pid: number): Promise<ProcessInfo> {
  if (!isTauri()) {
    const row = MOCK_PORTS.find((candidate) => candidate.pid === pid);
    if (!row) throw new Error("process information unavailable");
    return {
      pid,
      name: row.process_name ?? "",
      exe: `C:\\Program Files\\${row.process_name ?? "process.exe"}`,
      start_time: 0,
      memory_bytes: 0,
      command_line: row.command_line ?? null,
      executable_path: row.executable_path ?? null,
      process_start_time: row.process_start_time ?? null,
    };
  }
  return invoke<ProcessInfo>("get_process_info", { pid });
}

/** PID를 백엔드에서 다시 조회해 해당 실행 파일만 탐색기에 표시한다. */
export async function revealProcess(pid: number): Promise<void> {
  if (!isTauri()) return;
  await invoke("reveal_process", { pid });
}

export async function openPortOwner(actionKey: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_port_owner", { actionKey });
}

export async function openPortLog(
  actionKey: string,
  stream: LogStream,
): Promise<PortLogDispatch> {
  if (!isTauri()) {
    return { handoff_id: `mock-log-${stream}` };
  }
  return invoke<PortLogDispatch>("open_port_log", { actionKey, stream });
}
