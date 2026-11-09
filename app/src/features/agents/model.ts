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
/** A task of a profile devbox connects to that reported no session in its first 90 seconds, or a
 * pi launch that has not said in its first 90 seconds that it is ready. */
export function hookMissing(tasks: AgentTask[], profiles: ToolProfile[], nowMs: number) {
  const hooks = new Map(profiles.map((p) => [p.id, p.hooks]));
  // Only Claude reports a session before a prompt; the others report it with the first one.
  const reports = (t: AgentTask) => {
    const kind = hooks.get(t.profile);
    return kind === "claude" || (kind !== undefined && kind !== "none" && t.prompt.trim() !== "");
  };
  // pi has its session from the start, and says on every launch, prompt or not, that it is ready.
  const silent = (t: AgentTask) =>
    hooks.get(t.profile) === "pi"
      ? t.startingSinceMs !== null && nowMs - t.startingSinceMs > 90_000
      : reports(t) && !t.sessionId && nowMs - t.createdMs > 90_000;
  return tasks.some((t) => t.state === "running" && silent(t));
}
export function fillTemplate(body: string, vars: { title: string; branch: string; project: string }) {
  return body.replace(/\{\{(title|branch|project)\}\}/g, (_, key: keyof typeof vars) => vars[key]);
}
/** Every task branch lives under this prefix; cleanup only ever touches branches that do. */
export const BRANCH_PREFIX = "agent/";
const MAX_BRANCH_NAME = 40;
/** The part after `agent/` that the daemon makes from a title: ASCII only, empty when there is none. */
export function titleSlug(title: string) {
  return title
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .toLowerCase()
    .replace(/^-/, "")
    .slice(0, MAX_BRANCH_NAME)
    .replace(/-$/, "");
}
/** Approximate only: the daemon reserves the branch and may add a collision suffix. */
export function previewBranch(title: string, nowMs: number) {
  return `${BRANCH_PREFIX}${titleSlug(title) || `task-${Math.floor(nowMs / 1000)}`}`;
}
/**
 * Why a branch name (the part after `agent/`) cannot be used, or null. This is the daemon's slug
 * rule (`plan::checked_slug`), which the daemon applies again: lowercase ASCII letters and digits
 * with single hyphens between them, up to 40 characters. It is a strict subset of what
 * `git check-ref-format` accepts, so anything that passes here is also a valid ref name.
 */
export function branchNameProblem(name: string) {
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name))
    return "브랜치 이름에는 영문 소문자, 숫자, 하이픈(-)만 쓸 수 있습니다. 하이픈은 처음이나 끝에 쓸 수 없고 연달아 쓸 수도 없습니다. 이름을 고쳐 주세요.";
  if (name.length > MAX_BRANCH_NAME) return "브랜치 이름이 40자를 넘었습니다. 40자 이내로 줄여 주세요.";
  return null;
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
/** The state mark's icon colour. Only a waiting task is in the warning colour; the state name stays in text colours. */
export const STATE_TONE: Record<AgentTask["state"], string> = {
  preparing: "var(--progress)",
  running: "var(--progress)",
  waiting: "var(--warn)",
  idle: "var(--fg-muted)",
  failed: "var(--danger)",
  review: "var(--ok)",
  closed: "var(--fg-muted)",
};
/** The tool's name from the profile list, or the task's profile id while the list is missing or lacks it. */
export function toolLabel(profiles: ToolProfile[] | undefined, profile: string) {
  return profiles?.find((p) => p.id === profile)?.label ?? profile;
}
export function failureLabel(reason: string | null) {
  if (reason === "session_lost") return "WSL이 멈춰 세션이 끊겼습니다";
  if (reason?.startsWith("killed:")) return "외부에서 종료됨(메모리 부족이나 다른 도구가 끝냈을 수 있습니다)";
  if (reason?.startsWith("exit:")) return `도구가 오류로 끝났습니다(종료 코드 ${reason.slice(5)})`;
  if (reason?.startsWith("setup:")) return "준비 명령이 실패했습니다";
  return "에이전트 작업이 실패했습니다";
}
