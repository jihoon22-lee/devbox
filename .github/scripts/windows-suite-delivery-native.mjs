import {
  projectInitializationDiagnostics,
  projectConnectionDiagnostics,
  projectHandoffDiagnostics,
} from "./agent-runtime-diagnostics.mjs";
import { exerciseAgentCollectors } from "./windows-agent-collectors.mjs";
import { exerciseAgentWebhooks } from "./windows-agent-webhooks.mjs";
import { exerciseAgentRuntime } from "./windows-agent-runtime.mjs";
// Actual installed products, native owner observations and activation gating.
// The PowerShell fixture owns the random installation and namespace cleanup.
import { requireHostedNetworkFixture } from "./fixture-network-safety.mjs";
import { freePort, connect, waitForRenderer } from "./workspace-cdp-fixture.mjs";
import {
  allWindowsProcesses,
  stopOwnedProcess,
  windowsProcessIsElevated,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
} from "./windows-packaged-smoke.mjs";
import assert from "node:assert/strict";
import {
  readFileSync,
  writeFileSync,
  mkdirSync,
  realpathSync,
  readdirSync,
  lstatSync,
  openSync,
  readSync,
  closeSync,
} from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
requireHostedNetworkFixture();
assert.equal(process.platform, "win32");
const [directory, mode] = process.argv.slice(2);
assert.ok(["import", "health", "committed"].includes(mode));
const root = realpathSync.native(directory);
assert.ok(path.basename(path.dirname(root)).startsWith("devbox-suite-delivery-"));
const manifest = JSON.parse(readFileSync(path.join(root, "devbox-installation.json"), "utf8"));
const evidence = {
  source: JSON.parse(readFileSync(path.join(root, "suite-payload.json"), "utf8")).sourceSha,
  productSources: JSON.parse(process.env.DEVBOX_SUITE_PRODUCT_SOURCES ?? "null"),
  fixtureSource: process.env.GITHUB_SHA,
  mode,
  installationId: manifest.installationId,
  generation: manifest.generation,
  checks: {},
  result: "failed",
  cleanup: [],
};
const live = [];
const value = (result) => {
  assert.equal(result.operation.outcome.state, "succeeded", JSON.stringify(result));
  return result.value;
};
function captureDiagnostics() {
  // Only the hosted fixture's verified installation namespace is inspected.
  // Keep fixed metadata, never complete logs or arbitrary fields.
  try {
    const registration = JSON.parse(readFileSync(path.join(root, "suite-registration.json"), "utf8"));
    assert.match(registration.installationKey, /^[0-9a-f]{64}$/);
    for (const [namespace, field, project] of [
      ["workspace", "runtimeDiagnostics", projectInitializationDiagnostics],
      ["agent", "agentConnectionDiagnostics", projectConnectionDiagnostics],
      ["workspace", "workspaceHandoffDiagnostics", projectHandoffDiagnostics],
      ["apistudio", "apiHandoffDiagnostics", projectHandoffDiagnostics],
    ]) {
      try {
        const logs = path.join(
          process.env.LOCALAPPDATA,
          `com.devbox.v08.${namespace}.i${registration.installationKey}`,
          "logs",
        );
        assert.ok(lstatSync(logs).isDirectory() && !lstatSync(logs).isSymbolicLink());
        evidence[field] = [];
        for (const name of readdirSync(logs)
          .filter((name) => /^operations-\d{4}-\d{2}-\d{2}\.jsonl$/.test(name))
          .sort()
          .slice(-2)) {
          const file = path.join(logs, name),
            info = lstatSync(file);
          if (!info.isFile() || info.isSymbolicLink()) continue;
          const buffer = Buffer.alloc(Math.min(info.size, 65536)),
            fd = openSync(file, "r");
          try {
            const bytes = readSync(fd, buffer, 0, buffer.length, Math.max(0, info.size - buffer.length));
            evidence[field].push(...project(buffer.subarray(0, bytes).toString("utf8")));
          } finally {
            closeSync(fd);
          }
        }
        evidence[field] = evidence[field].slice(namespace === "agent" ? -96 : -32);
      } catch {
        evidence[`${field}Unavailable`] = true;
      }
    }
  } catch {
    evidence.runtimeDiagnosticsUnavailable = true;
  }
}
async function call(item, command, body, route) {
  return item.cdp.evaluate(
    `(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke('plugin:product-shell|describe');const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:${JSON.stringify(route)},context:d.context};try{return await invoke(${JSON.stringify(command)},{request:{header,...${JSON.stringify(body)}}});}catch(problem){throw new Error(JSON.stringify(problem).slice(0,2000));}})()`,
    { timeoutMs: 35000 },
  );
}
async function start(member) {
  const executable = realpathSync.native(path.join(root, member.executable)),
    port = await freePort();
  const policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
  const item = { product: member.product, executable, policy, child: null, cdp: null };
  live.push(item);
  if (policy) installElevatedCdpPolicy(policy);
  const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
  for (const name of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(name)) delete env[name];
  item.child = spawn(executable, [], { cwd: path.dirname(executable), env, stdio: ["ignore", "ignore", "pipe"] });
  item.child.stderr.setEncoding("utf8");
  item.child.stderr.on("data", (text) => {
    item.error = ((item.error ?? "") + text).slice(-2000);
  });
  await once(item.child, "spawn");
  item.identity = allWindowsProcesses().find(
    (row) =>
      row.Pid === item.child.pid && path.resolve(row.Path).toLowerCase() === path.resolve(executable).toLowerCase(),
  );
  assert.ok(item.identity);
  item.cdp = await connect(port, item.child);
  await waitForRenderer(item.cdp, "!!window.__TAURI_INTERNALS__", "installed product bridge missing");
  await waitForRenderer(
    item.cdp,
    "!!document.querySelector('nav[aria-label=\"제품 화면\"]')",
    "installed shell missing",
  );
  assert.equal(
    (await item.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')")).deliveryState,
    mode,
  );
  return item;
}
try {
  const apps = {};
  const startupOrder =
    mode === "committed"
      ? [...manifest.members].sort(
          (a, b) => Number(b.product === "control-center") - Number(a.product === "control-center"),
        )
      : manifest.members;
  for (const member of startupOrder) {
    apps[member.product] = await start(member);
    if (mode === "committed" && member.product === "control-center") {
      const agentImage = realpathSync.native(
        path.join(root, "generations", manifest.generation, "products/control-center/resources/suite/devbox-agent.exe"),
      );
      const owners = () =>
        allWindowsProcesses().filter(
          (row) => path.resolve(row.Path).toLowerCase() === path.resolve(agentImage).toLowerCase(),
        );
      const deadline = Date.now() + 20000;
      while (owners().length === 0 && Date.now() < deadline) await delay(100);
      assert.equal(
        owners().length,
        1,
        "opening Control Center must start the agent before any explicit business request",
      );
      evidence.checks.productReadyStartsAgent = true;
    }
  }
  for (const item of Object.values(apps)) {
    const route = { workspace: "overview", "api-studio": "requests", knowledge: "notes", "control-center": "recovery" }[
      item.product
    ];
    const review = value(await call(item, "plugin:suite|connection", { method: { kind: "preview" } }, route));
    assert.equal(review.products.length, 4);
    assert.equal(review.installationId, manifest.installationId);
    assert.ok(review.products.every((product) => product.available));
    // Health/reinstall/update reopen a remembered native connection. Preview
    // checks this exact package; approving an already connected bus is rejected.
    const status = value(await call(item, "plugin:suite|connection", { method: { kind: "status" } }, route));
    if (!status.connected)
      value(
        await call(
          item,
          "plugin:suite|connection",
          { method: { kind: "approve", token: review.token, remember: true } },
          route,
        ),
      );
    const connected = value(await call(item, "plugin:suite|connection", { method: { kind: "status" } }, route));
    assert.equal(connected.connected, true);
    assert.equal(connected.generation, review.generation);
    evidence.checks[`connection_${item.product}`] = true;
  }
  const center = apps["control-center"];
  if (mode === "import") {
    const rejected = await call(
      center,
      "plugin:control-center|delivery",
      { method: "open_installation_folder", args: { path: root } },
      "products",
    ).then(
      () => false,
      () => true,
    );
    assert.equal(rejected, true);
    assert.equal(
      value(
        await call(
          center,
          "plugin:control-center|delivery",
          { method: "open_installation_folder", args: {} },
          "products",
        ),
      ).opened,
      true,
    );
    evidence.checks.verifiedInstallationFolderOnly = true;
  }
  if (mode !== "committed") {
    for (const member of manifest.members) {
      const readyDeadline = Date.now() + 30000;
      let ready = false;
      while (Date.now() < readyDeadline) {
        const status = value(
          await call(
            center,
            "plugin:suite|connection",
            { method: { kind: "readHealthStatus", product: member.product } },
            "recovery",
          ),
        );
        if (status.nativeStoreReady) {
          ready = true;
          break;
        }
        await delay(100);
      }
      assert.ok(ready, `native store preparation timed out: ${member.product}`);
      const method = "record_suite_health";
      const result = value(
        await call(center, "plugin:control-center|delivery", { method, args: { product: member.product } }, "recovery"),
      );
      assert.equal(result.recorded, true);
      evidence.checks[`recorded_${member.product}`] = true;
    }
    const blocked = await call(
      apps.workspace,
      "plugin:workspace|runtime",
      { method: "list_jobs", args: {} },
      "tasks",
    ).catch(() => null);
    assert.ok(
      blocked === null || blocked.operation.outcome.state !== "succeeded",
      "ordinary Runtime remains blocked before commit",
    );
    evidence.checks.businessGate = true;
    assert.equal(
      allWindowsProcesses().some(
        (row) =>
          path.resolve(row.Path).toLowerCase() ===
          path
            .resolve(
              path.join(
                root,
                "generations",
                manifest.generation,
                "products/control-center/resources/suite/devbox-agent.exe",
              ),
            )
            .toLowerCase(),
      ),
      false,
      "activation health/import must not retain an agent writer",
    );
    evidence.checks.noAgentBeforeCommit = true;
  } else {
    for (const member of manifest.members) {
      const result = value(
        await call(
          center,
          "plugin:suite|connection",
          { method: { kind: "readHealthStatus", product: member.product } },
          "recovery",
        ),
      );
      assert.equal(result.nativeStoreReady, true);
    }
    evidence.checks.fourCommittedNativeOwners = true;
    // A real Workspace owner request starts the installed background service.
    value(await call(apps.workspace, "plugin:workspace|runtime", { method: "list_jobs", args: {} }, "tasks"));
    const agentImage = realpathSync.native(
      path.join(root, "generations", manifest.generation, "products/control-center/resources/suite/devbox-agent.exe"),
    );
    const agents = () =>
      allWindowsProcesses().filter(
        (row) => path.resolve(row.Path).toLowerCase() === path.resolve(agentImage).toLowerCase(),
      );
    const deadline = Date.now() + 15000;
    while (agents().length === 0 && Date.now() < deadline) await delay(100);
    assert.equal(agents().length, 1, "one installed agent serves the four products");
    evidence.agent = agents()[0];
    const fixture = path.join(path.dirname(root), "agent-runtime-" + randomUUID());
    mkdirSync(fixture);
    const agentIdentity = () => {
      const rows = agents();
      assert.equal(rows.length, 1, "exactly one verified fixture agent required");
      return rows[0];
    };
    const sameProcess = (identity) =>
      allWindowsProcesses().some(
        (row) =>
          row.Pid === identity.Pid &&
          row.Created === identity.Created &&
          path.resolve(row.Path).toLowerCase() === path.resolve(identity.Path).toLowerCase(),
      );
    const crashOwnedAgent = async () => {
      const identity = agentIdentity();
      const observed = {
        get exitCode() {
          return sameProcess(identity) ? null : 0;
        },
        signalCode: null,
      };
      const result = await stopOwnedProcess(identity, agentImage, observed);
      assert.equal(result.forced, true, "acceptance must prove abrupt agent loss");
    };
    const result = await exerciseAgentRuntime({
      workspace: apps.workspace,
      directory: fixture,
      agentIdentity,
      closeWorkspace: async (item) => {
        const stopped = await stopOwnedProcess(item.identity, item.executable, item.child);
        assert.equal(stopped.forced, false, "Workspace close must drain and exit instead of hiding");
        assert.equal(sameProcess(item.identity), false, "Workspace must finish ordinary owner shutdown");
        releaseCdpSession(item);
      },
      restartWorkspace: async () => {
        apps.workspace = await start(manifest.members.find((member) => member.product === "workspace"));
        return apps.workspace;
      },
      crashAgent: crashOwnedAgent,
      report: (state) => {
        evidence.checks.agentRuntime = { ...state };
        console.log(`Agent runtime acceptance: ${state.stage}`);
      },
    });
    evidence.checks.agentRuntime = result.evidence;
    const webhookResult = await exerciseAgentWebhooks({
      api: apps["api-studio"],
      connectionStatus: async (item) =>
        value(await call(item, "plugin:suite|connection", { method: { kind: "status" } }, "requests")),
      call: async (item, method, args) =>
        value(await call(item, "plugin:api-studio|webhooks", { method, args }, "webhooks")),
      closeApi: async (item) => {
        const stopped = await stopOwnedProcess(item.identity, item.executable, item.child);
        assert.equal(stopped.forced, false, "API Studio close must honor its selected policy and exit");
        assert.equal(sameProcess(item.identity), false, "API Studio must finish its selected close policy");
        releaseCdpSession(item);
      },
      restartApi: async () => {
        apps["api-studio"] = await start(manifest.members.find((member) => member.product === "api-studio"));
        return apps["api-studio"];
      },
      crashAgent: crashOwnedAgent,
      report: (state) => {
        evidence.checks.agentWebhooks = { ...state };
      },
    });
    evidence.checks.agentWebhooks = webhookResult.evidence;
    const collectorResult = await exerciseAgentCollectors({
      knowledge: apps.knowledge,
      directory: fixture,
      agentIdentity,
      call: async (item, component, method, args) =>
        value(
          await call(
            item,
            `plugin:knowledge|${component}`,
            { method, args },
            component === "activity" ? "activity" : "search",
          ),
        ),
      closeKnowledge: async (item) => {
        const stopped = await stopOwnedProcess(item.identity, item.executable, item.child);
        assert.equal(stopped.forced, false, "Knowledge close must finish the unsaved-note review and exit");
        assert.equal(sameProcess(item.identity), false);
        releaseCdpSession(item);
      },
      restartKnowledge: async () => {
        apps.knowledge = await start(manifest.members.find((member) => member.product === "knowledge"));
        return apps.knowledge;
      },
      report: (state) => {
        evidence.checks.agentCollectors = { ...state };
      },
    });
    evidence.checks.agentCollectors = collectorResult.evidence;
    const agentSetting = async (product, method, args = {}) =>
      value(
        await call(
          apps[product],
          product === "knowledge" ? "plugin:knowledge|activity" : "plugin:control-center|tools",
          { method, args },
          product === "knowledge" ? "activity" : "environment",
        ),
      );
    assert.deepEqual(await agentSetting("control-center", "autostart_status"), { supported: true, enabled: false });
    try {
      assert.deepEqual(await agentSetting("control-center", "set_autostart", { enabled: true }), {
        supported: true,
        enabled: true,
      });
      assert.deepEqual(await agentSetting("knowledge", "autostart_status"), { supported: true, enabled: true });
      assert.deepEqual(await agentSetting("knowledge", "set_autostart", { enabled: false }), {
        supported: true,
        enabled: false,
      });
      assert.deepEqual(await agentSetting("control-center", "autostart_status"), { supported: true, enabled: false });
      evidence.checks.sharedAgentLoginSetting = true;
    } finally {
      // This disposable installation owns the preference just created above.
      await agentSetting("control-center", "set_autostart", { enabled: false });
    }
    // All existing native clients observe a deliberate stop. Background reads
    // must not undo the user's choice; an explicit reconnect may start it again.
    for (const item of Object.values(apps)) {
      assert.equal(
        await item.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_reconnect')", {
          timeoutMs: 35000,
        }),
        "connected",
      );
    }
    const relayEnvironment = { ...process.env };
    for (const key of Object.keys(relayEnvironment))
      if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete relayEnvironment[key];
    const relay = spawn(center.executable, ["--stop-agent-for-update"], {
      env: relayEnvironment,
      windowsHide: true,
      stdio: "ignore",
    });
    await once(relay, "spawn");
    const relayExit = await Promise.race([once(relay, "exit"), delay(15000).then(() => null)]);
    if (!relayExit) {
      relay.kill();
      await Promise.race([once(relay, "exit"), delay(5000)]);
      assert.fail("owned shutdown relay timed out");
    }
    assert.equal(relayExit[0], 0);
    assert.equal(agents().length, 0);
    for (const item of Object.values(apps)) {
      assert.equal(
        await item.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_status')"),
        "unavailable",
      );
    }
    for (const [product, command, method, route, issue] of [
      ["workspace", "plugin:workspace|runtime", "list_jobs", "tasks", "runtime_agent_unavailable"],
      ["api-studio", "plugin:api-studio|webhooks", "server_status", "webhooks", "webhook_agent_unavailable"],
      ["knowledge", "plugin:knowledge|activity", "is_tracking", "activity", "knowledge_agent_unavailable"],
    ]) {
      const refused = await call(apps[product], command, { method, args: {} }, route);
      assert.equal(refused.operation.outcome.state, "failed");
      assert.equal(refused.value.issue, issue);
    }
    await delay(2500);
    assert.equal(agents().length, 0, "background queries relaunched an intentionally stopped owner");
    assert.equal(
      await apps.workspace.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|agent_reconnect')", {
        timeoutMs: 35000,
      }),
      "connected",
    );
    value(
      await call(apps["api-studio"], "plugin:api-studio|webhooks", { method: "server_status", args: {} }, "webhooks"),
    );
    value(await call(apps.knowledge, "plugin:knowledge|activity", { method: "is_tracking", args: {} }, "activity"));
    assert.equal(agents().length, 1);
    evidence.checks.intentionalAgentStopRequiresExplicitRestart = true;
    evidence.checks.otherProductsReconnectToTheRestartedOwner = true;
  }
  evidence.result = "passed";
} catch (error) {
  evidence.failure = String(error).slice(0, 3000);
  captureDiagnostics();
  process.exitCode = 1;
} finally {
  for (const item of live.reverse()) {
    // Request ordinary window shutdown first, including owner cleanup. Forceful
    // fixture cleanup is restricted to the captured synthetic process identity.
    try {
      await item.cdp
        ?.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})")
        .catch(() => {});
      await delay(400);
    } catch {}
    item.cdp?.close();
    item.cdp = null;
    try {
      if (item.identity) await stopOwnedProcess(item.identity, item.executable, item.child);
      else if (item.child?.exitCode === null) {
        item.child.kill();
        await Promise.race([once(item.child, "exit"), delay(10000)]);
      }
    } catch (error) {
      evidence.cleanup.push({ product: item.product, issue: String(error) });
      evidence.result = "failed";
      process.exitCode = 1;
    } finally {
      releaseCdpSession(item);
    }
    evidence.cleanup.push({
      product: item.product,
      nativeError: item.error,
      exited:
        !item.identity ||
        !allWindowsProcesses().some((row) => row.Pid === item.identity.Pid && row.Created === item.identity.Created),
    });
  }
  if (!evidence.agentConnectionDiagnostics) captureDiagnostics();
  mkdirSync("product-foundation-evidence", { recursive: true });
  writeFileSync(
    `product-foundation-evidence/suite-delivery-${mode}-${Date.now()}.json`,
    JSON.stringify(evidence, null, 2),
  );
}
