import type { ResourceSummary } from "../types";

export type DashboardFreshness = "loading" | "refreshing" | "fresh" | "stale" | "error";

export function formatResourceBytes(bytes: number): string {
  if (!Number.isSafeInteger(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = "B";
  for (const nextUnit of units) {
    value /= 1024;
    unit = nextUnit;
    if (value < 1024 || nextUnit === units[units.length - 1]) break;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}

export function formatResourcePair(used: number, total: number): string {
  if (!Number.isSafeInteger(used) || !Number.isSafeInteger(total) || total <= 0 || used < 0 || used > total) {
    return "—";
  }
  return `${formatResourceBytes(used)} / ${formatResourceBytes(total)}`;
}

export function resourceSummaryLabel(resource: ResourceSummary | null | undefined): string {
  if (!resource) return "리소스 조회 안 함";
  const cpu = Number.isInteger(resource.cpuPercent)
    && resource.cpuPercent !== null
    && resource.cpuPercent >= 0
    && resource.cpuPercent <= 100
    ? `${resource.cpuPercent}%`
    : "—";
  return `CPU ${cpu} · 메모리 ${formatResourcePair(resource.memoryUsedBytes, resource.memoryTotalBytes)} · 디스크 ${formatResourcePair(resource.diskUsedBytes, resource.diskTotalBytes)}`;
}
