import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, mkdir, writeFile, symlink, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { readWorkspaceAgentOperations } from "./windows-workspace-agent-observation.mjs";
const entry = {
  tsMs: 1,
  version: "0.9.0",
  product: "workspace",
  component: "workspace.agents",
  method: "list",
  durationMs: 2,
  outcome: "rejected",
  code: "stale_context",
};
test("projects bounded operation identifiers without raw messages or arguments", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "agent-observation-"));
  try {
    await mkdir(path.join(root, "logs"));
    const rows = Array.from({ length: 35 }, (_, tsMs) => ({
      ...entry,
      tsMs,
      args: { secret: "private" },
      message: "private",
    }));
    rows.push({ ...entry, method: "/private/path" }, { ...entry, component: "knowledge.activity" });
    await writeFile(
      path.join(root, "logs", "operations-2026-10-05.jsonl"),
      rows.map(JSON.stringify).join("\n") + "\n{partial",
    );
    const observed = await readWorkspaceAgentOperations(root);
    assert.equal(observed.length, 30);
    assert.deepEqual(Object.keys(observed[0]).sort(), ["code", "component", "durationMs", "method", "outcome", "tsMs"]);
    assert.equal(observed[0].tsMs, 5);
    assert.equal(JSON.stringify(observed).includes("private"), false);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
test("rejects redirected log directory and file without reading outside owned root", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "agent-boundary-"));
  try {
    const outside = path.join(root, "outside");
    await mkdir(outside);
    await symlink(outside, path.join(root, "logs"), "dir");
    await assert.rejects(readWorkspaceAgentOperations(root));
    await rm(path.join(root, "logs"));
    await mkdir(path.join(root, "logs"));
    await writeFile(path.join(outside, "private"), JSON.stringify(entry));
    await symlink(path.join(outside, "private"), path.join(root, "logs", "operations-2026-10-05.jsonl"));
    await assert.rejects(readWorkspaceAgentOperations(root));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
