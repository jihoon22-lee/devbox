import type { Methods } from "../../rpc/gen/rpc";
export const profiles: Methods["agents.profiles"]["result"]["items"] = [
  { id: "claude", label: "Claude Code", command: "claude", hooks: "claude", waitingPatterns: [] },
  { id: "codex", label: "Codex", command: "codex", hooks: "codex", waitingPatterns: [] },
];
export function task(
  id: string,
  state: Methods["agents.get"]["result"]["state"] = "running",
  updatedMs = 1,
): Methods["agents.get"]["result"] {
  return {
    id,
    projectId: "a",
    title: `작업 ${id}`,
    profile: "claude",
    baseBranch: "main",
    branch: `agent/${id}`,
    worktreePath: "/fixture/worktree",
    workdir: "/fixture/worktree",
    sessionId: "s",
    terminalId: "t",
    toolExited: false,
    state,
    reason: null,
    attention: null,
    port: 3000,
    prompt: "",
    followUps: [],
    tokens: null,
    tokensTruncated: false,
    activitySeq: 0,
    prUrl: null,
    closedHow: null,
    createdMs: updatedMs,
    updatedMs,
    closedMs: state === "closed" ? updatedMs : null,
    closedCommits: null,
    closedHeadOid: null,
    cleanupError: null,
  };
}
