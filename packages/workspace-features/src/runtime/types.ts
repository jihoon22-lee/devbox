export interface PortRow {
  proto: string;
  local_addr: string;
  port: number;
  state: string;
  pid: number | null;
  process_name: string | null;
  source?: ListenerSource;
  command_line?: string | null;
  executable_path?: string | null;
  /** Decimal native process identity; never a JS number (Windows FILETIME). */
  process_start_time?: string | null;
  wsl_distro?: string | null;
  wsl_start_tick?: number | null;
  container_engine?: string | null;
  container_id?: string | null;
  container_name?: string | null;
  identity?: ListenerIdentity | null;
  /** Read-only app ownership hints attached to an observation snapshot. */
  correlations?: PortCorrelation[];
}

export type CorrelationConfidence = "verified" | "declared" | "expected";

export interface PortCorrelation {
  source_app: string;
  target_kind: string;
  target_id: string;
  label: string;
  confidence: CorrelationConfidence;
  /** Opaque key revalidated by the native action command. */
  action_key: string;
  logs_available: boolean;
}

export type SnapshotSourceState = "available" | "missing" | "invalid" | "stale";

export interface SnapshotSourceStatus {
  producer: string;
  state: SnapshotSourceState;
  freshness_ms: number | null;
}

export interface PortObservationSnapshot {
  rows: PortRow[];
  sources: SnapshotSourceStatus[];
  correlations_truncated: boolean;
}

export type LogStream = "stdout" | "stderr";

export interface LogLensDispatch {
  handoff_id: string;
}

export type PortLogDispatch = LogLensDispatch;

export interface ProcessInfo {
  pid: number;
  name: string;
  exe: string | null;
  start_time: number;
  memory_bytes: number;
  command_line?: string | null;
  executable_path?: string | null;
  /** Exact native process identity, serialized as decimal text. */
  process_start_time?: string | null;
}

export type ProtoFilter = "all" | "tcp" | "udp";
export type StateFilter = "all" | "listening" | "established";

export type ListenerSource = "windows" | "wsl" | "container";

export type ListenerIdentity =
  | { kind: "windows"; pid: number; start_time: string }
  | { kind: "wsl"; distro: string; pid: number; start_tick: number }
  | { kind: "container"; engine: string; container_id: string; distro: string };

export interface ListenerKillRequest {
  endpoint: {
    proto: string;
    local_addr: string;
    port: number;
    state: string;
  };
  identity: ListenerIdentity;
}

export interface ContainerStopHandoff {
  target_app: string;
  action: string;
  engine: string;
  container_id: string;
  distro: string;
}

export type ListenerActionResult =
  | { kind: "terminated" }
  | { kind: "handoff"; handoff: ContainerStopHandoff };

export interface PortFavorite {
  source: ListenerSource;
  proto: string;
  local_addr: string;
  port: number;
}

export interface ProcessFavorite {
  source: ListenerSource;
  identity: ListenerIdentity;
}

/** Native-owned, bounded view state. It deliberately has no path or command fields. */
export interface PortManagerPreferences {
  schema_version: 1;
  refresh_interval_ms: number;
  pinned_only: boolean;
  favorite_ports: PortFavorite[];
  favorite_processes: ProcessFavorite[];
}
