import type { ContainerStopHandoff } from "./ContainerStopHandoff";
import type { PortLogReply } from "./PortLogReply";
import type { PortManagerPreferences } from "./PortManagerPreferences";
import type { PortObservationSnapshot } from "./PortObservationSnapshot";
import type { PortRow } from "./PortRow";
import type { ProcessInfo } from "./ProcessInfo";
import type { JsonValue } from "./serde_json/JsonValue";

export type ProcessesResults = {
  apply_legacy_runtime_settings: JsonValue;
  get_process_info: ProcessInfo;
  handoff_container_stop: ContainerStopHandoff;
  list_port_observations: PortObservationSnapshot;
  list_ports: Array<PortRow>;
  load_port_manager_preferences: PortManagerPreferences;
  open_browser: null;
  open_port_log: PortLogReply;
  open_port_owner: null;
  preview_legacy_runtime_settings: JsonValue;
  reveal_process: null;
  save_port_manager_preferences: null;
};
