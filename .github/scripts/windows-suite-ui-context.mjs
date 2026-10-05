import { cleanupOwnedFixture, withOwnedCleanup } from "./owned-fixture-cleanup.mjs";
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
import { nativeIssueCollector } from "./windows-reviewed-helper-evidence.mjs";
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
// WebView shutdown precedes the exact child's exit event during normal Close.
// A disconnected renderer is never itself proof that the owned child exited.
export async function observeNormalClose(
  { child, product, cdp, ui, reviewWorkspaceClose, reportClose },
  observe = observeUntil,
) {
  let reviewed = false;
  let rendererClosed = false;
  let reviewObserved = false;
  const report = () =>
    reportClose?.({
      reviewObserved,
      reviewSubmitted: reviewed,
      rendererDisconnected: rendererClosed,
      childExited: child.exitCode !== null,
    });
  report();
  await observe(async () => {
    if (child.exitCode !== null) {
      report();
      return true;
    }
    if (product !== "workspace" || reviewed || rendererClosed) return false;
    try {
      if (await cdp.evaluate('!!document.querySelector(\'[role="dialog"][aria-label="Workspace 종료 검토"]\')')) {
        reviewObserved = true;
        report();
        if (reviewWorkspaceClose) await reviewWorkspaceClose();
        else await ui.click({ role: "button", name: "종료", scope: { role: "dialog", name: "Workspace 종료 검토" } });
        reviewed = true;
        report();
      }
    } catch (error) {
      if (error?.message !== "CDP disconnected") throw error;
      rendererClosed = true;
      report();
    }
    return child.exitCode !== null;
  }, "normal native close");
}
export async function requestNormalClose(context, closeWindow, observe = observeUntil) {
  if (context.child.exitCode !== null) return;
  await closeWindow();
  await observeNormalClose(context, observe);
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
  const nativeIssues = nativeIssueCollector();
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    releaseCdpSession({ cdp, policy });
    disposed = true;
  };
  try {
    if (policy) installElevatedCdpPolicy(policy);
    child = spawn(executable, [], { cwd: root, env, stdio: ["ignore", "ignore", "pipe"] });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => nativeIssues.write(chunk));
    // A reviewed helper may retain the pipe beyond the product's shutdown.
    // It may report a preflight failure, but must not keep this runner alive.
    child.stderr.unref();
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
    const cleanup = () =>
      cleanupOwnedFixture(
        { identity: processIdentity, child },
        () => stopOwnedProcess(processIdentity, executable, child),
        dispose,
      );
    const close = ({ reviewWorkspaceClose } = {}) =>
      withOwnedCleanup(async () => {
        await requestNormalClose({ child, product, cdp, ui, reviewWorkspaceClose }, () =>
          nativeWindowAction(owner, "Close"),
        );
      }, cleanup);
    const delivery = async (method) => {
      assert.ok(
        ["restore_inventory", "suite_recovery", "suite_health"].includes(method),
        "Read-only delivery observation required",
      );
      const result = await cdp.evaluate(
        `(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');return invoke('plugin:control-center|delivery',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+10000,route:'recovery',context:d.context},method:${JSON.stringify(method)},args:{}}});})()`,
      );
      const code = [result.operation.outcome.code, result.value?.issue]
        .filter((value) => typeof value === "string" && /^[a-z][a-z0-9_]{0,100}$/u.test(value))
        .join("/");
      assert.equal(
        result.operation.outcome.state,
        "succeeded",
        `Read-only delivery ${method} failed: ${code || "unknown"}`,
      );
      return result.value;
    };
    return {
      ...identity,
      root,
      installationKey,
      manifest,
      executable,
      child,
      nativeIssueCodes: nativeIssues.codes,
      processIdentity,
      ui,
      cdp,
      close,
      dispose,
      delivery,
      body: () => cdp.evaluate("document.body.innerText"),
    };
  } catch (error) {
    // Retain the first launch boundary before releasing its pipes and policy.
    // Fixed issue tokens are already filtered by nativeIssueCollector.
    if (error && typeof error === "object" && Object.isExtensible(error)) {
      error.launch = {
        product,
        identityCaptured: Boolean(processIdentity),
        exitCode: Number.isInteger(child?.exitCode) ? child.exitCode : null,
        signal:
          typeof child?.signalCode === "string" && /^SIG[A-Z0-9]{1,16}$/.test(child.signalCode)
            ? child.signalCode
            : null,
        nativeIssues: [...nativeIssues.codes],
      };
    }
    await cleanupOwnedFixture(
      { identity: processIdentity, child },
      () => stopOwnedProcess(processIdentity, executable, child),
      dispose,
    ).catch(() => {});
    throw error;
  }
}
