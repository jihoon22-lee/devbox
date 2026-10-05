import assert from "node:assert/strict";
import { lstat, realpath, readdir, open } from "node:fs/promises";
import path from "node:path";
const token = (value) => typeof value === "string" && /^[A-Za-z0-9_.:-]{1,64}$/.test(value);
export async function readWorkspaceAgentOperations(dataRoot) {
  const root = path.resolve(dataRoot);
  assert.equal(await realpath(root), root, "Owned data root redirected");
  const directory = path.join(root, "logs");
  const info = await lstat(directory);
  assert.ok(info.isDirectory() && !info.isSymbolicLink(), "Owned logs redirected");
  assert.equal(await realpath(directory), directory, "Owned logs redirected");
  const names = (await readdir(directory))
    .filter((name) => /^operations-\d{4}-\d{2}-\d{2}\.jsonl$/.test(name))
    .sort()
    .slice(-2);
  const results = [];
  for (const name of names) {
    const file = path.join(directory, name);
    const before = await lstat(file);
    assert.ok(before.isFile() && !before.isSymbolicLink() && before.size <= 4 * 1024 * 1024, "Invalid owned log file");
    assert.equal(await realpath(file), file, "Owned log file redirected");
    const handle = await open(file, "r");
    try {
      const current = await handle.stat();
      assert.ok(
        current.dev === before.dev && current.ino === before.ino && current.size <= 4 * 1024 * 1024,
        "Owned log file changed",
      );
      const buffer = Buffer.alloc(current.size);
      await handle.read(buffer, 0, buffer.length, 0);
      for (const line of buffer.toString("utf8").split("\n")) {
        let entry;
        try {
          entry = JSON.parse(line);
        } catch {
          continue;
        }
        if (
          !entry ||
          entry.product !== "workspace" ||
          !["workspace.agents", "workspace.registry", "workspace.terminal", "workspace.lsp"].includes(
            entry.component,
          ) ||
          !["failed", "cancelled", "rejected", "panicked", "limit"].includes(entry.outcome) ||
          !token(entry.method) ||
          (entry.code != null && !token(entry.code)) ||
          !Number.isSafeInteger(entry.tsMs) ||
          entry.tsMs < 0 ||
          !Number.isSafeInteger(entry.durationMs) ||
          entry.durationMs < 0
        )
          continue;
        results.push({
          tsMs: entry.tsMs,
          component: entry.component,
          method: entry.method,
          durationMs: entry.durationMs,
          outcome: entry.outcome,
          code: entry.code ?? null,
        });
      }
    } finally {
      await handle.close();
    }
  }
  return results.slice(-30);
}
