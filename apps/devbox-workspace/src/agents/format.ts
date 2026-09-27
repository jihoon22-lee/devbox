export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}
export function formatCpu(percent: number | null): string {
  return percent === null ? "측정 중" : `${percent}%`;
}
export function formatTokens(tokens: number): string {
  return tokens >= 10_000 ? `${Math.round(tokens / 10_000)}만` : String(tokens);
}
