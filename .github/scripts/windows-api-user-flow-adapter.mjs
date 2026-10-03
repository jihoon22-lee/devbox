// The provisioning owner supplies a committed, disposable installed namespace.
// This adapter owns only the API Studio process that it launches.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, realpath, lstat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { packagedIdentity, fileDigest } from "./suite-user-flow-results.mjs";
import { freePort, connect, waitForRenderer } from "./workspace-cdp-fixture.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import {
  allWindowsProcesses, stopOwnedProcess, windowsProcessIsElevated,
  inspectElevatedCdpPolicy, installElevatedCdpPolicy, releaseCdpSession,
} from "./windows-packaged-smoke.mjs";

export async function verifyApiInstallation(directory, assets, sourceSha) {
  const identity = await packagedIdentity(assets, sourceSha);
  assert.ok(directory, "Owned installation root required");
  const root = await realpath(directory);
  assert.ok(path.basename(path.dirname(root)).startsWith("devbox-suite-delivery-"));
  const manifest = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
  const registration = JSON.parse(await readFile(path.join(root, "suite-registration.json"), "utf8"));
  const payload = JSON.parse(await readFile(path.join(root, "suite-payload.json"), "utf8"));
  assert.equal(payload.sourceSha, identity.sourceSha);
  assert.equal(registration.schemaVersion, 1);
  assert.match(registration.installationKey ?? "", /^[a-f0-9]{64}$/);
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocolVersion, 1);
  assert.match(manifest.generation ?? "", /^[a-zA-Z0-9_-]+$/);
  const member = manifest.members.find(item => item.product === "api-studio");
  assert.equal(member?.executable, `generations/${manifest.generation}/products/api-studio/devbox-api-studio.exe`);
  const executable = await realpath(path.join(root, member.executable));
  assert.equal(executable.toLowerCase(), path.resolve(root, member.executable).toLowerCase());
  assert.ok(!(await lstat(executable)).isSymbolicLink());
  const release = JSON.parse(await readFile(path.join(assets, "release-manifest.json"), "utf8"));
  const image = release.products.find(item => item.id === "api-studio").files.find(item => item.name === "devbox-api-studio.exe");
  const digest = await fileDigest(executable);
  assert.equal(digest, member.sha256);
  assert.equal(digest, image.sha256, "Installed API image differs from exact candidate");
  return { root, executable, installationKey: registration.installationKey, manifest, ...identity };
}

export async function createApiUserFlowContext() {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const installed = await verifyApiInstallation(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT,
    process.env.DEVBOX_USER_FLOW_ASSETS, process.env.GITHUB_SHA);
  const port = await freePort();
  const policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(installed.executable), port) : null;
  if (policy) installElevatedCdpPolicy(policy);
  let child, cdp, processIdentity, windowOwner;
  let closed = false;
  const closeWindow = async () => {
    assert.ok(processIdentity && child && child.exitCode === null, "Owned API window unavailable");
    assert.ok(allWindowsProcesses().some(item => item.Pid === child.pid &&
      item.Path?.toLowerCase() === installed.executable.toLowerCase()), "API process identity changed");
    nativeWindowAction(windowOwner,"Close");
    const deadline = Date.now() + 15000;
    while (child.exitCode === null && Date.now() < deadline) await delay(100);
    assert.notEqual(child.exitCode, null, "API native window did not close");
  };
  const close = async () => {
    if (closed) return;
    closed = true;
    try {
      if (child && processIdentity && child.exitCode === null) {
        const stopped = await stopOwnedProcess(processIdentity, installed.executable, child);
        assert.equal(stopped.forced, false, "API Studio required forced termination");
      }
    } finally {
      if (cdp) releaseCdpSession({ cdp, policy });
      else if (policy) releaseCdpSession({ policy });
    }
  };
  try {
    const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
    for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
    child = spawn(installed.executable, [], { cwd: path.dirname(installed.executable), env, stdio: "ignore", windowsHide: true });
    await once(child, "spawn");
    processIdentity = allWindowsProcesses().find(item => item.Pid === child.pid &&
      path.resolve(item.Path).toLowerCase() === installed.executable.toLowerCase());
    assert.ok(processIdentity, "API process identity unavailable");
    windowOwner = captureWindowOwner(processIdentity, installed.root);
    cdp = await connect(port, child);
    await waitForRenderer(cdp, 'Boolean(document.querySelector(\'nav[aria-label="제품 화면"]\'))', "API user-flow boot");
    const ui = createUiDriver({ cdp, evidenceRoot: "product-foundation-evidence/user-flows/screenshots/api", closeOwnedWindow: closeWindow });
    const context = { ...installed, fixtureRoot: installed.root, ui, cdp, close, child,
      namespace: path.join(process.env.LOCALAPPDATA, `com.devbox.v08.apistudio.i${installed.installationKey}`) };
    context.chooseFile = filePath => nativeWindowAction(windowOwner,"ChooseFile",{filePath});
    context.saveFile = filePath => nativeWindowAction(windowOwner,"SaveFile",{filePath});
    context.nativeCall = async (command, method, args, route = "requests") => {
      // Fixture preparation/observation retains the actual authenticated envelope.
      const result = await context.cdp.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');return invoke(${JSON.stringify(command)},{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+15000,route:${JSON.stringify(route)},context:d.context},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}});})()`);
      assert.equal(result.operation.outcome.state,"succeeded","Native fixture operation rejected");
      return result.value;
    };
    context.document = async kind => {
      const stored = await context.nativeCall("plugin:api-studio|store","load",{kind});
      return stored ? {...stored,value:JSON.parse(stored.body)} : null;
    };
    context.seedDocument = async (kind, value) => {
      const current = await context.document(kind);
      await context.nativeCall("plugin:api-studio|store","save",{kind,body:JSON.stringify(value),expectedRevision:current?.revision ?? null});
      const stored = await context.document(kind);
      assert.deepEqual(stored.value,value,"Prepared document bytes differ");
      return stored;
    };
    const restart = async () => {
      await context.ui.closeOwnedWindow();
      await context.close();
      Object.assign(context,await createApiUserFlowContext());
      context.restart = restart;
      return context;
    };
    context.restart = restart;
    return context;
  } catch (error) {
    await close();
    throw error;
  }
}
