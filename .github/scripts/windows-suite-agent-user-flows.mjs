// Installer checkpoints and Suite Agent acceptance use one owned candidate installation.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, writeFile, readdir, mkdir, realpath } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { freePort, connect } from "./workspace-cdp-fixture.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { fileDigest, packagedIdentity, writeUserFlowResults } from "./suite-user-flow-results.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import {
  allWindowsProcesses,
  windowsLocalAppData,
  windowsProcessIsElevated,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
  stopOwnedProcess,
} from "./windows-packaged-smoke.mjs";
import { focusWindowsCdpHost } from "./windows-cdp-host.mjs";
import { createInstalledProductContext } from "./windows-suite-ui-context.mjs";
import { workspaceRequestExpression } from "./windows-workspace-registration.mjs";
import { fileURLToPath } from "node:url";

const json = async (file) => JSON.parse((await readFile(file, "utf8")).replace(/^\uFEFF/u, ""));
export async function until(check, label, ms = 30000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const result = await check();
      if (result) return result;
    } catch {}
    await delay(100);
  }
  throw new Error(label);
}
async function verifiedScope(context) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const { identity, installationKey, manifest } = context,
    root = await realpath(context.root);
  assert.match(identity?.sourceSha ?? "", /^[a-f0-9]{40}$/u);
  assert.match(installationKey ?? "", /^[a-f0-9]{64}$/u);
  const registration = await json(path.join(root, "suite-registration.json")),
    payload = await json(path.join(root, "suite-payload.json"));
  assert.equal(registration.installationKey, installationKey);
  assert.equal(payload.sourceSha, identity.sourceSha);
  assert.equal((await json(path.join(root, "devbox-installation.json"))).installationId, manifest.installationId);
  return { ...context, root };
}
const agentProcesses = (context) =>
  allWindowsProcesses().filter(
    (p) =>
      p.Path?.toLowerCase().startsWith(context.root.toLowerCase() + path.sep) &&
      p.Path.toLowerCase().endsWith(path.join("resources", "suite", "devbox-agent.exe").toLowerCase()),
  );
async function storeCount(directory, depth = 0) {
  let count = 0;
  if (depth > 8) throw new Error("Owned store metadata exceeds bound");
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return 0;
    throw error;
  }
  assert.ok(entries.length <= 256, "Owned store metadata exceeds bound");
  for (const entry of entries) {
    assert.ok(!entry.isSymbolicLink(), "Linked fixture namespace forbidden");
    if (entry.isFile() && entry.name.endsWith(".db")) count++;
    else if (entry.isDirectory() && !/webview|ebwebview|logs|cache/i.test(entry.name))
      count += await storeCount(path.join(directory, entry.name), depth + 1);
  }
  return count;
}
async function stores(context) {
  const base = windowsLocalAppData(),
    result = {};
  for (const [product, namespace] of [
    ["workspace", "workspace"],
    ["api-studio", "apistudio"],
    ["knowledge", "knowledge"],
  ])
    result[product] = await storeCount(path.join(base, `com.devbox.v08.${namespace}.i${context.installationKey}`));
  return result;
}
export async function beforeAgentProductPreparation(input) {
  const context = await verifiedScope(input),
    counts = await stores(context);
  assert.ok(
    Object.values(counts).every((count) => count === 0),
    "Fresh product stores must be absent before first preparation",
  );
  assert.equal(agentProcesses(context).length, 0, "Setup must not launch an unprepared Agent");
  const paused = await context.center.cdp.evaluate(
    "document.body.innerText.includes('설치 준비가 끝나면 백그라운드 서비스를 연결합니다.')",
  );
  assert.equal(paused, true);
  return {
    schemaVersion: 1,
    installationKey: context.installationKey,
    sourceSha: context.identity.sourceSha,
    installationId: context.manifest.installationId,
    stage: "absent",
    counts,
  };
}
export async function afterAgentProductPreparation(input) {
  const context = await verifiedScope(input),
    checkpoint = input.checkpoint;
  assert.equal(checkpoint?.stage, "absent");
  assert.equal(checkpoint.installationId, context.manifest.installationId);
  const counts = await stores(context);
  assert.ok(
    Object.values(counts).every((count) => count > 0),
    "Every product must prepare its native store",
  );
  assert.equal(agentProcesses(context).length, 0, "Preparation remains paused before commit");
  const text = await context.center.cdp.evaluate("document.body.innerText");
  assert.ok(text.includes("준비 기록 4/4") || text.split("응답·저장소 선택 확인됨").length - 1 === 4);
  return { ...checkpoint, stage: "prepared", preparedCounts: counts };
}
export async function launchOwnedProduct(context, product) {
  const member = context.manifest.members.find((m) => m.product === product);
  assert.ok(member);
  const executable = await realpath(path.join(context.root, member.executable));
  assert.ok(executable.startsWith(context.root + path.sep));
  assert.equal(await fileDigest(executable), member.sha256);
  const port = context.ports?.[product] ?? (await freePort()),
    policy =
      !context.ports?.[product] && windowsProcessIsElevated()
        ? inspectElevatedCdpPolicy(path.basename(executable), port)
        : null;
  if (policy) installElevatedCdpPolicy(policy);
  const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
  for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
  const child = spawn(executable, [], { cwd: path.dirname(executable), env, stdio: "ignore" });
  await once(child, "spawn");
  const identity = await until(
    () => allWindowsProcesses().find((p) => p.Pid === child.pid && p.Path?.toLowerCase() === executable.toLowerCase()),
    "owned product identity",
  );
  const item = { product, identity, executable, child, policy, cdp: null };
  try {
    item.cdp = await connect(port, child);
    item.windowOwner = captureWindowOwner(identity, context.root);
    await focusWindowsCdpHost(identity);
    const ui = createUiDriver({
      cdp: item.cdp,
      evidenceRoot: `product-foundation-evidence/user-flows/screenshots/agent-${product}`,
      closeOwnedWindow: async () => nativeWindowAction(item.windowOwner, "Close"),
    });
    item.ui = ui;
    await until(
      () => item.cdp.evaluate("!!document.querySelector('nav[aria-label=\"제품 화면\"]')"),
      "real product UI ready",
    );
    return item;
  } catch (error) {
    await stopOwnedProcess(identity, executable, child);
    releaseCdpSession(item);
    throw error;
  }
}
export async function closeOwnedProduct(item, force = false) {
  try {
    if (item.child?.exitCode === null) {
      if (force) await stopOwnedProcess(item.identity, item.executable, item.child);
      else {
        await item.ui.closeOwnedWindow();
        let reviewed = false;
        await until(async () => {
          if (item.child.exitCode !== null) return true;
          if (
            item.product === "workspace" &&
            !reviewed &&
            (await item.cdp.evaluate("document.body.innerText")).includes("Workspace 종료 검토")
          ) {
            await item.ui.click({
              role: "button",
              name: "종료",
              scope: { role: "dialog", name: "Workspace 종료 검토" },
            });
            reviewed = true;
          }
          return false;
        }, "normal owned product close");
      }
    }
  } finally {
    releaseCdpSession(item);
  }
}
export async function afterAgentCommit(input) {
  const context = await verifiedScope(input),
    checkpoint = input.checkpoint,
    opened = [],
    screenshots = [];
  const record = (status, assertions, failureCode = null) => ({
    id: "AGENT-01",
    status,
    ...context.identity,
    evidenceKind: "packaged-ui",
    assertions,
    screenshotPaths: screenshots,
    failureCode,
  });
  let result;
  try {
    assert.equal(checkpoint?.stage, "prepared");
    assert.equal(checkpoint.installationId, context.manifest.installationId);
    assert.equal((await json(path.join(context.root, "devbox-activation.json"))).phase, "committed");
    const launches = await Promise.allSettled(
      ["workspace", "api-studio", "knowledge"].map((product) => launchOwnedProduct(context, product)),
    );
    for (const launch of launches) {
      if (launch.status === "fulfilled") opened.push(launch.value);
    }
    for (const launch of launches)
      assert.equal(launch.status, "fulfilled", "Every simultaneous first product launch must succeed");
    const apps = [context.center, ...opened];
    for (const app of apps)
      await until(
        async () =>
          (await app.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')")) ===
          "connected",
        "first native Agent connection",
      );
    const agent = agentProcesses(context);
    assert.equal(agent.length, 1, "Concurrent first clients must share one Agent owner");
    for (const app of opened) {
      assert.ok(!(await app.cdp.evaluate("document.body.innerText")).includes("백그라운드 서비스 연결 안 됨"));
      screenshots.push(await app.ui.screenshot(`AGENT-01-${app.product}`));
    }
    result = record("PASS", [
      "Fresh native product stores absent and setup UI paused Agent before preparation",
      "All four product stores were prepared before commit; concurrent actual first product launches connected after commit",
      "All connected product windows share exactly one installed Agent PID/creation owner",
    ]);
  } catch (error) {
    result = record("FAIL", [String(error.message).slice(0, 300)], "agent-first-connect-failed");
  } finally {
    for (const app of opened) {
      try {
        await closeOwnedProduct(app);
      } catch (error) {
        result = record(
          "FAIL",
          ["Normal first-connect product close failed", String(error.message).slice(0, 200)],
          "agent-product-close-failed",
        );
        await closeOwnedProduct(app, true);
      }
    }
  }
  await writeUserFlowResults("agent-initialization", [result]);
  assert.equal(result.status, "PASS");
  return result;
}

export async function observeAgentUpdateQuiesce(input) {
  const context = await verifiedScope(input);
  assert.equal(input.action, "actual-control-center-update");
  assert.equal(input.actualUpdateHealthGate, true);
  assert.equal(input.previousInstallationId, context.manifest.installationId);
  assert.ok(input.reviewScreenshot, "Actual update review screenshot required");
  const body = await context.center.cdp.evaluate("document.body.innerText");
  assert.ok(
    body.includes("설치 준비") || body.includes("준비 기록"),
    "Actual Control Center health gate must remain visible",
  );
  assert.equal(agentProcesses(context).length, 0, "Update quiesce must stop the existing Agent");
  for (let poll = 0; poll < 5; poll++) {
    await context.center.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')");
    assert.equal(agentProcesses(context).length, 0, "Reads must not restart Agent during update quiesce");
    await delay(500);
  }
  const receipt = {
    ...context.identity,
    installationKey: context.installationKey,
    updateQuiesced: true,
    previousInstallationId: input.previousInstallationId,
    proofScope: "actual-candidate-update-review-and-health-gate",
    screenshotPaths: [relativeScreenshot(input.reviewScreenshot)],
  };
  await mkdir("product-foundation-evidence/user-flows/agent-hooks", { recursive: true });
  await writeFile(
    "product-foundation-evidence/user-flows/agent-hooks/update-quiesce.json",
    JSON.stringify(receipt, null, 2),
    { flag: "wx" },
  );
  return receipt;
}

const nativeStatus = (app) =>
  app.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')");
async function reconnectUi(app) {
  await until(
    async () => (await app.cdp.evaluate("document.body.innerText")).includes("백그라운드 서비스 연결 안 됨"),
    "disconnected UI",
  );
  await app.ui.click({ role: "button", name: "백그라운드 서비스 다시 연결" });
  await until(async () => (await nativeStatus(app)) === "connected", "explicit UI reconnect");
}
async function workspaceRead(app, component, method, args = {}) {
  const value = await app.cdp.evaluate(workspaceRequestExpression(component, method, args));
  assert.equal(value?.operation?.outcome?.state, "succeeded");
  return value.value;
}
async function agentCrashBusiness(context, app) {
  const registry = await workspaceRead(app, "workspace.registry", "snapshot");
  const root = registry.worktrees.find((item) => item.binding?.target?.kind === "windows");
  assert.ok(root, "An existing owned Windows worktree is required");
  await app.ui.click({ role: "button", name: "개요" });
  await app.ui.click({
    role: "button",
    name: "프로젝트 선택",
    scope: { role: "region", name: path.basename(root.binding.root) },
  });
  const folder = path.join(process.env.RUNNER_TEMP, `devbox-agent-business-${randomUUID()}`);
  await mkdir(folder);
  const counter = path.join(folder, "launch-count.txt"),
    script = path.join(folder, "owned.cjs"),
    name = `Agent recovery ${randomUUID()}`;
  await writeFile(
    script,
    `require('node:fs').appendFileSync(${JSON.stringify(counter)},'launch\\n');setInterval(()=>{},1000);`,
  );
  await app.ui.click({ role: "button", name: "작업 및 서비스" });
  await app.ui.click({ role: "button", name: "+ 새 작업" });
  await app.ui.fill({ role: "textbox", name: "작업 이름" }, name);
  await app.ui.fill({ role: "textbox", name: "실행 명령" }, `"${process.execPath}" "${script}"`);
  await app.ui.fill({ role: "textbox", name: "작업 디렉터리" }, folder);
  await app.ui.click({ role: "button", name: "작업 저장" });
  const job = await until(
    async () => (await workspaceRead(app, "workspace.runtime", "list_jobs")).find((item) => item.name === name),
    "saved business job",
  );
  const scope = { role: "article", name };
  await app.cdp.command("Debugger.enable");
  const fn = await app.cdp.command("Runtime.evaluate", {
    expression: "window.__TAURI_INTERNALS__.runCallback",
    returnByValue: false,
  });
  assert.ok(fn.result.objectId);
  let paused = false;
  const unlisten = app.cdp.onEvent("Debugger.paused", () => {
    paused = true;
  });
  await app.cdp.command("Debugger.setBreakpointOnFunctionCall", {
    objectId: fn.result.objectId,
    condition: `data?.value?.jobId===${JSON.stringify(job.id)}`,
  });
  const click = app.ui.click({ role: "button", name: "지금 실행", scope });
  click.catch(() => {});
  try {
    await until(() => paused, "business reply paused");
    await until(async () => (await readFile(counter, "utf8")) === "launch\n", "single business effect before crash");
    const pending = await app.cdp.evaluate(
      `Object.keys(localStorage).filter(k=>k.startsWith('devbox-runtime-pending:')&&k.endsWith(':'+${JSON.stringify(job.id)})).map(k=>JSON.parse(localStorage.getItem(k)))`,
    );
    assert.equal(pending.length, 1);
    const agents = agentProcesses(context);
    assert.equal(agents.length, 1);
    const crashOwner = captureWindowOwner(agents[0], context.root);
    const image64 = Buffer.from(crashOwner.identity.Path).toString("base64"),
      started64 = Buffer.from(crashOwner.started).toString("base64");
    const crashed = spawnSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `$ErrorActionPreference='Stop';$p=[Diagnostics.Process]::GetProcessById(${crashOwner.identity.Pid});$decode=[Text.Encoding]::UTF8;if($p.MainModule.FileName -ne $decode.GetString([Convert]::FromBase64String('${image64}')) -or $p.StartTime.ToUniversalTime().ToString('o') -ne $decode.GetString([Convert]::FromBase64String('${started64}'))){throw 'Owned Agent identity changed'};Stop-Process -Id ${crashOwner.identity.Pid} -Force -ErrorAction Stop`,
      ],
      { encoding: "utf8", timeout: 10000 },
    );
    assert.equal(crashed.status, 0, "Exact owned Agent crash failed");
    await until(() => agentProcesses(context).length === 0, "owned Agent crash completed");
    assert.equal(agentProcesses(context).length, 0);
    await app.cdp.command("Debugger.disable");
    await click;
    await reconnectUi(app);
    assert.equal(agentProcesses(context).length, 1);
    const receipt = await workspaceRead(app, "workspace.runtime", "runtime_control_status", {
      operationId: pending[0].operationId,
    });
    assert.equal(receipt?.jobId, job.id, "Original durable operation must remain queryable");
    assert.equal(
      await readFile(counter, "utf8"),
      "launch\n",
      "Uncertain operation must not replay after Agent reconnect",
    );
    await app.ui.click({ role: "button", name: "지금 실행", scope });
    await until(
      async () => (await readFile(counter, "utf8")) === "launch\nlaunch\n",
      "new explicit business request after reconnect",
    );
    await app.ui.click({ role: "button", name: "중지", scope });
    return {
      assertions: [
        "Owned Agent crashed with actual business receipt paused while Workspace stayed open",
        "Original operation was queried after real reconnect without replay; only new explicit business request produced second effect",
      ],
      screenshotPaths: [await app.ui.screenshot("AGENT-02-reconnected-business")],
    };
  } finally {
    unlisten();
    await app.cdp.command("Debugger.disable").catch(() => {});
  }
}
async function trayQuitReconnect(context, app) {
  const agents = agentProcesses(context);
  assert.equal(agents.length, 1);
  const owner = captureWindowOwner(agents[0], context.root);
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-File",
      path.resolve(".github/scripts/windows-agent-tray-ui.ps1"),
      "-AgentProcessId",
      String(agents[0].Pid),
      "-ExpectedExecutable",
      agents[0].Path,
      "-ExpectedStartTimeUtc",
      owner.started,
    ],
    { encoding: "utf8", timeout: 20000 },
  );
  assert.equal(result.status, 0, `Owned tray input failed: ${(result.stderr ?? "").slice(0, 300)}`);
  await until(() => agentProcesses(context).length === 0, "tray shutdown");
  for (let poll = 0; poll < 5; poll++) {
    assert.notEqual(await nativeStatus(app), "connected");
    assert.equal(agentProcesses(context).length, 0, "Product read polling must not defeat tray shutdown");
    await delay(500);
  }
  await reconnectUi(app);
  assert.equal(agentProcesses(context).length, 1);
  return {
    assertions: [
      "Exact owned Agent tray menu was clicked with real Windows mouse input",
      "Existing product read polls left Agent stopped until explicit reconnect button launched one owner",
    ],
    screenshotPaths: [await app.ui.screenshot("AGENT-03-tray-reconnect")],
  };
}
export async function runInstalledAgentUserFlows() {
  const identity = await packagedIdentity(),
    root = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT),
    registration = await json(path.join(root, "suite-registration.json")),
    manifest = await json(path.join(root, "devbox-installation.json"));
  const context = await verifiedScope({ root, manifest, identity, installationKey: registration.installationKey });
  const records = [],
    app = await createInstalledProductContext("workspace");
  try {
    for (const [id, run] of [
      ["AGENT-02", () => agentCrashBusiness(context, app)],
      ["AGENT-03", () => trayQuitReconnect(context, app)],
    ]) {
      try {
        records.push({
          id,
          status: "PASS",
          ...identity,
          evidenceKind: "packaged-ui",
          ...(await run()),
          failureCode: null,
        });
      } catch (error) {
        records.push({
          id,
          status: "FAIL",
          ...identity,
          evidenceKind: "packaged-ui",
          assertions: [String(error.message).slice(0, 300)],
          screenshotPaths: [await app.ui.screenshot(`${id}-failure`).catch(() => "")].filter(Boolean),
          failureCode: "agent-ui-scenario-failed",
        });
      }
    }
  } finally {
    const previous = agentProcesses(context);
    await app.close();
    const current = agentProcesses(context);
    if (
      previous.length === 1 &&
      current.length === 1 &&
      previous[0].Pid === current[0].Pid &&
      previous[0].Created === current[0].Created
    ) {
      try {
        const portable = await json("product-foundation-evidence/user-flows/agent-hooks/portable-close.json"),
          update = await json("product-foundation-evidence/user-flows/agent-hooks/update-quiesce.json");
        validateAgentOwnershipReceipts(context, portable, update);
        records.push({
          id: "AGENT-04",
          status: "PASS",
          ...identity,
          evidenceKind: "packaged-ui",
          assertions: [
            "Installed Workspace normal X preserved the same installed Agent PID and creation owner",
            "Actual portable Workspace X exited its local owner while installed Agent stayed unchanged",
            "Actual Control Center candidate update review and health gate kept Agent stopped under read polling; full published-version download is separate delivery evidence",
          ],
          screenshotPaths: [...portable.screenshotPaths, ...update.screenshotPaths],
          failureCode: null,
        });
      } catch (error) {
        records.push({
          id: "AGENT-04",
          status: error.code === "ENOENT" ? "NOT_RUN" : "FAIL",
          ...identity,
          evidenceKind: "packaged-ui",
          assertions: [String(error.message).slice(0, 300)],
          screenshotPaths: [],
          failureCode: "portable-and-update-ui-proof-required",
        });
      }
    } else
      records.push({
        id: "AGENT-04",
        status: "FAIL",
        ...identity,
        evidenceKind: "packaged-ui",
        assertions: ["Installed normal X changed or stopped the installed Agent owner"],
        screenshotPaths: [],
        failureCode: "installed-X-owner-changed",
      });
  }
  await writeUserFlowResults("agent", records);
  assert.ok(
    records.every((record) => record.status === "PASS"),
    "Agent acceptance incomplete; inspect actual evidence",
  );
  return records;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  await runInstalledAgentUserFlows();

export async function observePortableAgentBeforeClose(context) {
  assert.equal(process.platform, "win32");
  assert.equal(context.product, "workspace");
  const installedRoot = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT),
    portableRoot = await realpath(context.root);
  assert.notEqual(portableRoot.toLowerCase(), installedRoot.toLowerCase());
  assert.equal(
    context.processIdentity.Path.toLowerCase(),
    path.join(portableRoot, "devbox-workspace.exe").toLowerCase(),
  );
  assert.equal(
    await nativeStatus(context),
    "unsupported",
    "Portable Workspace must own its local runtime instead of installed Agent",
  );
  const agents = agentProcesses({ root: installedRoot });
  assert.equal(agents.length, 1);
  return {
    sourceSha: (await packagedIdentity()).sourceSha,
    installedAgent: agents[0],
    portableOwner: context.processIdentity,
    screenshotPaths: [await context.ui.screenshot("AGENT-04-portable-local-owner-before-X")],
  };
}
export async function observePortableAgentAfterClose(context, proof) {
  const installedRoot = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT),
    alive = allWindowsProcesses();
  assert.equal(
    alive.some((item) => item.Pid === proof.portableOwner.Pid && item.Created === proof.portableOwner.Created),
    false,
    "Portable local runtime owner must exit after actual X",
  );
  const agents = agentProcesses({ root: installedRoot });
  assert.equal(agents.length, 1);
  assert.equal(agents[0].Pid, proof.installedAgent.Pid);
  assert.equal(agents[0].Created, proof.installedAgent.Created);
  const identity = await packagedIdentity();
  assert.equal(identity.sourceSha, proof.sourceSha);
  const registration = await json(path.join(installedRoot, "suite-registration.json"));
  const receipt = {
    ...identity,
    installationKey: registration.installationKey,
    portableLocalOwnerExited: true,
    installedAgentUnchanged: true,
    screenshotPaths: proof.screenshotPaths.map(relativeScreenshot),
  };
  await mkdir("product-foundation-evidence/user-flows/agent-hooks", { recursive: true });
  await writeFile(
    "product-foundation-evidence/user-flows/agent-hooks/portable-close.json",
    JSON.stringify(receipt, null, 2),
    { flag: "wx" },
  );
  return receipt;
}

export function validateAgentOwnershipReceipts(context, portable, update) {
  for (const proof of [portable, update]) {
    assert.equal(proof.sourceSha, context.identity.sourceSha);
    assert.equal(proof.fixtureSha, context.identity.fixtureSha);
    assert.deepEqual(proof.artifactDigests, context.identity.artifactDigests);
    assert.equal(proof.installationKey, context.installationKey);
    assert.ok(Array.isArray(proof.screenshotPaths) && proof.screenshotPaths.length > 0);
    for (const image of proof.screenshotPaths)
      assert.ok(
        image.startsWith("product-foundation-evidence/user-flows/screenshots/") && !image.includes(".."),
        "Owned relative evidence screenshot required",
      );
  }
  assert.equal(portable.portableLocalOwnerExited, true);
  assert.equal(portable.installedAgentUnchanged, true);
  assert.equal(update.updateQuiesced, true);
  assert.equal(update.previousInstallationId, context.manifest.installationId);
  assert.equal(update.proofScope, "actual-candidate-update-review-and-health-gate");
}

function relativeScreenshot(image) {
  const relative = path.relative(process.cwd(), path.resolve(image)).split(path.sep).join("/");
  assert.ok(
    relative.startsWith("product-foundation-evidence/user-flows/screenshots/") && !relative.includes(".."),
    "Screenshot outside owned evidence",
  );
  return relative;
}
