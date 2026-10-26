import { useNavigate } from "@tanstack/react-router";
import { useAgentSummary } from "../features/agents/AgentData";
import { memoryLabel, useSystemMetrics } from "../features/runs/systemMetrics";
import { UpdateStatus } from "../features/settings/Update";
import { GitStatus } from "../features/source/GitStatus";
import { useConnection } from "../rpc/connection";
import { useRpcQuery, useTopicInvalidation } from "../rpc/query";
import { StatusDot } from "../ui";
import { usePageVisible } from "./visibility";

const LABEL = {
  connecting: "연결 중",
  ready: "데몬 연결됨",
  reconnecting: "다시 연결 중",
  versionMismatch: "버전 불일치",
} as const;
const TONE = { connecting: "progress", ready: "ok", reconnecting: "warn", versionMismatch: "danger" } as const;

export function StatusBar() {
  const agents = useAgentSummary();
  const visible = usePageVisible();
  const summary = useRpcQuery("runtime.summary", {}, { enabled: visible, staleTime: 0 });
  useTopicInvalidation("runtime.changed", [["runtime.summary", {}]]);
  const { system, ports } = useSystemMetrics();
  const portNumbers = [...new Set(ports.data?.map((port) => port.port) ?? [])].sort((a, b) => a - b);
  const actionStyle = { background: "transparent", border: 0, cursor: "pointer", color: "inherit" };
  const navigate = useNavigate();
  const { state, daemonVersion } = useConnection();
  return (
    <footer
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "0 var(--space-3)",
        fontSize: "var(--text-xs)",
        color: "var(--fg-muted)",
        background: "var(--bg-0)",
        borderTop: "1px solid var(--border)",
      }}
    >
      <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
        <StatusDot tone={TONE[state]} label={LABEL[state]} />
        {LABEL[state]}
      </span>
      {agents?.list.data && (
        <button
          type="button"
          style={{ background: "transparent", border: 0, cursor: "pointer" }}
          onClick={() => void navigate({ to: "/agents" })}
        >
          에이전트 {agents.tasks.filter((task) => task.state !== "closed").length}(입력 대기 {agents.waiting})
        </button>
      )}
      <button type="button" style={actionStyle} onClick={() => void navigate({ to: "/runs" })}>
        실행 {summary.isError ? "조회 실패" : (summary.data?.active ?? "—")}
      </button>
      <button
        type="button"
        style={actionStyle}
        onClick={() => void navigate({ to: "/runs", search: { view: "ports" } })}
      >
        포트 {ports.isError ? "조회 실패" : ports.data ? portNumbers.join(", ") || "없음" : "—"}
      </button>
      <button
        type="button"
        style={actionStyle}
        onClick={() => void navigate({ to: "/runs", search: { view: "system" } })}
      >
        WSL {system.isError ? "조회 실패" : system.data ? memoryLabel(system.data) : "—"}
      </button>
      <GitStatus />
      <span style={{ flex: 1 }} />
      <UpdateStatus />
      {daemonVersion && <span>데몬 {daemonVersion}</span>}
    </footer>
  );
}
