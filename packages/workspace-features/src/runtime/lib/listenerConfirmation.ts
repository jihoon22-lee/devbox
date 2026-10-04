import type { PortRow } from "../types";

export function listenerStopMessage(row: PortRow, productHosted: boolean): string {
  const processLabel = row.process_name ? " (" + row.process_name + ")" : "";
  const actionLabel =
    row.source === "container" ? (productHosted ? "컨테이너 중지" : "WSL Desktop에서 중지") : "리스너 종료";
  return row.local_addr + processLabel + " " + actionLabel + "할까요?";
}
