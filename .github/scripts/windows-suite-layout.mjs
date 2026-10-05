import assert from "node:assert/strict";
import { measureInput, measureIdle, evaluateBudgets } from "./product-foundation-performance.mjs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { observePortableAgentBeforeClose, observePortableAgentAfterClose } from "./windows-suite-agent-user-flows.mjs";
import { createDirectProductContext } from "./windows-suite-direct-layout.mjs";
import { createInstalledProductContext } from "./windows-suite-ui-context.mjs";
import { observeProductLayout, assertProductLayout } from "./browser-product-layout.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { packagedIdentity, installedFixtureIdentity, writeUserFlowResults } from "./suite-user-flow-results.mjs";

export const layoutProducts = ["workspace", "api-studio", "knowledge", "control-center"];
export const layoutStates = ["direct", "committed", "import", "health", "recover"];
const evidenceRoot = "product-foundation-evidence/user-flows/layout-observations";

export function correctClientSize(nativeSize, observed, target, pixelRatio) {
  for (const value of [
    nativeSize.width,
    nativeSize.height,
    observed.width,
    observed.height,
    target.width,
    target.height,
    pixelRatio,
  ])
    assert.ok(Number.isFinite(value) && value > 0, "Available positive window dimensions required");
  if (Math.abs(observed.width - target.width) <= 2 && Math.abs(observed.height - target.height) <= 2) return null;
  const corrected = {
    width: Math.round(nativeSize.width + (target.width - observed.width) * pixelRatio),
    height: Math.round(nativeSize.height + (target.height - observed.height) * pixelRatio),
  };
  assert.ok(
    corrected.width >= 400 && corrected.width <= 2560 && corrected.height >= 300 && corrected.height <= 1600,
    "Corrected physical window size outside native ownership bounds",
  );
  return corrected;
}

// Keep diagnostic evidence even when the OS clamps a requested native size.
export async function persistMeasuredLayout(observation, { screenshot, persist }) {
  const saved = { ...observation, screenshotPath: await screenshot() };
  await persist(saved);
  assertProductLayout(saved.observed, { editor: Boolean(saved.observed.editor) });
  assert.ok(
    Math.abs(saved.observed.viewport.width - saved.size.width) <= 2 &&
      Math.abs(saved.observed.viewport.height - saved.size.height) <= 2,
    "Actual client dimensions must correspond to requested native size",
  );
  if (saved.size.name === "minimum")
    assert.ok(
      saved.observed.viewport.width >= saved.size.width - 2 && saved.observed.viewport.height >= saved.size.height - 2,
      "OS minimum must preserve configured client task area",
    );
  return saved;
}

const windowMetricsExpression =
  "({width:innerWidth,height:innerHeight,outerWidth,outerHeight,pixelRatio:devicePixelRatio,screen:{width:screen.width,height:screen.height,availWidth:screen.availWidth,availHeight:screen.availHeight,availLeft:screen.availLeft,availTop:screen.availTop},screenX,screenY,visibility:document.visibilityState})";

// Installer calls this while the real product is in the given delivery phase.
// Observations never change delivery state or replace a production description.
export async function observeInstalledProductLayout({ cdp, ui, processIdentity, root, product, state }) {
  assert.ok(layoutProducts.includes(product));
  assert.ok(layoutStates.includes(state));
  const description = await cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')");
  assert.equal(description.product.id, product);
  assert.equal(description.deliveryState, state, "Actual delivery phase must match observation");
  const identity = await packagedIdentity();
  const installationKey = await installedFixtureIdentity();
  const owner = captureWindowOwner(processIdentity, path.dirname(root));
  const config = JSON.parse(await readFile(path.resolve(`apps/devbox-${product}/src-tauri/tauri.conf.json`), "utf8"));
  const window = config.app.windows.find((item) => item.label === "main");
  assert.ok(window);
  const sizes = [
    { name: "default", width: window.width, height: window.height },
    { name: "minimum", width: window.minWidth, height: window.minHeight },
  ];
  const observations = [];
  for (const size of sizes) {
    const before = await cdp.evaluate(windowMetricsExpression);
    assert.ok(Number.isFinite(before.pixelRatio) && before.pixelRatio > 0);
    let nativeSize = {
      width: Math.round(size.width * before.pixelRatio),
      height: Math.round(size.height * before.pixelRatio),
    };
    const resizeRequests = [{ ...nativeSize }];
    nativeWindowAction(owner, "Resize", nativeSize);
    await cdp.command("Page.bringToFront");
    await cdp.evaluate("new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
    let observed = await cdp.evaluate(`(${observeProductLayout.toString()})()`);
    const corrected = correctClientSize(nativeSize, observed.viewport, size, before.pixelRatio);
    if (corrected) {
      nativeSize = corrected;
      resizeRequests.push({ ...corrected });
      nativeWindowAction(owner, "Resize", corrected);
      await cdp.evaluate("new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
      observed = await cdp.evaluate(`(${observeProductLayout.toString()})()`);
    }
    await mkdir(evidenceRoot, { recursive: true });
    observations.push(
      await persistMeasuredLayout(
        {
          size,
          beforeWindowMetrics: before,
          requestedNativeSize: nativeSize,
          nativeResizeRequests: resizeRequests,
          windowMetrics: await cdp.evaluate(windowMetricsExpression),
          nativeWindow: nativeWindowAction(owner, "Inspect"),
          observed,
        },
        {
          screenshot: () => ui.screenshot(`layout-${product}-${state}-${size.name}`),
          persist: (observation) =>
            writeFile(
              path.join(evidenceRoot, `${product}-${state}-${size.name}-measurement.json`),
              JSON.stringify({ ...identity, installationKey, product, state, observation }, null, 2),
              { flag: "wx" },
            ),
        },
      ),
    );
  }
  await mkdir(evidenceRoot, { recursive: true });
  const record = { ...identity, installationKey, product, state, observations };
  await writeFile(path.join(evidenceRoot, `${product}-${state}.json`), JSON.stringify(record, null, 2), { flag: "wx" });
  return record;
}

export async function writeProductInputObservation({ product, checks, assertions, screenshotPaths }) {
  assert.ok(layoutProducts.includes(product));
  for (const check of ["keyboard", "modalFocusReturn", "ime", "scale", "taskComplete"])
    assert.equal(checks?.[check], true, `${product}: ${check} unobserved`);
  assert.ok(assertions.length && screenshotPaths.length);
  const record = {
    ...(await packagedIdentity()),
    installationKey: await installedFixtureIdentity(),
    product,
    status: "PASS",
    checks,
    assertions,
    screenshotPaths,
  };
  await mkdir(evidenceRoot, { recursive: true });
  await writeFile(path.join(evidenceRoot, `input-${product}.json`), JSON.stringify(record, null, 2), { flag: "wx" });
  return record;
}

export async function capturePerformanceScreenshot(
  cdp,
  product,
  directory = "product-foundation-evidence/user-flows/screenshots/performance",
) {
  assert.ok(layoutProducts.includes(product));
  // Restarting workloads replace their owned renderer; resolve its session only at capture time.
  const current = typeof cdp === "function" ? cdp() : cdp;
  const { data } = await current.command("Page.captureScreenshot", { format: "png" });
  const bytes = Buffer.from(data, "base64");
  assert.equal(bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", "Performance PNG capture required");
  const file = path.resolve(directory, `performance-${product}-completed.png`);
  await mkdir(directory, { recursive: true });
  await writeFile(file, bytes, { flag: "wx" });
  return [file];
}

export function layoutEvidenceStatus(missing, failureCode) {
  return missing.length ? { status: "NOT_RUN", failureCode, missing } : { status: "PASS", failureCode: null };
}

// Inserted in the existing first installed launch, before synthetic business work.
export async function observeProductPerformance({
  product,
  cdp,
  getCdp = () => cdp,
  getIdentities,
  coldRendererReadyMs,
  warmExistingWindowMs,
  workload,
}) {
  assert.ok(layoutProducts.includes(product));
  assert.ok(Number.isFinite(coldRendererReadyMs) && Number.isFinite(warmExistingWindowMs));
  assert.equal(typeof getIdentities, "function");
  const config = JSON.parse(await readFile(new URL("./product-foundation-performance.json", import.meta.url), "utf8"));
  await delay(10_000); // Existing startup survival condition, before owned idle sample.
  const measured = {
    coldRendererReadyMs,
    warmExistingWindowMs,
    firstKeyboardEventMs: await measureInput({
      evaluate: (...args) => cdp.evaluate(...args),
      send: (...args) => cdp.command(...args),
    }),
    idle: await measureIdle(getIdentities, config.idleSampleMs),
    workload: typeof workload === "function" ? await workload() : workload,
  };
  const budget = evaluateBudgets(measured, config, product);
  assert.ok(budget.passed, `${product} exceeded performance budgets: ${budget.violations.join(", ")}`);
  const record = {
    ...(await packagedIdentity()),
    installationKey: await installedFixtureIdentity(),
    product,
    conditions: {
      cold: "Spawn to actual ready shell using prepared installed product namespace; product data is retained",
      warm: "Existing owned primary window minimized then activated through native UI Automation",
      idle: "Ten-second startup survival followed by five-second owned process cohort sample before representative workload",
      input: "Actual inert F24 key dispatch and renderer acknowledgement",
    },
    measured,
    budget,
    screenshotPaths: await capturePerformanceScreenshot(getCdp, product),
  };
  await mkdir(evidenceRoot, { recursive: true });
  await writeFile(path.join(evidenceRoot, `performance-${product}.json`), JSON.stringify(record, null, 2), {
    flag: "wx",
  });
  return record;
}

export async function aggregateLayoutEvidence() {
  const identity = await packagedIdentity(),
    installationKey = await installedFixtureIdentity();
  const assertions = [],
    screenshotPaths = [],
    missing = [];
  for (const product of layoutProducts)
    for (const state of layoutStates) {
      let record;
      try {
        record = JSON.parse(await readFile(path.join(evidenceRoot, `${product}-${state}.json`), "utf8"));
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
        missing.push(`${product}/${state}`);
        continue;
      }
      assert.equal(record.sourceSha, identity.sourceSha);
      assert.equal(record.fixtureSha, identity.fixtureSha);
      assert.deepEqual(record.artifactDigests, identity.artifactDigests);
      assert.equal(record.installationKey, installationKey);
      assert.equal(record.product, product);
      assert.equal(record.state, state);
      assert.equal(record.observations.length, 2);
      assert.deepEqual(
        record.observations.map((item) => item.size.name),
        ["default", "minimum"],
      );
      for (const observation of record.observations) {
        assertProductLayout(observation.observed, { editor: Boolean(observation.observed.editor) });
        assert.ok(observation.screenshotPath);
        screenshotPaths.push(observation.screenshotPath);
        assertions.push(
          `${product}/${state}/${observation.size.name}: main and enabled primary controls visible without notice overlap or horizontal clipping`,
        );
      }
    }
  return {
    ...identity,
    id: "UI-01",
    evidenceKind: "packaged-ui",
    ...layoutEvidenceStatus(missing, "missing-actual-delivery-layout-observations"),
    assertions,
    screenshotPaths,
  };
}

export async function runSuiteLayout() {
  for (const product of layoutProducts) {
    const direct = await createDirectProductContext(product);
    let portableProof;
    try {
      await observeInstalledProductLayout({ ...direct, state: "direct" });
      if (product === "workspace") portableProof = await observePortableAgentBeforeClose(direct);
    } finally {
      await direct.close();
    }
    if (portableProof) await observePortableAgentAfterClose(direct, portableProof);
    const context = await createInstalledProductContext(product);
    try {
      await observeInstalledProductLayout({ ...context, product, state: "committed" });
    } finally {
      await context.close();
      await context.dispose();
    }
  }
  const result = await aggregateLayoutEvidence();
  const identity = await packagedIdentity(),
    installationKey = await installedFixtureIdentity();
  const results = [result];
  for (const [id, prefix] of [
    ["UI-02", "input"],
    ["PERF-01", "performance"],
  ]) {
    const assertions = [],
      screenshotPaths = [],
      missing = [],
      records = [];
    for (const product of layoutProducts) {
      let record;
      try {
        record = JSON.parse(await readFile(path.join(evidenceRoot, `${prefix}-${product}.json`), "utf8"));
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
        missing.push(product);
        continue;
      }
      assert.equal(record.sourceSha, identity.sourceSha);
      assert.equal(record.fixtureSha, identity.fixtureSha);
      assert.deepEqual(record.artifactDigests, identity.artifactDigests);
      assert.equal(record.installationKey, installationKey);
      assert.equal(record.product, product);
      if (id === "UI-02") {
        for (const check of ["keyboard", "modalFocusReturn", "ime", "scale", "taskComplete"])
          assert.equal(record.checks?.[check], true, `${product}: ${check} unobserved`);
        assert.equal(record.status, "PASS");
        assert.ok(record.assertions.length);
        assert.ok(record.screenshotPaths.length);
        assertions.push(...record.assertions);
        screenshotPaths.push(...record.screenshotPaths);
      } else {
        assert.equal(record.budget.passed, true);
        assert.ok(Array.isArray(record.screenshotPaths) && record.screenshotPaths.length === 1);
        screenshotPaths.push(...record.screenshotPaths);
        assertions.push(`${product}: actual cold/warm/input/owned-idle measurements within existing budgets`);
      }
      records.push(record);
    }
    if (id === "PERF-01" && !missing.length) {
      const workload = records.find((record) => record.product === "knowledge")?.measured.workload;
      if (
        !workload ||
        workload.fileCount !== 500 ||
        !Number.isFinite(workload.indexMs) ||
        workload.searchMs?.length !== 10
      )
        missing.push("500-file-index-and-ten-searches");
      for (const product of ["workspace", "api-studio", "control-center"]) {
        const work = records.find((record) => record.product === product)?.measured.workload;
        if (!work?.taskCompleted || !Number.isFinite(work.completeMs)) missing.push(`${product}-representative-task`);
      }
    }
    results.push({
      ...identity,
      id,
      ...layoutEvidenceStatus(missing, "missing-actual-input-or-performance-observations"),
      evidenceKind: "packaged-ui",
      assertions,
      screenshotPaths,
      measurements: records,
    });
  }
  await writeUserFlowResults("suite-layout", results);
  assert.ok(
    results.every((item) => item.status === "PASS"),
    "Required actual layout/input/performance observations incomplete",
  );
  return results;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) await runSuiteLayout();
