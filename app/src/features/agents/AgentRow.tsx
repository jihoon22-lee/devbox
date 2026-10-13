import { useQueryClient } from "@tanstack/react-query";
import {
  CircleCheck,
  CircleX,
  LoaderCircle,
  MessageCircleQuestion,
  PauseCircle,
  PlayCircle,
  XCircle,
} from "lucide-react";
import { useRef } from "react";
import type { Methods } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { Button } from "../../ui";
import { AGENT_KEYS } from "./AgentData";
import s from "./Agents.module.css";
import { type AgentTask, failureLabel, STATE_LABEL } from "./model";
import { TokenMetrics } from "./TokenMetrics";

const ICON = {
  preparing: LoaderCircle,
  running: PlayCircle,
  waiting: MessageCircleQuestion,
  idle: PauseCircle,
  failed: CircleX,
  review: CircleCheck,
  closed: XCircle,
};
const TONE = {
  preparing: "var(--info)",
  running: "var(--info)",
  waiting: "var(--warn)",
  idle: "var(--warn)",
  failed: "var(--danger)",
  review: "var(--ok)",
  closed: "var(--fg-muted)",
};
const relative = new Intl.RelativeTimeFormat("ko", { numeric: "auto" });
function age(createdMs: number, now: number) {
  const seconds = Math.max(0, Math.floor((now - createdMs) / 1000));
  return seconds < 60
    ? relative.format(-seconds, "second")
    : seconds < 3600
      ? relative.format(-Math.floor(seconds / 60), "minute")
      : relative.format(-Math.floor(seconds / 3600), "hour");
}
export function AgentRow({
  task,
  selected,
  projectName,
  now,
  stats,
  resources,
  onSelect,
}: {
  task: AgentTask;
  selected: boolean;
  projectName: string;
  now: number;
  stats?: Methods["agents.git_stats"]["result"]["items"][number];
  resources?: Methods["agents.resources"]["result"]["items"][number];
  onSelect: (id: string) => void;
}) {
  const Icon = ICON[task.state];
  const resume = useRpcMutation("agents.resume"),
    qc = useQueryClient(),
    pending = useRef(false);
  const canResume =
    task.state !== "closed" &&
    task.state !== "preparing" &&
    (!task.terminalId ||
      task.toolExited ||
      (task.state === "failed" && (task.reason === "session_lost" || task.reason?.startsWith("killed:"))));
  const doResume = async () => {
    if (pending.current) return;
    pending.current = true;
    try {
      const updated = await resume.mutateAsync({ id: task.id });
      qc.setQueryData(["agents.get", { id: task.id }], updated);
      for (const queryKey of AGENT_KEYS) void qc.invalidateQueries({ queryKey }).catch(() => {});
    } catch {
      /* Typed error is shown in this row. */
    } finally {
      pending.current = false;
    }
  };
  const error = resume.error && messageFor(resume.error.code, resume.error.detail);
  return (
    <div className={s.row} data-selected={selected || undefined}>
      <button
        type="button"
        className={s.open}
        aria-current={selected ? "true" : undefined}
        onClick={() => onSelect(task.id)}
      >
        <span className={s.title}>
          <Icon size={16} aria-label={STATE_LABEL[task.state]} style={{ color: TONE[task.state] }} />
          <strong>{task.title}</strong>
          <span className={s.muted}>{age(task.createdMs, now)}</span>
        </span>
        <span className={s.muted}>{projectName}</span>
        <span className={s.message}>
          {task.state === "failed" ? failureLabel(task.reason) : task.attention?.message || STATE_LABEL[task.state]}
        </span>
        <span className={s.metrics}>
          {stats && (
            <>
              <span>변경 {stats.changedFiles}</span>
              <span>뒤처짐 {stats.behind}</span>
            </>
          )}
          {resources?.cpuPercent != null && <span>CPU {resources.cpuPercent.toFixed(1)}%</span>}
          {resources?.memoryBytes != null && <span>메모리 {(resources.memoryBytes / 1048576).toFixed(0)} MB</span>}
          <TokenMetrics tokens={task.tokens} truncated={task.tokensTruncated} />
        </span>
      </button>
      {canResume && (
        <Button size="sm" disabled={resume.isPending} onClick={() => void doResume()}>
          {resume.isPending ? "재개 중…" : "재개"}
        </Button>
      )}
      {error && (
        <p role="alert">
          {error.title} {error.action}
        </p>
      )}
    </div>
  );
}
