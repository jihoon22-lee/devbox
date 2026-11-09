import { useNavigate } from "@tanstack/react-router";
import { useAgentSummary } from "../features/agents/AgentData";
import { memoryLabel, useSystemMetrics } from "../features/runs/systemMetrics";
import { UpdateStatus } from "../features/settings/Update";
import { GitStatus } from "../features/source/GitStatus";
import { useConnection } from "../rpc/connection";
import { useRpcQuery, useTopicInvalidation } from "../rpc/query";
import { StatusDot } from "../ui";
import s from "./StatusBar.module.css";
import { usePageVisible } from "./visibility";

const LABEL = {
  connecting: "연결 중",
  ready: "데몬 연결됨",
  reconnecting: "다시 연결 중",
  versionMismatch: "버전 불일치",
} as const;
const PORT_LIMIT = 3;
const TONE = { connecting: "progress", ready: "ok", reconnecting: "warn", versionMismatch: "danger" } as const;

type PortRow = { port: number; runId?: string | null; containerId?: string | null; projectId?: string | null };

// Owned ports (tied to a run, container or project) come first; each group is ascending and deduplicated.
export function portSummary(rows: readonly PortRow[]): { shown: string; all: string } {
  const owned = new Map<number, boolean>();
  for (const row of rows) {
    const mine = Boolean(row.runId || row.containerId || row.projectId);
    owned.set(row.port, (owned.get(row.port) ?? false) || mine);
  }
  const ordered = [...owned.entries()].sort(([a, aOwned], [b, bOwned]) => Number(bOwned) - Number(aOwned) || a - b);
  const numbers = ordered.map(([port]) => port);
  const rest = numbers.length - PORT_LIMIT;
  const all = numbers.join(", ");
  return { shown: rest > 0 ? `${numbers.slice(0, PORT_LIMIT).join(", ")} 외 ${rest}개` : all, all };
}

export function StatusBar() {
  const agents = useAgentSummary();
  const visible = usePageVisible();
  const summary = useRpcQuery("runtime.summary", {}, { enabled: visible, staleTime: 0 });
  useTopicInvalidation("runtime.changed", [["runtime.summary", {}]]);
  const { system, ports } = useSystemMetrics();
  const portText = portSummary(ports.data ?? []);
  const actionStyle = { background: "transparent", border: 0, cursor: "pointer", color: "inherit" };
  const navigate = useNavigate();
  const { state, daemonVersion } = useConnection();
  return (
    <footer
      className={s.bar}
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "0 var(--space-3)",
        whiteSpace: "nowrap",
        overflow: "hidden",
        minWidth: 0,
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
        title={portText.all ? `포트 ${portText.all}` : undefined}
        onClick={() => void navigate({ to: "/runs", search: { view: "ports" } })}
      >
        포트 {ports.isError ? "조회 실패" : ports.data ? portText.shown || "없음" : "—"}
      </button>
      <button
        type="button"
        className={s.memory}
        style={actionStyle}
        onClick={() => void navigate({ to: "/runs", search: { view: "system" } })}
      >
        WSL {system.isError ? "조회 실패" : system.data ? memoryLabel(system.data) : "—"}
      </button>
      <GitStatus />
      <span style={{ flex: 1 }} />
      <UpdateStatus />
      {daemonVersion && <span style={{ flexShrink: 0 }}>데몬 {daemonVersion}</span>}
    </footer>
  );
}
