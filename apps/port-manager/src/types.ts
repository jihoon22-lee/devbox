export interface PortRow {
  proto: string;
  local_addr: string;
  port: number;
  state: string;
  pid: number | null;
  process_name: string | null;
}

export type ProtoFilter = "all" | "tcp" | "udp";
export type StateFilter = "all" | "listening" | "established";
