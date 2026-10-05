// A location-keyed portable copy of the exact installed candidate, on hosted Windows.
import assert from "node:assert/strict";
import { cp, readFile, realpath, mkdtemp, rename, rm } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";
import { fileDigest } from "./suite-user-flow-results.mjs";
import { freePort, connect } from "./workspace-cdp-fixture.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import {
  allWindowsProcesses,
  stopOwnedProcess,
  windowsProcessIsElevated,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
} from "./windows-packaged-smoke.mjs";
import { observeUntil } from "./windows-suite-ui-context.mjs";
export async function copyDirectProductImage(source, root) {
  await cp(path.dirname(source), root, { recursive: true, errorOnExist: true });
  // WebView2 policy uses the image basename. A concurrent installed copy owns
  // the original name, so this location-keyed portable fixture needs its own.
  const executable = path.join(root, `${path.basename(root)}.exe`);
  await rename(path.join(root, path.basename(source)), executable);
  return realpath(executable);
}
export async function createDirectProductContext(product) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const installed = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT);
  const manifest = JSON.parse(await readFile(path.join(installed, "devbox-installation.json"), "utf8"));
  const member = manifest.members.find((item) => item.product === product);
  assert.ok(member);
  const source = await realpath(path.join(installed, member.executable));
  assert.equal(await fileDigest(source), member.sha256);
  const release = JSON.parse(
    await readFile(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"), "utf8"),
  );
  assert.equal(
    member.sha256,
    release.products.find((item) => item.id === product).files.find((item) => item.name === path.basename(source))
      .sha256,
  );
  const root = await mkdtemp(path.join(path.dirname(installed), `direct-${product}-`));
  let child, processIdentity, cdp, policy, executable;
  try {
    executable = await copyDirectProductImage(source, root);
    assert.equal(await fileDigest(executable), member.sha256);
    const port = await freePort();
    policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
    if (policy) installElevatedCdpPolicy(policy);
    const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
    for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
    child = spawn(executable, [], { cwd: root, env, stdio: "ignore" });
    await once(child, "spawn");
    processIdentity = allWindowsProcesses().find(
      (item) => item.Pid === child.pid && item.Path.toLowerCase() === executable.toLowerCase(),
    );
    assert.ok(processIdentity);
    const owner = captureWindowOwner(processIdentity, path.dirname(root));
    cdp = await connect(port, child);
    const ui = createUiDriver({
      cdp,
      evidenceRoot: `product-foundation-evidence/user-flows/screenshots/${product}-direct`,
      closeOwnedWindow: () => nativeWindowAction(owner, "Close"),
    });
    await observeUntil(
      () => cdp.evaluate('Boolean(document.querySelector(".product-shell > main"))'),
      "portable product shell",
    );
    const close = async () => {
      try {
        nativeWindowAction(owner, "Close");
        if (product === "workspace")
          await observeUntil(async () => {
            if (child.exitCode !== null) return true;
            if (await cdp.evaluate('!!document.querySelector(\'[role="dialog"][aria-label="Workspace 종료 검토"]\')')) {
              await ui.click({ role: "button", name: "종료", scope: { role: "dialog", name: "Workspace 종료 검토" } });
              return true;
            }
            return false;
          }, "portable Workspace close review");
        await observeUntil(() => child.exitCode !== null, "portable normal close");
      } finally {
        if (child.exitCode === null) await stopOwnedProcess(processIdentity, executable, child);
        releaseCdpSession({ cdp, policy });
        await rm(root, { recursive: true, force: true });
      }
    };
    return { root, product, executable, ui, cdp, processIdentity, close };
  } catch (error) {
    if (child?.exitCode === null && processIdentity) await stopOwnedProcess(processIdentity, executable, child);
    releaseCdpSession({ cdp, policy });
    await rm(root, { recursive: true, force: true });
    throw error;
  }
}
