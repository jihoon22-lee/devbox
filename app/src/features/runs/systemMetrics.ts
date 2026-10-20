import type { SystemInfo } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { usePageVisible } from "../../shell/visibility";

export function useSystemMetrics(view: "system" | "ports" | "all" = "all") {
  const visible = usePageVisible();
  const options = {
    enabled: visible,
    refetchInterval: visible ? 3000 : (false as const),
    refetchIntervalInBackground: true,
    staleTime: 0,
  };
  const system = useRpcQuery("observe.system", {}, { ...options, enabled: visible && view !== "ports" });
  const ports = useRpcQuery("observe.ports", {}, { ...options, enabled: visible && view !== "system" });
  return { system, ports };
}
export function gigabytes(bytes: number) {
  return Number((bytes / 1024 ** 3).toFixed(1));
}
export function memoryLabel(info: SystemInfo) {
  return `${gigabytes(Math.max(0, info.memoryTotalBytes - info.memoryAvailableBytes))}/${gigabytes(info.memoryTotalBytes)}GB`;
}
