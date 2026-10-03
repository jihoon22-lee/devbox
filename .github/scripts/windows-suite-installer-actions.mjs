// Exact owned NSIS windows; no silent flags and no activation helper shortcuts.
import assert from "node:assert/strict";
import { copyFile, readFile } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
import { observeUntil } from "./windows-suite-ui-context.mjs";
import { fileDigest } from "./suite-user-flow-results.mjs";
export function ownedNsisSpawnOptions(image, args) {
  assert.ok(path.win32.isAbsolute(image) && !/["\r\n]/.test(image));
  assert.ok(Array.isArray(args) && args.length >= 1);
  assert.ok(args.slice(0, -1).every((flag) => flag === "/S"));
  const last = args.at(-1),
    match = /^(?:\/D=|_\?=)(.+)$/.exec(last);
  assert.ok(
    match && path.win32.isAbsolute(match[1]) && !/["\r\n]/.test(match[1]),
    "Final unquoted absolute NSIS path required",
  );
  return { windowsVerbatimArguments: true, argv0: `"${image}"` };
}
export async function startOwnedInstaller(image, args, fixtureRoot, env = process.env) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const child = spawn(image, args, { env, stdio: "ignore", windowsHide: false, ...ownedNsisSpawnOptions(image, args) });
  await once(child, "spawn");
  let identity;
  await observeUntil(() => {
    identity = allWindowsProcesses().find((p) => p.Pid === child.pid && p.Path.toLowerCase() === image.toLowerCase());
    return !!identity;
  }, "owned NSIS process");
  const owner = captureWindowOwner(identity, fixtureRoot);
  const inspect = () => {
    try {
      return nativeWindowAction(owner, "Inspect");
    } catch {
      return null;
    }
  };
  const wait = async (test, label, timeout = 180000) => observeUntil(() => test(inspect()), label, timeout);
  return { child, owner, wait, inspect, invoke: (id) => nativeWindowAction(owner, "Invoke", { controlId: id }) };
}
export async function runVisibleSetup(setup, root, env = process.env) {
  const scratch = path.dirname(root),
    image = path.join(scratch, `setup-${randomUUID()}.exe`);
  await copyFile(setup, image);
  assert.equal(await fileDigest(image), await fileDigest(setup));
  const installer = await startOwnedInstaller(image, [`/D=${root}`], scratch, env);
  await installer.wait((view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible), "welcome");
  installer.invoke("1");
  await installer.wait((view) => view?.controls?.some((c) => c.id === "1019" && c.visible), "directory");
  installer.invoke("1");
  await installer.wait(
    (view) => view?.controls?.some((c) => c.name === "Devbox 설치 준비 완료" && c.visible),
    "installation finish",
  );
  installer.invoke("1");
  await observeUntil(() => installer.child.exitCode !== null, "NSIS finish exit");
  assert.equal(installer.child.exitCode, 0);
  return JSON.parse(await readFile(path.join(root, "suite-registration.json"), "utf8"));
}
export async function runVisibleRemoval(root, { cancel = false } = {}) {
  const scratch = path.dirname(root),
    source = path.join(root, "Uninstall.exe"),
    image = path.join(scratch, `uninstall-${randomUUID()}.exe`);
  await copyFile(source, image);
  assert.equal(await fileDigest(image), await fileDigest(source));
  // NSIS _?= selects its already-owned installation and avoids an untracked
  // temporary executable. This is the same interactive confirmation/sections.
  const installer = await startOwnedInstaller(image, [`_?=${root}`], scratch);
  await installer.wait(
    (view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible),
    "removal confirmation",
  );
  if (cancel) {
    installer.invoke("2");
    await observeUntil(() => installer.child.exitCode !== null, "removal cancelled");
    return;
  }
  installer.invoke("1");
  await observeUntil(
    async () => {
      try {
        return Boolean(JSON.parse(await readFile(path.join(root, "uninstall-complete.json"), "utf8")));
      } catch {
        return false;
      }
    },
    "owned removal receipt",
    180000,
  );
  await installer.wait((view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible), "removal finish");
  installer.invoke("1");
  await observeUntil(() => installer.child.exitCode !== null, "removal closed");
  assert.equal(installer.child.exitCode, 0);
}

export function rejectedInstallerIssue(view, expectedIssues) {
  const text = (view?.controls ?? []).map((control) => control.name ?? "").join("\n");
  return expectedIssues.find((issue) => new RegExp(`(^|[^a-z_])${issue}([^a-z_]|$)`).test(text)) ?? null;
}
export function directorySpaceRejected(view, proof) {
  if (!proof || !Number.isFinite(proof.availableBytes) || proof.availableBytes >= 128 * 1024 * 1024) return false;
  const labels = (view?.controls ?? [])
    .filter((control) => control.visible)
    .map((control) => control.name ?? "")
    .join("\n");
  return (
    view?.buttons?.some((button) => button.id === "1" && button.visible && !button.enabled) &&
    /필요.*(?:공간|디스크)/.test(labels) &&
    /(?:사용 가능|남은).*(?:공간|디스크)/.test(labels)
  );
}
export async function rejectVisibleSetup(setup, root, { expectedIssues, onRejected, spaceProof } = {}) {
  assert.ok(Array.isArray(expectedIssues) && expectedIssues.length > 0);
  assert.ok(expectedIssues.every((issue) => /^[a-z_]+$/.test(issue)));
  const scratch = path.dirname(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT),
    image = path.join(scratch, `rejected-setup-${randomUUID()}.exe`);
  await copyFile(setup, image);
  assert.equal(await fileDigest(image), await fileDigest(setup));
  const installer = await startOwnedInstaller(image, [`/D=${root}`], scratch);
  try {
    await installer.wait(
      (view) => view?.buttons?.some((b) => b.id === "1" && b.enabled && b.visible),
      "rejected setup welcome",
    );
    installer.invoke("1");
    await installer.wait(
      (view) => view?.controls?.some((control) => control.id === "1019" && control.visible),
      "rejected setup directory",
    );
    let observed = directorySpaceRejected(installer.inspect(), spaceProof) ? "nsis_directory_space_insufficient" : null,
      detailsOpened = false;
    if (!observed) installer.invoke("1");
    if (!observed)
      await installer.wait((view) => {
        if (
          !detailsOpened &&
          view?.buttons?.some((button) => button.id === "1027" && button.enabled && button.visible)
        ) {
          installer.invoke("1027");
          detailsOpened = true;
          return false;
        }
        observed = rejectedInstallerIssue(view, expectedIssues);
        return !!observed;
      }, "owned installer displays expected native preparation issue");
    if (onRejected) await onRejected(installer, observed);
    installer.invoke("2");
    await observeUntil(() => {
      if (installer.child.exitCode !== null) return true;
      const view = installer.inspect();
      if (view?.windows?.length > 1) installer.invoke("6");
      return false;
    }, "rejected setup cancelled");
    assert.notEqual(installer.child.exitCode, 0, "Rejected setup cannot report success");
    return { issue: observed, exitCode: installer.child.exitCode };
  } finally {
    if (installer.child.exitCode === null) {
      installer.invoke("2");
      await observeUntil(() => {
        if (installer.child.exitCode !== null) return true;
        if (installer.inspect()?.windows?.length > 1) installer.invoke("6");
        return false;
      }, "owned failed installer normal cancellation");
    }
  }
}
export async function rejectBusyVisibleUpdate(setup, root) {
  return rejectVisibleSetup(setup, root, { expectedIssues: ["suite_writers_must_close", "update_agent_busy"] });
}
