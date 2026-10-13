import type { Methods } from "../../rpc/gen/rpc";
export type AgentTask = Methods["agents.get"]["result"];
export type ToolProfile = Methods["agents.profiles"]["result"]["items"][number];
const GROUPS = [
  { key: "attention", label: "입력 대기", states: ["waiting", "idle"], oldestFirst: true },
  { key: "failed", label: "실패", states: ["failed"], oldestFirst: false },
  { key: "review", label: "검토 대기", states: ["review"], oldestFirst: false },
  { key: "running", label: "실행 중", states: ["running"], oldestFirst: false },
  { key: "preparing", label: "준비 중", states: ["preparing"], oldestFirst: false },
  { key: "closed", label: "닫힘(24시간)", states: ["closed"], oldestFirst: false },
] as const;
export function groupByUrgency(tasks: AgentTask[], nowMs: number) {
  return GROUPS.map((g) => ({
    key: g.key,
    label: g.label,
    items: tasks
      .filter((t) => (g.states as readonly string[]).includes(t.state))
      .filter((t) => t.state !== "closed" || nowMs - (t.closedMs ?? t.updatedMs) < 24 * 3600_000)
      .sort((a, b) => (g.oldestFirst ? 1 : -1) * (a.updatedMs - b.updatedMs) || a.id.localeCompare(b.id)),
  })).filter((g) => g.items.length > 0);
}
export function nextWaiting(groups: ReturnType<typeof groupByUrgency>, current?: string): string | undefined {
  const items = groups.find((g) => g.key === "attention")?.items ?? [];
  return items[(items.findIndex((t) => t.id === current) + 1) % items.length]?.id;
}
export function attentionCount(tasks: AgentTask[]) {
  return tasks.filter((t) => t.state === "waiting" || t.state === "idle" || t.state === "failed").length;
}
export function hookMissing(tasks: AgentTask[], profiles: ToolProfile[], nowMs: number) {
  const claude = new Set(profiles.filter((p) => p.hooks === "claude").map((p) => p.id));
  return tasks.some(
    (t) => t.state === "running" && claude.has(t.profile) && !t.sessionId && nowMs - t.createdMs > 90_000,
  );
}
export function fillTemplate(body: string, vars: { title: string; branch: string; project: string }) {
  return body.replace(/\{\{(title|branch|project)\}\}/g, (_, key: keyof typeof vars) => vars[key]);
}
/** Approximate only: the daemon reserves the branch and may add a collision suffix. */
export function previewBranch(title: string, nowMs: number) {
  const slug = title
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .toLowerCase()
    .replace(/^-/, "")
    .slice(0, 40)
    .replace(/-$/, "");
  return `agent/${slug || `task-${Math.floor(nowMs / 1000)}`}`;
}
export const STATE_LABEL: Record<AgentTask["state"], string> = {
  preparing: "준비 중",
  running: "실행 중",
  waiting: "입력 대기",
  idle: "다음 지시 대기",
  failed: "실패",
  review: "검토 대기",
  closed: "닫힘",
};
export function failureLabel(reason: string | null) {
  if (reason === "session_lost") return "WSL이 멈춰 세션이 끊겼습니다";
  if (reason?.startsWith("killed:")) return "외부에서 종료됨(메모리 부족이나 다른 도구가 끝냈을 수 있습니다)";
  if (reason?.startsWith("exit:")) return `도구가 오류로 끝났습니다(종료 코드 ${reason.slice(5)})`;
  if (reason?.startsWith("setup:")) return "준비 명령이 실패했습니다";
  return "에이전트 작업이 실패했습니다";
}
