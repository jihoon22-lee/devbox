import type { AgentTask } from "./model";
/** These counters overlap differently by tool, so preserve their raw labels. */
export function TokenMetrics({ tokens, truncated }: { tokens: AgentTask["tokens"]; truncated: boolean }) {
  if (!tokens) return null;
  return (
    <>
      <span>입력 토큰 {tokens.input.toLocaleString("ko")}</span>
      <span>출력 토큰 {tokens.output.toLocaleString("ko")}</span>
      <span>캐시 읽기 토큰 {tokens.cacheRead.toLocaleString("ko")}</span>
      <span>캐시 쓰기 토큰 {tokens.cacheWrite.toLocaleString("ko")}</span>
      {truncated && <span>토큰 기록 일부</span>}
    </>
  );
}
