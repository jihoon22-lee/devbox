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
  restoreElevatedCdpPolicy,
} from "./windows-packaged-smoke.mjs";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync, realpathSync } from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
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
  if (policy) installElevatedCdpPolicy(policy);
  const item = { product: member.product, executable, policy, child: null, cdp: null };
  live.push(item);
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
  for (const member of manifest.members) apps[member.product] = await start(member);
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
  }
  evidence.result = "passed";
} catch (error) {
  evidence.failure = String(error).slice(0, 3000);
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
      if (item.policy) restoreElevatedCdpPolicy(item.policy);
    }
    evidence.cleanup.push({
      product: item.product,
      nativeError: item.error,
      exited:
        !item.identity ||
        !allWindowsProcesses().some((row) => row.Pid === item.identity.Pid && row.Created === item.identity.Created),
    });
  }
  mkdirSync("product-foundation-evidence", { recursive: true });
  writeFileSync(
    `product-foundation-evidence/suite-delivery-${mode}-${Date.now()}.json`,
    JSON.stringify(evidence, null, 2),
  );
}
