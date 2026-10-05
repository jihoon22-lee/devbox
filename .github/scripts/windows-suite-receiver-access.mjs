import assert from "node:assert/strict";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";

export async function withUnavailableReceiver(changeAccess, probe) {
  await changeAccess("Deny");
  let failure;
  try {
    await probe();
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    try {
      await changeAccess("Restore");
    } catch (error) {
      if (!failure) throw error;
    }
  }
}

export function receiverAccessPending(context) {
  return existsSync(path.join(context.root, ".handoff-receiver-access.json"));
}

export function changeKnowledgeExecuteAccess(context, action) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  assert.ok(["Deny", "Restore"].includes(action));
  if (action === "Deny")
    assert.equal(
      allWindowsProcesses().some((p) => p.Path.toLowerCase() === context.executable.toLowerCase()),
      false,
      "Receiver must be normally closed before changing execution access",
    );
  const member = context.manifest.members.find((item) => item.product === "knowledge");
  assert.ok(member);
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-File",
      path.resolve(".github/scripts/windows-suite-receiver-access.ps1"),
      "-Root",
      context.root,
      "-Image",
      context.executable,
      "-ExpectedDigest",
      member.sha256,
      "-Action",
      action,
    ],
    { encoding: "utf8", windowsHide: true, timeout: 20000 },
  );
  assert.equal(
    result.status,
    0,
    `Owned receiver ${action} failed: ${result.stderr || result.error?.code || "unknown"}`,
  );
}
