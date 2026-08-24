export interface PortRow {
  proto: string;
  local_addr: string;
  port: number;
  state: string;
  pid: number | null;
  process_name: string | null;
}

export interface ProcessInfo {
  pid: number;
  name: string;
  exe: string | null;
  start_time: number;
  memory_bytes: number;
}

export type ProtoFilter = "all" | "tcp" | "udp";
export type StateFilter = "all" | "listening" | "established";
