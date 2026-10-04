// Interactive installer and Control Center journey; no setup/activation IPC shortcuts.
import assert from "node:assert/strict";
import {
  writeInstallerFailure,
  reportInstallerResults,
  readInstallerOperations,
} from "./windows-suite-installer-evidence.mjs";
import { ownedNsisSpawnOptions } from "./windows-suite-installer-actions.mjs";
import path from "node:path";
import { mkdir, appendFile, copyFile, readFile, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { observeControlCenterInput } from "./windows-control-center-input.mjs";
import {
  beforeAgentProductPreparation,
  afterAgentProductPreparation,
  afterAgentCommit,
} from "./windows-suite-agent-user-flows.mjs";
import { observeInstalledProductLayout, observeProductPerformance } from "./windows-suite-layout.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { packagedIdentity, writeUserFlowResults, fileDigest } from "./suite-user-flow-results.mjs";
import {
  captureWindowOwner,
  nativeWindowAction,
  measureWarmOwnedWindow,
  ownedProductCohort,
} from "./windows-user-flow-window.mjs";
import {
  allWindowsProcesses,
  Cdp,
  unusedPort,
  waitForCdp,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
  stopOwnedProcess,
} from "./windows-packaged-smoke.mjs";
export const scenarioIds = ["INSTALL-01", "INSTALL-02"];
async function until(check, label, ms = 60000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const value = await check();
    if (value) return value;
    await delay(150);
  }
  throw new Error(`Interactive installer timed out: ${label}`);
}
export async function run() {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const identity = await packagedIdentity();
  const scratch = path.join(process.env.RUNNER_TEMP, `devbox-suite-delivery-${randomUUID().replaceAll("-", "")}`);
  await mkdir(scratch);
  const root = path.join(scratch, "Suite UI Fixture");
  process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = root;
  await appendFile(process.env.GITHUB_ENV, `DEVBOX_USER_FLOW_INSTALL_ROOT=${root}\n`);
  const assets = path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS);
  const release = JSON.parse(await readFile(path.join(assets, "release-manifest.json"), "utf8"));
  const setup = path.join(scratch, release.setup.name);
  await copyFile(path.join(assets, release.setup.name), setup);
  const ports = {},
    policies = [];
  let port, env;
  let center = null,
    manifest = null,
    registration = null,
    installer = null,
    agentCheckpoint = null,
    coldRendererReadyMs = null,
    installerOwner = null,
    lastInstallerInspection = null,
    stage = "cdp-policy-preparation";
  const checkpoint = async (next) => {
    stage = next;
    await mkdir("product-foundation-evidence", { recursive: true });
    await appendFile(
      "product-foundation-evidence/interactive-installer-stages.jsonl",
      `${JSON.stringify({ stage, at: new Date().toISOString() })}\n`,
    );
  };
  const installerInspectionErrors = [];
  const inspectInstaller = (owner) => {
    try {
      const view = nativeWindowAction(owner, "Inspect");
      lastInstallerInspection = { stage, at: new Date().toISOString(), view, error: null };
      return view;
    } catch (error) {
      lastInstallerInspection = {
        stage,
        at: new Date().toISOString(),
        view: lastInstallerInspection?.view ?? null,
        error: { message: String(error.message).slice(0, 1000), stack: String(error.stack ?? "").slice(0, 3000) },
      };
      installerInspectionErrors.push(lastInstallerInspection);
      if (installerInspectionErrors.length > 2000) installerInspectionErrors.shift();
      throw error;
    }
  };
  const screenshots = [],
    results = [],
    products = new Map();
  const record = (id, status, assertions, failureCode = null) => ({
    id,
    status,
    ...identity,
    evidenceKind: "packaged-ui",
    assertions,
    screenshotPaths: [...screenshots],
    failureCode,
  });
  const imagePath = (product) =>
    path.resolve(root, manifest.members.find((member) => member.product === product).executable);
  const processFor = (product) =>
    allWindowsProcesses().filter((item) => item.Path.toLowerCase() === imagePath(product).toLowerCase());
  const attach = async (previous) => {
    const process = await until(
      () => processFor("control-center").find((p) => p.Created !== previous?.Created),
      "Control Center launch",
    );
    const target = await waitForCdp(port, "Devbox Control Center");
    const cdp = new Cdp(target.webSocketDebuggerUrl ?? target);
    await cdp.connect();
    await cdp.send("Page.enable");
    const owner = captureWindowOwner(process, scratch);
    const ui = createUiDriver({
      cdp: { command: cdp.send.bind(cdp) },
      evidenceRoot: "product-foundation-evidence/user-flows/screenshots/installer",
      closeOwnedWindow: async () => nativeWindowAction(owner, "Close"),
    });
    center = { process, processIdentity: process, owner, cdp, ui };
    await until(() => cdp.evaluate('document.body.innerText.includes("데이터")'), "Control Center ready");
    return center;
  };
  const closeProduct = async (product) => {
    for (const process of processFor(product)) {
      nativeWindowAction(captureWindowOwner(process, scratch), "Close");
      const attached = products.get(product);
      if (product === "workspace" && attached) {
        await until(async () => {
          if (!allWindowsProcesses().some((p) => p.Pid === process.Pid && p.Created === process.Created)) return true;
          if (
            await attached.cdp.evaluate(
              '!!document.querySelector(\'[role="dialog"][aria-label="Workspace 종료 검토"]\')',
            )
          ) {
            await attached.ui.click({
              role: "button",
              name: "종료",
              scope: { role: "dialog", name: "Workspace 종료 검토" },
            });
            return true;
          }
          return false;
        }, "Workspace close review");
      }
      await until(
        () => !allWindowsProcesses().some((p) => p.Pid === process.Pid && p.Created === process.Created),
        `${product} normal close`,
      );
      attached?.cdp.close();
      products.delete(product);
    }
  };
  const attachProduct = async (product) => {
    if (products.has(product)) return products.get(product);
    const target = await waitForCdp(
      ports[product],
      `Devbox ${product === "api-studio" ? "API Studio" : product === "workspace" ? "Workspace" : "Knowledge"}`,
    );
    const cdp = new Cdp(target.webSocketDebuggerUrl ?? target);
    await cdp.connect();
    const processIdentity = processFor(product)[0];
    const owner = captureWindowOwner(processIdentity, scratch);
    const ui = createUiDriver({
      cdp: { command: cdp.send.bind(cdp) },
      evidenceRoot: "product-foundation-evidence/user-flows/screenshots/installer",
      closeOwnedWindow: async () => nativeWindowAction(owner, "Close"),
    });
    const result = {
      cdp: { command: cdp.send.bind(cdp), evaluate: cdp.evaluate.bind(cdp), close: cdp.close.bind(cdp) },
      ui,
      processIdentity,
      root,
      product,
    };
    products.set(product, result);
    return result;
  };
  const captureLayout = async (state) => {
    for (const product of ["workspace", "api-studio", "knowledge"])
      await observeInstalledProductLayout({ ...(await attachProduct(product)), state });
    await observeInstalledProductLayout({
      cdp: { command: center.cdp.send.bind(center.cdp), evaluate: center.cdp.evaluate.bind(center.cdp) },
      ui: center.ui,
      processIdentity: center.process,
      root,
      product: "control-center",
      state,
    });
  };
  const agentInput = () => ({
    root,
    manifest,
    identity,
    installationKey: registration.installationKey,
    ports,
    center: {
      ...center,
      cdp: { command: center.cdp.send.bind(center.cdp), evaluate: center.cdp.evaluate.bind(center.cdp) },
    },
  });
  const text = () => center.cdp.evaluate("document.body.innerText");
  const openProducts = async (products) => {
    for (const product of products) {
      await center.ui.click({
        role: "button",
        name: `Devbox ${product === "api-studio" ? "API Studio" : product === "workspace" ? "Workspace" : "Knowledge"} 열기`,
      });
      await until(() => processFor(product).length === 1, `${product} visible launch`);
    }
  };
  const health = async () => {
    await center.ui.click({ role: "button", name: "네 제품 상태 확인" });
    await until(async () => !(await text()).includes("제품 확인 중…"), "health observation");
  };
  const captureGenuineRecoverStage = async () => {
    const previous = center.process,
      originalInstallation = manifest.installationId,
      originalGeneration = manifest.generation;
    await attachProduct("workspace");
    await closeProduct("workspace");
    await center.ui.closeOwnedWindow();
    await until(() => processFor("control-center").length === 0, "Center close before native recovery preparation");
    center.cdp.close();
    const payloadPath = path.join(
        root,
        "setup",
        await fileDigest(path.join(root, "suite-payload.json")),
        "suite-payload.json",
      ),
      payload = JSON.parse(await readFile(payloadPath, "utf8"));
    const helper = path.join(path.dirname(payloadPath), "devbox-suite-bootstrap.exe");
    const helperEntry = payload.products
      .find((product) => product.id === "control-center")
      .files.find((file) => file.name === "resources/suite/devbox-suite-bootstrap.exe");
    assert.equal(await fileDigest(helper), helperEntry.sha256);
    async function prepareStage(mode, expected) {
      const process = spawn(helper, [mode, root, payloadPath], { env, stdio: ["ignore", "pipe", "pipe"] });
      await once(process, "spawn");
      process.stdout.resume();
      process.stderr.resume();
      const timeout = setTimeout(() => process.kill(), 180000);
      let code;
      try {
        [code] = await once(process, "exit");
      } finally {
        clearTimeout(timeout);
      }
      assert.equal(code, 0, `Native fixture preparation ${mode} failed`);
      assert.equal(JSON.parse(await readFile(path.join(root, "devbox-activation.json"), "utf8")).phase, expected);
    }
    await prepareStage("--recover-install", "recover");
    let launch = spawn(imagePath("control-center"), ["--suite-setup"], { env, stdio: "ignore" });
    await once(launch, "spawn");
    await attach(previous);
    await openProducts(["workspace", "api-studio", "knowledge"]);
    await captureLayout("recover");
    for (const product of ["workspace", "api-studio", "knowledge"]) await closeProduct(product);
    const recoverCenter = center.process;
    await center.ui.closeOwnedWindow();
    await until(() => processFor("control-center").length === 0, "recover Center normal close");
    center.cdp.close();
    await prepareStage("--restart-install", "import");
    manifest = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
    assert.equal(manifest.installationId, originalInstallation);
    assert.notEqual(manifest.generation, originalGeneration);
    const freshRegistration = JSON.parse(await readFile(path.join(root, "suite-registration.json"), "utf8"));
    assert.equal(freshRegistration.installationKey, registration.installationKey);
    await writeFile(
      path.join(scratch, "recover-layout-native-preparation.json"),
      JSON.stringify({
        ...identity,
        installationKey: registration.installationKey,
        evidenceKind: "native-boundary",
        fixturePreparation: true,
        methods: ["--recover-install", "--restart-install"],
        originalGeneration,
        restartedGeneration: manifest.generation,
        assertions: [
          "Supported native recovery creates a real rendered Recover phase without data-restore writer blocks",
          "Restart retains physical installation and existing prepared stores; earliest Agent absent checkpoint is preserved",
        ],
      }),
      { flag: "wx" },
    );
    launch = spawn(imagePath("control-center"), ["--suite-setup"], { env, stdio: "ignore" });
    await once(launch, "spawn");
    await attach(recoverCenter);
    await openProducts(["workspace"]);
  };
  const advance = async (expected) => {
    const previous = center.process;
    for (const product of ["workspace", "api-studio", "knowledge"]) await closeProduct(product);
    await center.ui.click({ role: "button", name: "다음 단계" });
    await center.ui.click({ role: "checkbox", name: "선택한 작업과 제품 종료를 확인했습니다." });
    await center.ui.click({ role: "button", name: "Control Center를 닫고 실행" });
    await until(
      async () => JSON.parse(await readFile(path.join(root, "devbox-activation.json"), "utf8")).phase === expected,
      `activation ${expected}`,
    );
    center.cdp.close();
    await attach(previous);
    if (expected !== "committed") await center.ui.click({ role: "button", name: "데이터 및 복구" });
  };
  try {
    for (const product of ["workspace", "api-studio", "knowledge", "control-center"]) {
      ports[product] = await unusedPort();
      const policy = inspectElevatedCdpPolicy(`devbox-${product}.exe`, ports[product]);
      policies.push(policy);
      installElevatedCdpPolicy(policy);
    }
    port = ports["control-center"];
    env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
    for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
    await checkpoint("installer-spawn");
    // NSIS consumes its final /D argument unquoted, including spaces (Docs/Chapter3.html).
    installer = spawn(setup, [`/D=${root}`], {
      env,
      stdio: "ignore",
      windowsHide: false,
      ...ownedNsisSpawnOptions(setup, [`/D=${root}`]),
    });
    await once(installer, "spawn");
    await checkpoint("installer-process-identity");
    const installerIdentity = await until(
      () => allWindowsProcesses().find((p) => p.Pid === installer.pid && p.Path.toLowerCase() === setup.toLowerCase()),
      "installer process",
    );
    const owner = captureWindowOwner(installerIdentity, scratch);
    installerOwner = owner;
    await checkpoint("installer-welcome-ready");
    await until(() => {
      try {
        return inspectInstaller(owner).buttons?.some((b) => b.id === "1" && b.enabled && b.visible);
      } catch {
        return false;
      }
    }, "installer welcome");
    await checkpoint("installer-welcome-next");
    nativeWindowAction(owner, "Invoke", { controlId: "1" });
    await checkpoint("installer-directory-ready");
    await until(() => {
      try {
        return inspectInstaller(owner).controls?.some((b) => b.id === "1019" && b.enabled && b.visible);
      } catch {
        return false;
      }
    }, "installer directory");
    await checkpoint("installer-directory-install");
    nativeWindowAction(owner, "Invoke", { controlId: "1" });
    await checkpoint("installer-registration-ready");
    await until(
      async () => {
        try {
          const value = JSON.parse(await readFile(path.join(root, "suite-registration.json"), "utf8"));
          if (/^[a-f0-9]{64}$/.test(value.installationKey)) {
            registration = value;
            return true;
          }
        } catch {}
        return false;
      },
      "installer registration",
      180000,
    );
    await checkpoint("installer-owner-receipt");
    manifest = JSON.parse(await readFile(path.join(root, "devbox-installation.json"), "utf8"));
    await writeFile(
      path.join(scratch, "user-flow-owner.json"),
      JSON.stringify({
        schemaVersion: 1,
        root,
        sourceSha: identity.sourceSha,
        installationKey: registration.installationKey,
        staging: path.resolve("candidate/delivery"),
      }),
    );
    await checkpoint("installer-finish-ready");
    await until(() => {
      try {
        return inspectInstaller(owner).controls?.some((b) => b.name === "Devbox 설치 준비 완료" && b.visible);
      } catch {
        return false;
      }
    }, "installer finish");
    await checkpoint("installer-finish-open-center");
    const coldStart = performance.now();
    nativeWindowAction(owner, "Invoke", { controlId: "1" });
    await checkpoint("installed-center-attach");
    await attach();
    await checkpoint("installed-product-preparation");
    coldRendererReadyMs = performance.now() - coldStart;
    await checkpoint("installed-import-screenshot");
    const initialViewport = await center.cdp.evaluate(`(() => ({
      visibility: document.visibilityState, width: innerWidth, height: innerHeight,
      bodyWidth: document.body?.getBoundingClientRect().width ?? 0,
      bodyHeight: document.body?.getBoundingClientRect().height ?? 0,
      readyState: document.readyState
    }))()`);
    await writeFile("product-foundation-evidence/interactive-initial-viewport.json", JSON.stringify(initialViewport));
    screenshots.push(await center.ui.screenshot("interactive-import"));
    await checkpoint("installed-agent-before-preparation");
    agentCheckpoint = await beforeAgentProductPreparation(agentInput());
    await checkpoint("installed-workspace-preparation");
    await openProducts(["workspace"]);
    await checkpoint("installed-partial-health");
    await health();
    assert.ok((await text()).includes("응답 또는 저장소를 확인하지 못함"));
    assert.ok(!(await text()).includes("다음 단계"), "Unprepared products must block activation");
    const before = JSON.parse(await readFile(path.join(root, "devbox-activation.json"), "utf8"));
    const previous = center.process;
    await checkpoint("installed-center-close-for-resume");
    const closeWindowView = nativeWindowAction(center.owner, "Inspect");
    await writeFile(
      "product-foundation-evidence/interactive-center-close-window.json",
      JSON.stringify({
        stage,
        sourceSha: identity.sourceSha,
        windowCount: closeWindowView.windowCount,
        selectedWindowCount: closeWindowView.selectedWindowCount,
        windows: closeWindowView.windows,
        nativeWindowCount: closeWindowView.nativeWindowCount,
        nativeWindows: closeWindowView.nativeWindows,
      }),
    );
    await center.ui.closeOwnedWindow();
    await until(() => processFor("control-center").length === 0, "cancel and close setup");
    center.cdp.close();
    // Reopen the same installed executable, like the user's normal shortcut.
    await checkpoint("installed-center-reopen");
    const reopened = spawn(imagePath("control-center"), [], { env, stdio: "ignore", windowsHide: false });
    await once(reopened, "spawn");
    await attach(previous);
    assert.deepEqual(JSON.parse(await readFile(path.join(root, "devbox-activation.json"), "utf8")), before);
    assert.equal(processFor("workspace").length, 1, "Prepared product remains open across Center restart");
    screenshots.push(await center.ui.screenshot("interactive-resume"));
    results.push(
      record("INSTALL-02", "PASS", [
        "One prepared product cannot activate an incomplete installation",
        "Normal Control Center close and reopen preserved activation revision and existing prepared product",
      ]),
    );
    await captureGenuineRecoverStage();
    await openProducts(["api-studio", "knowledge"]);
    await until(async () => {
      await health();
      return (await text()).split("응답·저장소 선택 확인됨").length - 1 === 4;
    }, "four prepared products");
    await center.ui.click({ role: "button", name: "상태 기록" });
    await until(
      async () => (await text()).includes("준비 기록 4/4") && (await text()).includes("다음 단계"),
      "recorded preparation",
    );
    screenshots.push(await center.ui.screenshot("interactive-prepared"));
    await captureLayout("import");
    agentCheckpoint = await afterAgentProductPreparation({ ...agentInput(), checkpoint: agentCheckpoint });
    await advance("health");
    await openProducts(["workspace", "api-studio", "knowledge"]);
    await until(async () => {
      await health();
      return (await text()).split("응답·저장소 선택 확인됨").length - 1 === 4;
    }, "fresh native health");
    await center.ui.click({ role: "button", name: "상태 기록" });
    await until(async () => (await text()).includes("다음 단계"), "fresh health recorded");
    screenshots.push(await center.ui.screenshot("interactive-health"));
    await captureLayout("health");
    await advance("committed");
    const description = await center.cdp.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:product-shell|describe")');
    assert.equal(description.deliveryState, "committed");
    screenshots.push(await center.ui.screenshot("interactive-committed"));
    await afterAgentCommit({ ...agentInput(), checkpoint: agentCheckpoint });
    const centerTransport = {
      command: center.cdp.send.bind(center.cdp),
      evaluate: center.cdp.evaluate.bind(center.cdp),
    };
    const warmExistingWindowMs = await measureWarmOwnedWindow(center.owner, centerTransport);
    const taskStart = performance.now();
    await center.ui.click({ role: "button", name: "데이터 및 복구" });
    await until(async () => (await text()).includes("현재 데이터 보존"), "committed recovery usable");
    const completeMs = performance.now() - taskStart;
    await observeProductPerformance({
      product: "control-center",
      cdp: centerTransport,
      getIdentities: () => ownedProductCohort(center.process),
      coldRendererReadyMs,
      warmExistingWindowMs,
      workload: { taskCompleted: true, completeMs },
    });
    await observeControlCenterInput({
      cdp: { command: center.cdp.send.bind(center.cdp), evaluate: center.cdp.evaluate.bind(center.cdp) },
      ui: center.ui,
    });
    results.push(
      record("INSTALL-01", "PASS", [
        "Interactive NSIS Welcome/Directory/Install/Finish launched Control Center",
        "Visible product launch, health record and reviewed actions committed the same installed generation",
        "Reopened Control Center admits ordinary product work after commit",
      ]),
    );
  } catch (error) {
    for (const id of scenarioIds)
      if (!results.some((row) => row.id === id))
        results.push(record(id, "FAIL", [String(error.message).slice(0, 500)], "interactive-installation-failed"));
    console.error(`Original interactive installer failure at ${stage}:`, error);
    let observation = { lastInstallerInspection };
    try {
      if (installerOwner)
        observation = {
          kind: "owned-nsis-uia",
          lastInstallerInspection,
          window: nativeWindowAction(installerOwner, "Inspect"),
        };
      else
        observation = {
          kind: "owned-installer-process",
          lastInstallerInspection,
          spawned: Boolean(installer),
          exitCode: installer?.exitCode ?? null,
          signalCode: installer?.signalCode ?? null,
        };
    } catch (inspectionError) {
      observation = {
        kind: "owned-nsis-uia-unavailable",
        lastInstallerInspection,
        error: String(inspectionError.message).slice(0, 500),
      };
    }
    if (center) {
      try {
        observation.centerWindow = nativeWindowAction(center.owner, "Inspect");
      } catch {
        observation.centerWindow = { unavailable: true };
      }
      try {
        screenshots.push(await center.ui.screenshot("interactive-first-failure"));
      } catch (captureError) {
        console.error("Owned Center failure capture unavailable:", captureError.message);
      }
    }
    if (registration?.installationKey) {
      try {
        observation.operations = await readInstallerOperations(process.env.LOCALAPPDATA, registration.installationKey);
      } catch {
        observation.operationLogs = "unavailable";
      }
    }
    try {
      await writeInstallerFailure({ stage, error, identity, results, observation });
    } catch (reportError) {
      console.error("Original installer failure preserved above; failure evidence write failed:", reportError);
    }
  } finally {
    const cleanupFailures = [];
    if (manifest)
      for (const product of ["workspace", "api-studio", "knowledge", "control-center"]) {
        try {
          await closeProduct(product);
        } catch (error) {
          cleanupFailures.push(`${product}: ${String(error.message).slice(0, 160)}`);
          for (const owned of processFor(product)) {
            try {
              await stopOwnedProcess(owned, imagePath(product), {
                get exitCode() {
                  return allWindowsProcesses().some(
                    (p) => p.Pid === owned.Pid && p.Created === owned.Created && p.Path === owned.Path,
                  )
                    ? null
                    : 0;
                },
                signalCode: null,
              });
            } catch (cleanup) {
              cleanupFailures.push(String(cleanup.message).slice(0, 160));
            }
          }
        }
      }
    if (cleanupFailures.length) {
      for (const result of results) {
        result.status = "FAIL";
        result.failureCode = "owned-installer-cleanup-failed";
        result.assertions.push(...cleanupFailures);
      }
    }
    if (center) center.cdp.close();
    for (const policy of policies.reverse()) releaseCdpSession({ policy });
    await writeFile(
      path.join(scratch, "interactive-owner.json"),
      JSON.stringify({ root, sourceSha: identity.sourceSha, installationKey: registration?.installationKey ?? null }),
    );
  }
  return results;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const results = await run();
  await reportInstallerResults(results, (records) => writeUserFlowResults("installer", records));
  if (results.some((row) => row.status !== "PASS")) process.exitCode = 1;
}
