import type { ProjectConfig } from "../../rpc/gen/rpc";

export const emptyTrustConfig: ProjectConfig = {
  agent: { copy: [], setup: [], test: null, teardown: [] },
  launch: { services: [], terminals: [], agents: [] },
};
export function trustPreview(id = "a", overrides: Record<string, unknown> = {}) {
  return {
    status: overrides.requiresApproval === false ? "not_required" : "approval_required",
    projectId: id,
    projectName: "devbox",
    canonicalPath: "/fixture/devbox",
    configPath: "/fixture/devbox/.devbox/devbox.toml",
    digest: "digest-first",
    scopeVersion: 2,
    approved: false,
    requiresApproval: true,
    config: emptyTrustConfig,
    runDefinitions: [],
    executionDirectories: ["/fixture/devbox-agents/new"],
    ...overrides,
  };
}
