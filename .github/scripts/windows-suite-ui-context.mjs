// Attach real input and read-only observations to an exact owned installed product.
import assert from "node:assert/strict";
import { readFile, realpath } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
import { packagedIdentity, installedFixtureIdentity, fileDigest } from "./suite-user-flow-results.mjs";
import { freePort, connect } from "./workspace-cdp-fixture.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import {
  allWindowsProcesses,
  windowsProcessIsElevated,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
  stopOwnedProcess,
} from "./windows-packaged-smoke.mjs";
export async function observeUntil(check, label, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(100);
  }
  throw new Error(`Owned UI observation timed out: ${label}`);
}
export async function createInstalledProductContext(product, { legacyAssets } = {}) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  assert.ok(["workspace", "api-studio", "knowledge", "control-center"].includes(product));
  const identity = await packagedIdentity(),
    installationKey = await installedFixtureIdentity();
  const root = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT);
  const manifest = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
  const member = manifest.members.find((item) => item.product === product);
  assert.equal(member.executable, `generations/${manifest.generation}/products/${product}/devbox-${product}.exe`);
  const executable = await realpath(path.join(root, member.executable));
  assert.equal(executable.toLowerCase(), path.resolve(root, member.executable).toLowerCase());
  const release = JSON.parse(
    await readFile(path.join(legacyAssets ?? process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"), "utf8"),
  );
  if (legacyAssets) {
    assert.equal(release.sourceSha, "1c97b41ee10ca0df7c062338bfe85659af025a89");
    assert.equal(release.setup.sha256, "ff20ee2d45365bbd2d98526d56dbf0e71b0273bc70ddb4f54dd8d0725b4004b3");
  }
  const image = release.products
    .find((item) => item.id === product)
    .files.find((item) => item.name === `devbox-${product}.exe`);
  assert.equal(await fileDigest(executable), member.sha256);
  assert.equal(member.sha256, image.sha256);
  assert.equal(
    allWindowsProcesses().filter((p) => p.Path.toLowerCase() === executable.toLowerCase()).length,
    0,
    "Prior owned product remains open",
  );
  const port = await freePort();
  const policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
  const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
  for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
  let child, processIdentity, owner, cdp;
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    releaseCdpSession({ cdp, policy });
    disposed = true;
  };
  try {
    if (policy) installElevatedCdpPolicy(policy);
    child = spawn(executable, [], { cwd: root, env, stdio: "ignore" });
    await once(child, "spawn");
    await observeUntil(() => {
      processIdentity = allWindowsProcesses().find(
        (p) => p.Pid === child.pid && p.Path.toLowerCase() === executable.toLowerCase(),
      );
      return Boolean(processIdentity);
    }, "owned product process");
    owner = captureWindowOwner(processIdentity, path.dirname(root));
    cdp = await connect(port, child);
    const ui = createUiDriver({
      cdp,
      evidenceRoot: `product-foundation-evidence/user-flows/screenshots/${product}-suite`,
      closeOwnedWindow: async () => nativeWindowAction(owner, "Close"),
    });
    await observeUntil(
      () => cdp.evaluate(`Boolean(document.querySelector('nav[aria-label="제품 화면"]'))`),
      "product shell",
    );
    const close = async () => {
      try {
        if (child.exitCode === null) {
          nativeWindowAction(owner, "Close");
          let reviewed = false;
          await observeUntil(async () => {
            if (child.exitCode !== null) return true;
            if (
              product === "workspace" &&
              !reviewed &&
              (await cdp.evaluate("document.body.innerText")).includes("Workspace 종료 검토")
            ) {
              await ui.click({ role: "button", name: "종료", scope: { role: "dialog", name: "Workspace 종료 검토" } });
              reviewed = true;
            }
            return false;
          }, "normal native close");
        }
      } finally {
        try {
          if (child.exitCode === null) await stopOwnedProcess(processIdentity, executable, child);
        } finally {
          dispose();
        }
      }
    };
    const delivery = async (method) => {
      assert.ok(
        ["restore_inventory", "suite_recovery", "suite_health"].includes(method),
        "Read-only delivery observation required",
      );
      const result = await cdp.evaluate(
        `(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');return invoke('plugin:control-center|delivery',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+10000,route:'recovery',context:d.context},method:${JSON.stringify(method)},args:{}}});})()`,
      );
      assert.equal(result.operation.outcome.state, "succeeded");
      return result.value;
    };
    return {
      ...identity,
      root,
      installationKey,
      manifest,
      executable,
      child,
      processIdentity,
      ui,
      cdp,
      close,
      dispose,
      delivery,
      body: () => cdp.evaluate("document.body.innerText"),
    };
  } catch (error) {
    try {
      if (child?.exitCode === null && processIdentity) {
        await stopOwnedProcess(processIdentity, executable, child);
      } else if (child?.exitCode === null) {
        child.kill();
        await observeUntil(() => child.exitCode !== null || child.signalCode !== null, "failed owned spawn cleanup");
      }
    } finally {
      dispose();
    }
    throw error;
  }
}
