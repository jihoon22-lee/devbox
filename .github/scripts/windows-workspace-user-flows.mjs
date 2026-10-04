import { waitForFixtureChildExit } from "./fixture-child-exit.mjs";
import { crashOwnedWorkspace } from "./windows-workspace-crash.mjs";
import assert from "node:assert/strict";
import { preserveUserFlowFailure } from "./user-flow-failure-evidence.mjs";
import { stopWorkspaceBeforeDisconnect } from "./windows-workspace-ui-observations.mjs";
import { readFile, realpath, mkdtemp, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import {
  packagedIdentity,
  writeUserFlowResults,
  fileDigest,
  installedFixtureIdentity,
} from "./suite-user-flow-results.mjs";
import { observeProductPerformance } from "./windows-suite-layout.mjs";
import {
  captureWindowOwner,
  nativeWindowAction,
  measureWarmOwnedWindow,
  ownedProductCohort,
} from "./windows-user-flow-window.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { createWorkspaceUiFixture } from "./windows-workspace-ui-fixture.mjs";
import { run as draftFlows } from "./windows-workspace-draft-ui.mjs";
import { run as runtimeFlows } from "./windows-workspace-runtime-recovery-ui.mjs";
import { run as agentFlows } from "./windows-workspace-agent-registry-ui.mjs";
import { createWorkspaceLspProxy } from "./windows-workspace-lsp.mjs";
import { freePort, connect } from "./workspace-cdp-fixture.mjs";
import {
  allWindowsProcesses,
  stopOwnedProcess,
  windowsProcessIsElevated,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
} from "./windows-packaged-smoke.mjs";
export async function runWorkspaceUserFlows() {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const identity = await packagedIdentity();
  const root = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT);
  assert.match(path.basename(path.dirname(root)), /^devbox-suite-delivery-[a-f0-9]{32}$/u);
  assert.equal(path.basename(root), "Suite UI Fixture");
  assert.ok(path.dirname(root).startsWith(path.resolve(process.env.RUNNER_TEMP) + path.sep));
  const json = async (name) => JSON.parse((await readFile(path.join(root, name), "utf8")).replace(/^\uFEFF/u, ""));
  const registration = await json("suite-registration.json"),
    manifest = await json("devbox-installation.json");
  assert.match(registration.installationKey, /^[a-f0-9]{64}$/);
  assert.equal(registration.installationKey, await installedFixtureIdentity());
  assert.equal((await json("devbox-activation.json")).phase, "committed");
  assert.equal((await json("suite-payload.json")).sourceSha, identity.sourceSha);
  const member = manifest.members.find((item) => item.product === "workspace");
  assert.ok(member);
  assert.equal(member.executable, `generations/${manifest.generation}/products/workspace/devbox-workspace.exe`);
  const executable = await realpath(path.join(root, member.executable));
  assert.ok(path.relative(root, executable) && !path.relative(root, executable).startsWith(".."));
  assert.equal(await fileDigest(executable), member.sha256);
  const release = JSON.parse(
    await readFile(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"), "utf8"),
  );
  assert.equal(
    member.sha256,
    release.products
      .find((product) => product.id === "workspace")
      .files.find((file) => file.name === "devbox-workspace.exe").sha256,
  );
  const dataRoot = path.join(process.env.LOCALAPPDATA, `com.devbox.v08.workspace.i${registration.installationKey}`);
  const fixtureRoot = await mkdtemp(path.join(root, "workspace-ui-"));
  const port = await freePort();
  const network = await createWorkspaceLspProxy();
  let child, attached, owner, policy, coldRendererReadyMs;
  const cdp = {
    command: (...args) => attached.command(...args),
    evaluate: (...args) => attached.evaluate(...args),
    onEvent: (...args) => attached.onEvent(...args),
  };
  async function launch() {
    if (!policy && windowsProcessIsElevated()) {
      policy = inspectElevatedCdpPolicy(path.basename(executable), port);
      installElevatedCdpPolicy(policy);
    }
    const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
    Object.assign(env, {
      HTTP_PROXY: network.url,
      HTTPS_PROXY: network.url,
      ALL_PROXY: network.url,
      http_proxy: network.url,
      https_proxy: network.url,
      all_proxy: network.url,
      NO_PROXY: "127.0.0.1,localhost",
      no_proxy: "127.0.0.1,localhost",
    });
    for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
    const started = performance.now();
    child = spawn(executable, [], { cwd: path.dirname(executable), env, stdio: ["ignore", "ignore", "pipe"] });
    child.stderr.resume();
    await once(child, "spawn");
    owner = allWindowsProcesses().find(
      (item) => item.Pid === child.pid && path.resolve(item.Path).toLowerCase() === executable.toLowerCase(),
    );
    assert.ok(owner, "Owned Workspace process identity absent");
    attached = await connect(port, child);
    const deadline = performance.now() + 30000;
    while (!(await attached.evaluate('Boolean(document.querySelector(".product-shell > main"))'))) {
      assert.ok(performance.now() < deadline, "Workspace renderer ready timeout");
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    coldRendererReadyMs = performance.now() - started;
  }
  async function closeOwnedWindow() {
    assert.ok(
      child.exitCode === null &&
        allWindowsProcesses().some(
          (item) => item.Pid === owner.Pid && item.Created === owner.Created && item.Path === owner.Path,
        ),
    );
    const command = `$p=Get-Process -Id ${owner.Pid} -ErrorAction Stop; if(-not $p.CloseMainWindow()){throw 'Owned main window unavailable'}`;
    assert.equal(
      spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
        encoding: "utf8",
        timeout: 10000,
      }).status,
      0,
    );
  }
  async function waitForExit() {
    await waitForFixtureChildExit(child, 20_000);
  }
  async function cleanup() {
    await stopWorkspaceBeforeDisconnect(
      async () => {
        if (child?.exitCode === null) await stopOwnedProcess(owner, executable, child);
      },
      () => attached?.close(),
    );
  }
  const ui = createUiDriver({
    cdp,
    evidenceRoot: "product-foundation-evidence/user-flows/screenshots/workspace",
    closeOwnedWindow,
  });
  const fixture = createWorkspaceUiFixture({
    ui,
    cdp,
    dataRoot,
    fixtureRoot,
    windowOwner: () => captureWindowOwner(owner, root),
    network,
    async terminalUi(id) {
      const transport = await connect(port, child, performance.now() + 30_000, id);
      const windowOwner = captureWindowOwner(owner, root);
      const driver = createUiDriver({
        cdp: transport,
        evidenceRoot: "product-foundation-evidence/user-flows/screenshots/workspace",
        closeOwnedWindow: () => nativeWindowAction(windowOwner, "Close", { auxiliaryWindow: "workspace-terminal" }),
      });
      return { ui: driver, cdp: transport, close: () => transport.close() };
    },
    async chooseArchive(file) {
      nativeWindowAction(captureWindowOwner(owner, root), "ChooseFile", { filePath: file });
    },
    waitForExit,
    cleanup,
    async restart(crash) {
      if (crash)
        await stopWorkspaceBeforeDisconnect(
          () => crashOwnedWorkspace({ owner, executable, child, waitForExit }),
          () => attached?.close(),
        );
      await launch();
    },
  });
  const results = [];
  const scenarioIds = ["WORK-01", "WORK-02", "WORK-03", "RUNTIME-01", "RUNTIME-02", "LSP-01", "DEPS-01"];
  try {
    await launch();
    await observeProductPerformance({
      product: "workspace",
      cdp,
      getIdentities: () => ownedProductCohort(owner),
      coldRendererReadyMs,
      warmExistingWindowMs: await measureWarmOwnedWindow(captureWindowOwner(owner, root), cdp),
      workload: () => fixture.performanceTask(),
    });
    results.push(...(await draftFlows({ ...identity, ui, cdp, fixtureRoot, workspaceFixture: fixture })));
    await launch();
    results.push(...(await agentFlows({ ...identity, ui, fixtureRoot, workspaceFixture: fixture })));
    await launch();
    results.push(...(await runtimeFlows({ ...identity, ui, fixtureRoot, workspaceFixture: fixture })));
  } catch (error) {
    await preserveUserFlowFailure("workspace", error, { ui: attached ? ui : null, identity }).catch(() => {
      console.error("Workspace original-failure evidence unavailable");
    });
    for (const id of scenarioIds)
      if (!results.some((result) => result.id === id))
        results.push({
          ...identity,
          id,
          status: "FAIL",
          evidenceKind: "packaged-ui",
          assertions: ["Owned Workspace fixture could not complete the required user flow"],
          screenshotPaths: [],
          failureCode: "workspace-owned-fixture-failed",
        });
  } finally {
    try {
      await fixture.dispose();
      if (fixture.windowsRoot) {
        await writeFile(
          path.join(path.dirname(root), "workspace-handoff-fixture.json"),
          JSON.stringify({
            ...identity,
            installationKey: registration.installationKey,
            root: fixture.windowsRoot,
            file: path.join(fixture.windowsRoot, "한글.txt"),
          }),
          { flag: "wx" },
        );
      }
      // Keep this owned synthetic Windows root for the subsequent installed
      // handoff journey. The common installation owner performs final cleanup.
      if (policy) releaseCdpSession({ policy, child, cdp: attached });
    } catch {
      for (const result of results)
        if (result.status === "PASS") {
          result.status = "FAIL";
          result.failureCode = "workspace-owned-cleanup-failed";
          result.assertions.push("Owned fixture cleanup did not complete");
        }
    } finally {
      network.close();
      // Refuse to replace the first result file, including cleanup failure.
      await writeUserFlowResults("workspace", results);
    }
  }
  assert.ok(
    results.every((result) => result.status === "PASS"),
    "Workspace mandatory user flows incomplete",
  );
  return results;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await runWorkspaceUserFlows();
}
