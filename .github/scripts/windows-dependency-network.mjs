// Failure injection changes only one owned image on a disposable hosted runner.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { requireHostedNetworkFixture } from "./fixture-network-safety.mjs";

export function dependencyNetworkScope(owner, sha256, env = process.env) {
  const { runId } = requireHostedNetworkFixture(env);
  const root = path.win32.resolve(owner.fixtureRoot);
  const image = path.win32.resolve(owner.identity.Path);
  assert.equal(path.win32.basename(root), "Suite UI Fixture");
  assert.match(path.win32.basename(path.win32.dirname(root)), /^devbox-suite-delivery-[a-f0-9]{32}$/);
  assert.ok(root.toLowerCase().startsWith(`${path.win32.resolve(env.RUNNER_TEMP).toLowerCase()}\\`));
  assert.match(path.win32.relative(root, image), /^generations\\[^\\]+\\products\\workspace\\devbox-workspace\.exe$/);
  assert.ok(Number.isSafeInteger(owner.identity.Pid) && owner.identity.Pid > 0);
  assert.match(owner.started, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$/);
  assert.match(sha256, /^[a-f0-9]{64}$/);
  return {
    root,
    image,
    pid: owner.identity.Pid,
    started: owner.started,
    sha256,
    runId,
    ruleName: `DevboxFixture-Dependencies-${randomUUID()}`,
  };
}
function firewall(action, scope) {
  requireHostedNetworkFixture();
  assert.equal(process.platform, "win32", "Hosted Windows required");
  const args = [
    "-NoProfile",
    "-NonInteractive",
    "-File",
    path.resolve(".github/scripts/windows-dependency-network.ps1"),
    "-Action",
    action,
    "-InstallRoot",
    scope.root,
    "-Executable",
    scope.image,
    "-OwnerProcessId",
    String(scope.pid),
    "-ExpectedStart",
    scope.started,
    "-ExpectedDigest",
    scope.sha256,
    "-RuleName",
    scope.ruleName,
  ];
  const result = spawnSync("powershell.exe", args, { encoding: "utf8", timeout: 30000 });
  assert.equal(result.status, 0, `Owned dependency firewall ${action} failed: ${(result.stderr ?? "").slice(0, 500)}`);
}
export async function withDependencyNetworkBlock(scope, action, invoke = firewall) {
  // The Add adapter cleans its own partial creation on failure. Never remove a
  // preexisting rule after a rejected Add; it was not created by this scope.
  await invoke("Add", scope);
  let firstError;
  try {
    return await action();
  } catch (error) {
    firstError = error;
    throw error;
  } finally {
    try {
      await invoke("Remove", scope);
    } catch (error) {
      if (!firstError) throw error;
      throw new AggregateError([firstError, error], "Dependency failure and owned firewall cleanup failure");
    }
  }
}

export function dependencyProviderFailureVisible(document) {
  return Array.from(document.querySelectorAll(".dependency-enrichment-summaries > span")).some(
    (node) => /전송\s+[1-9]\d*/u.test(node.textContent) && /실패\s+[1-9]\d*/u.test(node.textContent),
  );
}
