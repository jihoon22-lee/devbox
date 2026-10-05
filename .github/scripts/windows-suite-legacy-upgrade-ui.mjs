import { preserveReviewedCommitFailure } from "./windows-reviewed-helper-evidence.mjs";
import { waitForFixtureChildExit } from "./fixture-child-exit.mjs";
import {
  executeReviewedDeliveryAction,
  selectCurrentGenerationSnapshot,
  waitDeliveryInventoryReady,
  readRestoreInventory,
} from "./windows-delivery-review.mjs";
// L4 legacy preparation is separate from the actual current installer/use/restore UI journey.
import assert from "node:assert/strict";
import { boundedFailure } from "./user-flow-failure-evidence.mjs";
import path from "node:path";
import { readFile, writeFile, mkdir, stat, realpath, lstat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createRequire } from "node:module";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { randomUUID } from "node:crypto";
import { pathToFileURL } from "node:url";
import { prepareLegacySuite, legacySource } from "./windows-legacy-suite-fixture.mjs";
import {
  packagedIdentity,
  installedFixtureIdentity,
  writeUserFlowResults,
  fileDigest,
} from "./suite-user-flow-results.mjs";
import { runVisibleSetup, ownedNsisSpawnOptions } from "./windows-suite-installer-actions.mjs";
import { completeInstalledHealth, closeAutomaticallyOpenedCenter } from "./windows-suite-health-actions.mjs";
import { createInstalledProductContext, observeUntil } from "./windows-suite-ui-context.mjs";
import { createInstalledKnowledgeContext } from "./windows-knowledge-user-flows.mjs";
import { windowsLocalAppData, allWindowsProcesses, stopOwnedProcess } from "./windows-packaged-smoke.mjs";
const editor = { role: "textbox", name: "Markdown 본문" };
const json = async (file) => JSON.parse((await readFile(file, "utf8")).replace(/^\uFEFF/u, ""));
export { readRestoreInventory as readLegacyRestoreInventory } from "./windows-delivery-review.mjs";
// Process presence is not a completed restore: the replacement must own a new
// native lifetime and the durable barrier must permit its selected generation.
export function legacyReviewReopenReady(center, action, { processes, activation, restoreBlocked }) {
  const phase = action === "restore" ? "health" : ["snapshot", "rollback"].includes(action) ? "committed" : null;
  return Boolean(
    phase &&
      restoreBlocked === false &&
      activation?.phase === phase &&
      activation.installationId === center.manifest.installationId &&
      activation.generation === center.manifest.generation &&
      processes.some(
        (item) =>
          item.Path.toLowerCase() === center.executable.toLowerCase() &&
          item.Pid !== center.processIdentity.Pid &&
          item.Created > center.processIdentity.Created,
      ),
  );
}
export async function preserveLegacyReviewFailure(center, error, preserve = preserveReviewedCommitFailure) {
  try {
    await preserve(center, error, randomUUID());
  } catch {}
  throw error;
}
export async function finishLegacyCleanup(close, remove, originalFailure) {
  let cleanupFailure;
  try {
    await close();
  } catch (error) {
    cleanupFailure = error;
  }
  try {
    await remove();
  } catch (error) {
    cleanupFailure ??= error;
  }
  if (originalFailure) throw originalFailure;
  if (cleanupFailure) throw cleanupFailure;
}
export function legacyOperationFailure(operation, code, signal, stdout, stderr) {
  let issue = null,
    shortcut = null;
  try {
    const value = JSON.parse(stdout.trim());
    if (typeof value.issue === "string" && /^[a-z][a-z0-9_]{0,100}$/.test(value.issue)) issue = value.issue;
    if (operation === "registered-shortcut-launch") {
      const stages = [
        "ownership",
        "activation",
        "physical-identity",
        "retained-payload",
        "link-target",
        "registered-link-launch",
      ];
      const commands = [
        "Assert-Unlinked",
        "Assert-Identity",
        "Get-Content",
        "Get-Item",
        "Get-FileHash",
        "Split-Path",
        "Join-Path",
        "Add-Type",
        "New-Object",
        "ConvertFrom-Json",
        "ConvertTo-Json",
        "Where-Object",
      ];
      shortcut = {
        stage: stages.includes(value.stage) ? value.stage : null,
        command: commands.includes(value.command) ? value.command : null,
        line: Number.isInteger(value.line) && value.line >= 1 && value.line <= 2000 ? value.line : null,
      };
    }
  } catch {}
  return Object.assign(new Error(`Owned legacy fixture operation failed: ${operation}`), {
    operation: {
      operation,
      exitCode: code,
      signal: signal ?? null,
      issue,
      ...(shortcut ? { shortcut } : {}),
      stderr: boundedFailure(new Error(stderr)).message,
    },
  });
}
async function execute(image, args, { timeout = 180000, operation = "subprocess" } = {}) {
  const env = { ...process.env };
  for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
  const nsis = args.at(-1)?.startsWith("/D=") ? ownedNsisSpawnOptions(image, args) : {};
  const child = spawn(image, args, { env, stdio: ["ignore", "pipe", "pipe"], ...nsis });
  await once(child, "spawn");
  let output = "",
    error = "";
  child.stdout.on("data", (chunk) => {
    output = (output + chunk).slice(-16000);
  });
  child.stderr.on("data", (chunk) => {
    error = (error + chunk).slice(-1000);
  });
  let code, signal;
  try {
    [code, signal] = await waitForFixtureChildExit(child, timeout);
  } catch (cause) {
    if (cause.code !== "ETIMEDOUT") throw cause;
    const failure = legacyOperationFailure(operation, null, "timeout", output, error);
    failure.code = cause.code;
    throw failure;
  }
  if (code !== 0) throw legacyOperationFailure(operation, code, signal, output, error);
  return output.trim();
}
const productCatalog = createRequire(import.meta.url)("../../apps/products.json");
export function shortcutWindowReady(product, view) {
  const definition = productCatalog.products.find((item) => item.id === product);
  const feature = productCatalog.features.find(
    (item) => item.owner === product && item.route === definition?.defaultRoute,
  );
  return (
    !!definition &&
    !!feature &&
    view.name === definition.label &&
    view.enabled === true &&
    view.selectedWindowCount === 1 &&
    view.buttons?.some((button) => button.name === feature.label && button.enabled && button.visible) === true
  );
}
export async function verifyRegisteredShortcutLaunches(verify) {
  const proofs = [];
  for (const product of ["workspace", "api-studio", "knowledge", "control-center"]) {
    const proof = await verify(product);
    assert.equal(proof.product, product);
    for (const field of ["registeredLink", "freshProcess", "imageVerified", "ownedWindowReady"])
      assert.equal(proof[field], true);
    assert.equal(typeof proof.screenshot, "string");
    proofs.push(proof);
  }
  return proofs;
}
export async function closeLegacyShortcut(
  identity,
  executable,
  child,
  { stop = stopOwnedProcess, read = allWindowsProcesses, wait = observeUntil } = {},
) {
  const result = await stop(identity, executable, child);
  const webviews = result.descendants.filter((item) => item.Name.toLowerCase() === "msedgewebview2.exe");
  if (webviews.length) {
    await wait(() => {
      const current = read();
      return !webviews.some((owned) =>
        current.some(
          (item) =>
            item.Pid === owned.Pid &&
            item.Created === owned.Created &&
            item.Name === owned.Name &&
            item.Path === owned.Path,
        ),
      );
    }, "shortcut-owned WebView processes retired");
  }
  return result;
}
async function verifyInstalledShortcut(root, product, evidenceId) {
  const manifest = await json(path.join(root, "devbox-installation.json"));
  const member = manifest.members.find((item) => item.product === product);
  assert.equal(member?.executable, `generations/${manifest.generation}/products/${product}/devbox-${product}.exe`);
  const executable = await realpath(path.join(root, member.executable));
  assert.equal(executable.toLowerCase(), path.resolve(root, member.executable).toLowerCase());
  const release = await json(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"));
  const expected = release.products
    .find((item) => item.id === product)
    .files.find((item) => item.name === `devbox-${product}.exe`);
  assert.equal(await fileDigest(executable), member.sha256);
  assert.equal(member.sha256, expected.sha256);
  const matches = () => allWindowsProcesses().filter((item) => item.Path.toLowerCase() === executable.toLowerCase());
  assert.equal(matches().length, 0, "Existing product must not satisfy a shortcut launch");
  let identity, owner, failure;
  try {
    const launched = JSON.parse(
      await execute(
        "powershell.exe",
        [
          "-NoProfile",
          "-NonInteractive",
          "-File",
          path.resolve(".github/scripts/windows-suite-shortcut-launch.ps1"),
          "-Root",
          root,
          "-Product",
          product,
        ],
        { operation: "registered-shortcut-launch", timeout: 30000 },
      ),
    );
    assert.deepEqual(launched, { schemaVersion: 1, product, shortcutVerified: true, launchedRegisteredLink: true });
    await observeUntil(() => {
      const found = matches();
      assert.ok(found.length <= 1, "Ambiguous shortcut product process");
      identity = found[0];
      return !!identity;
    }, "registered shortcut product process");
    owner = captureWindowOwner(identity, path.dirname(root));
    await observeUntil(() => {
      try {
        return shortcutWindowReady(product, nativeWindowAction(owner, "Inspect"));
      } catch {
        return false;
      }
    }, "registered shortcut usable product window");
    const screenshot = path.resolve(
      `product-foundation-evidence/user-flows/screenshots/installer/${evidenceId}-shortcut-${product}.png`,
    );
    await mkdir(path.dirname(screenshot), { recursive: true });
    await execute(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        path.resolve(".github/scripts/windows-owned-installer-fault.ps1"),
        "-Action",
        "Capture",
        "-ProductWindow",
        product,
        "-FixtureRoot",
        path.dirname(root),
        "-FilePath",
        screenshot,
        "-TargetProcessId",
        String(identity.Pid),
        "-ExpectedExecutable",
        executable,
        "-ExpectedStartTimeUtc",
        owner.started,
      ],
      { operation: "registered-shortcut-window-capture", timeout: 30000 },
    );
    assert.deepEqual(await json(path.join(root, "devbox-installation.json")), manifest);
    return {
      product,
      registeredLink: true,
      freshProcess: true,
      imageVerified: true,
      ownedWindowReady: true,
      screenshot,
    };
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    // Only the newly observed exact image/creation identity is eligible.
    if (!identity) {
      const found = matches();
      if (found.length === 1) identity = found[0];
    }
    if (identity) {
      const child = {
        get exitCode() {
          return matches().some((item) => item.Pid === identity.Pid && item.Created === identity.Created) ? null : 0;
        },
      };
      try {
        await closeLegacyShortcut(identity, executable, child);
      } catch (cleanupError) {
        if (!failure) throw cleanupError;
        failure.shortcutCleanupError = boundedFailure(cleanupError);
      }
    }
  }
}
export async function validateOwnedLegacyRun() {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  assert.ok(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT && process.env.RUNNER_TEMP);
  const parentRoot = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT),
    parentInstallationKey = await installedFixtureIdentity();
  return { parentRoot, parentInstallationKey, identity: await packagedIdentity() };
}
async function notesStore(key) {
  const root = path.join(windowsLocalAppData(), `com.devbox.v08.knowledge.i${key}`),
    pointer = await json(path.join(root, "active-stores.json"));
  assert.equal(pointer.schemaVersion, 1);
  assert.match(pointer.generation, /^[a-f0-9-]{36}$/u);
  const file = await realpath(path.join(root, "stores", pointer.generation, "notes", "data.db"));
  assert.ok(file.toLowerCase().startsWith(root.toLowerCase() + path.sep));
  return { root, file, generation: pointer.generation };
}
export const legacyWalRead =
  "import sqlite3,sys,json,pathlib; c=sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True); print(json.dumps(c.execute(\"SELECT value FROM settings WHERE key='devbox.fixture.legacy-wal'\").fetchone()[0]))";
export const legacyWalWrite = `import sqlite3,sys,json,os
c=sqlite3.connect(sys.argv[1]); c.execute('PRAGMA journal_mode=WAL'); c.execute('PRAGMA wal_autocheckpoint=0')
c.execute('PRAGMA wal_checkpoint(TRUNCATE)')
vault=c.execute("SELECT value FROM settings WHERE key='root'").fetchone()[0]
c.execute("INSERT INTO settings(key,value) VALUES('devbox.fixture.legacy-wal',?)",(sys.argv[2],));c.commit()
print(json.dumps({'vault':vault,'token':sys.argv[2]}),flush=True);os._exit(0)`;
async function readWalRow(file) {
  return JSON.parse(await execute("python", ["-c", legacyWalRead, file]));
}
async function createSyntheticLegacyWal(store, label) {
  const token = `legacy-wal-${randomUUID()}`;
  const result = JSON.parse(await execute("python", ["-c", legacyWalWrite, store.file, token]));
  assert.ok((await stat(`${store.file}-wal`)).size > 32, "Legacy committed row must remain in WAL");
  assert.equal(await readWalRow(store.file), token);
  const vault = await realpath(result.vault);
  assert.ok(
    vault.toLowerCase().startsWith(store.root.toLowerCase() + path.sep),
    "Only owned native default vault is fixture data",
  );
  const rel = "Notes/legacy-upgrade-owned.md",
    note = path.join(vault, rel),
    original = `# ${label} 합성 문서\n`;
  await mkdir(path.dirname(note), { recursive: true });
  await writeFile(note, original, { flag: "wx" });
  return { token, rel, note, original };
}
export const withdrawnSource = "e499ac7127269bf67863bf0fdc42eaf53236b9f3";
async function prepareWithdrawnSuite(directory) {
  const assets = await realpath(process.env.DEVBOX_WITHDRAWN_ASSETS),
    manifest = await json(path.join(assets, "release-manifest.json"));
  assert.equal(manifest.sourceSha, withdrawnSource);
  assert.equal(
    await fileDigest(path.join(assets, "release-manifest.json")),
    "27a3dd7b2dab585fa6a930d3a1e64bd4f1f3166a141c926080f005bc16114b4b",
  );
  assert.equal(manifest.setup.sha256, "e714be17bc9f74f8b9608570dad8edb2be2dba0ee5b11aecd907e7dd260c79d5");
  assert.equal(manifest.suiteVersion, "0.9.0");
  assert.notEqual(manifest.sourceSha, process.env.GITHUB_SHA);
  await execute("python", [
    ".github/scripts/prepare-suite-fixture.py",
    assets,
    directory,
    "--source",
    withdrawnSource,
    "--run-id",
    "37095144741",
  ]);
  const helper = path.join(directory, "devbox-suite-bootstrap.exe"),
    center = manifest.products.find((item) => item.id === "control-center");
  assert.equal(
    await fileDigest(helper),
    center.files.find((item) => item.name === "resources/suite/devbox-suite-bootstrap.exe").sha256,
  );
  return {
    directory,
    manifest,
    helper,
    payloadPath: path.join(directory, "suite-payload.json"),
    setup: path.join(directory, manifest.setup.name),
  };
}
export async function runLegacyUpgradeUserFlow({ withdrawn = false } = {}) {
  const parent = await validateOwnedLegacyRun(),
    identity = parent.identity,
    screenshots = [],
    assertions = [];
  const evidenceId = withdrawn ? "DELIVERY-02-withdrawn" : "DELIVERY-01";
  const scratch = path.join(process.env.RUNNER_TEMP, `devbox-suite-delivery-${randomUUID().replaceAll("-", "")}`),
    root = path.join(scratch, "Suite UI Fixture");
  await mkdir(scratch);
  let center, knowledge, registration, record, originalFailure;
  let stage = "prepare-pinned-assets";
  try {
    const legacy = withdrawn
      ? await prepareWithdrawnSuite(path.join(scratch, "withdrawn-assets"))
      : await prepareLegacySuite(path.join(scratch, "legacy-assets"));
    stage = "install-pinned-setup";
    await execute(legacy.setup, ["/S", `/D=${root}`], { operation: stage });
    registration = await json(path.join(root, "suite-registration.json"));
    assert.match(registration.installationKey, /^[a-f0-9]{64}$/u);
    assert.notEqual(registration.installationKey, parent.parentInstallationKey);
    await writeFile(
      path.join(scratch, "user-flow-owner.json"),
      JSON.stringify({
        schemaVersion: 1,
        sourceSha: identity.sourceSha,
        installationKey: registration.installationKey,
        root,
        staging: legacy.directory,
      }),
      { flag: "wx" },
    );
    process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = root;
    await closeAutomaticallyOpenedCenter(root);
    stage = "prepare-pinned-import";
    await execute(process.execPath, [".github/scripts/windows-suite-delivery-native.mjs", root, "import"], {
      operation: stage,
    });
    stage = "activate-pinned-install";
    await execute(legacy.helper, ["--activate-clean-install", root, legacy.payloadPath], { operation: stage });
    stage = "prepare-pinned-health";
    await execute(process.execPath, [".github/scripts/windows-suite-delivery-native.mjs", root, "health"], {
      operation: stage,
    });
    stage = "commit-pinned-install";
    await execute(legacy.helper, ["--commit-clean-install", root, legacy.payloadPath], { operation: stage });
    assert.equal((await json(path.join(root, "devbox-activation.json"))).phase, "committed");
    assert.equal(
      (await json(path.join(root, "suite-payload.json"))).sourceSha,
      withdrawn ? withdrawnSource : legacySource,
    );
    await closeAutomaticallyOpenedCenter(root);
    stage = "prepare-pinned-wal";
    const oldStore = await notesStore(registration.installationKey),
      synthetic = await createSyntheticLegacyWal(oldStore, withdrawn ? "withdrawn v0.9.0" : "published v0.8.1"),
      oldManifest = await json(path.join(root, "devbox-installation.json"));
    assertions.push(
      `Pinned ${withdrawn ? "withdrawn v0.9.0" : "published v0.8.1"} hashes verified; silent/native old activation is fixture preparation only; synthetic committed WAL and native default-vault note created in separate owned namespace`,
    );
    const release = await json(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"));
    stage = "visible-current-upgrade";
    const updated = await runVisibleSetup(path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS, release.setup.name), root);
    stage = "current-upgrade-health";
    assert.equal(updated.installationKey, registration.installationKey);
    await closeAutomaticallyOpenedCenter(root);
    center = await createInstalledProductContext("control-center");
    await center.ui.click({ role: "button", name: "데이터 및 복구", scope: { role: "navigation", name: "제품 화면" } });
    const inventory = await readRestoreInventory(center);
    assert.equal(inventory.update.previousVersion, oldManifest.suiteVersion);
    assert.equal(inventory.update.version, release.suiteVersion);
    const checkpointId = inventory.update.checkpointId;
    assert.ok(inventory.checkpoints.some((item) => item.id === checkpointId));
    const backupStore = path.join(
      windowsLocalAppData(),
      `com.devbox.v08.suite-backups.i${registration.installationKey}`,
      checkpointId,
      "knowledge",
      "stores",
      oldStore.generation,
      "notes",
      "data.db",
    );
    assert.equal(
      await readWalRow(backupStore),
      synthetic.token,
      "Actual installer checkpoint must include committed WAL-only row",
    );
    screenshots.push(await center.ui.screenshot(`${evidenceId}-current-upgrade-health`));
    await center.close();
    center = null;
    screenshots.push(...(await completeInstalledHealth("업데이트 확정")));
    assert.equal((await json(path.join(root, "suite-payload.json"))).sourceSha, identity.sourceSha);
    assert.equal(await readWalRow((await notesStore(registration.installationKey)).file), synthetic.token);
    stage = "committed-registered-shortcuts";
    const shortcutProofs = await verifyRegisteredShortcutLaunches(async (product) => {
      const proof = await verifyInstalledShortcut(root, product, evidenceId);
      screenshots.push(proof.screenshot);
      return proof;
    });
    assertions.push(
      "All four registered Start Menu links launched fresh exact committed product images with owned HWND and usable product navigation; no direct executable fallback",
    );
    await writeFile(
      `product-foundation-evidence/${evidenceId}-registered-shortcuts.json`,
      JSON.stringify(
        {
          schemaVersion: 1,
          sourceSha: identity.sourceSha,
          products: shortcutProofs.map(({ screenshot, ...proof }) => proof),
        },
        null,
        2,
      ),
      { flag: "wx" },
    );

    let currentCheckpoint;
    const launchRecovery = async () => {
      center = await createInstalledProductContext("control-center");
      await center.ui.click({
        role: "button",
        name: "데이터 및 복구",
        scope: { role: "navigation", name: "제품 화면" },
      });
      await waitDeliveryInventoryReady(center.ui);
    };
    const review = async (target, action) => {
      const reviewedCenter = center;
      stage = `reviewed-${action}`;
      try {
        await executeReviewedDeliveryAction(center.ui, target, async () => {
          screenshots.push(await center.ui.screenshot(`${evidenceId}-restore-review-${screenshots.length}`));
        });
        await observeUntil(() => center.child.exitCode !== null, "reviewed restore Center shutdown");
        center.dispose();
        stage = `reviewed-${action}-completion`;
        await observeUntil(
          async () => {
            const activation = await json(path.join(root, "devbox-activation.json")).catch(() => null);
            const restoreBlocked = await lstat(path.join(root, "suite-data-restore.block"))
              .then(() => true)
              .catch((error) => (error.code === "ENOENT" ? false : null));
            return legacyReviewReopenReady(reviewedCenter, action, {
              processes: allWindowsProcesses(),
              activation,
              restoreBlocked,
            });
          },
          "restore reopened fresh Center with completed activation",
          90000,
        );
        await closeAutomaticallyOpenedCenter(root);
        stage = `reviewed-${action}-reattach`;
        center = await createInstalledProductContext("control-center");
        await center.ui.click({
          role: "button",
          name: "데이터 및 복구",
          scope: { role: "navigation", name: "제품 화면" },
        });
        await waitDeliveryInventoryReady(center.ui);
      } catch (error) {
        return preserveLegacyReviewFailure(reviewedCenter, error);
      }
    };
    if (!withdrawn) {
      await launchRecovery();
      const beforeSnapshot = await center.delivery("restore_inventory");
      assert.equal(
        beforeSnapshot.checkpoints.find((item) => item.id === checkpointId)?.compatibility,
        "differentGeneration",
      );
      assert.equal(
        await center.cdp.evaluate(`(() => {
        const row = [...document.querySelectorAll('li')].find(item => item.getAttribute('aria-label') === ${JSON.stringify(checkpointId)});
        const buttons = row ? [...row.querySelectorAll('button')].filter(item => item.textContent.trim() === '이 보존본으로 복원') : [];
        return buttons.length === 1 && buttons[0].disabled;
      })()`),
        true,
        "Old generation restore must remain visibly disabled",
      );
      await review({ role: "button", name: "현재 데이터 보존" }, "snapshot");
      currentCheckpoint = selectCurrentGenerationSnapshot(
        beforeSnapshot,
        await center.delivery("restore_inventory"),
        checkpointId,
      );
      assert.equal(await readWalRow(backupStore), synthetic.token);
      await center.close();
      center = null;
    }
    knowledge = await createInstalledKnowledgeContext();
    await knowledge.knowledgeFixture.navigate("notes");
    await knowledge.knowledgeFixture.openNote(synthetic.rel);
    assert.equal(await knowledge.knowledgeFixture.editorText(), synthetic.original);
    await knowledge.knowledgeFixture.disableAutosave();
    const newBody = "# current에서 추가한 합성 데이터\n";
    await knowledge.ui.fill(editor, newBody);
    await knowledge.ui.press("Control+s");
    await knowledge.knowledgeFixture.wait(
      async () => (await readFile(synthetic.note, "utf8")) === newBody,
      "current UI note saved",
    );
    const newNoteHash = await fileDigest(synthetic.note);
    screenshots.push(await knowledge.ui.screenshot(`${evidenceId}-current-real-edit`));
    await knowledge.ui.closeOwnedWindow();
    await knowledge.knowledgeFixture.wait(() => knowledge.child.exitCode !== null, "saved current Knowledge normal X");
    await knowledge.close();
    knowledge = null;
    if (!withdrawn) {
      center = await createInstalledProductContext("control-center");
      await center.ui.click({
        role: "button",
        name: "데이터 및 복구",
        scope: { role: "navigation", name: "제품 화면" },
      });
      await waitDeliveryInventoryReady(center.ui);
      const beforeRestore = await center.delivery("restore_inventory");
      assert.deepEqual(
        beforeRestore.checkpoints.find((item) => item.id === currentCheckpoint.id),
        currentCheckpoint,
      );
      await review(
        {
          role: "button",
          name: "이 보존본으로 복원",
          scope: { role: "listitem", name: currentCheckpoint.id },
        },
        "restore",
      );
      const restored = await center.delivery("restore_inventory");
      assert.ok(restored.activeOperation);
      assert.equal(
        await readFile(synthetic.note, "utf8"),
        synthetic.original,
        "Reviewed restore must expose old note bytes before commit",
      );
      const operation = restored.operations.find((item) => item.id === restored.activeOperation);
      assert.equal(operation.phase, "health");
      assert.ok(
        restored.checkpoints.length > beforeRestore.checkpoints.length,
        "Restore must preserve current data before applying current-generation checkpoint",
      );
      screenshots.push(await center.ui.screenshot(`${evidenceId}-restored-health`));
      await review(
        { role: "button", name: "원본으로 복귀", scope: { role: "listitem", name: operation.id } },
        "rollback",
      );
      const afterRollback = await center.delivery("restore_inventory");
      assert.deepEqual(
        afterRollback.checkpoints.find((item) => item.id === checkpointId),
        beforeRestore.checkpoints.find((item) => item.id === checkpointId),
        "Restore and rollback must retain the pinned old checkpoint unchanged",
      );
      assert.equal(await readWalRow(backupStore), synthetic.token);
      assert.equal(
        await fileDigest(synthetic.note),
        newNoteHash,
        "Actual restore rollback must recover new current data exactly",
      );
      await center.close();
      center = null;
      knowledge = await createInstalledKnowledgeContext();
      await knowledge.knowledgeFixture.navigate("notes");
      await knowledge.knowledgeFixture.openNote(synthetic.rel);
      assert.equal(await knowledge.knowledgeFixture.editorText(), newBody);
      screenshots.push(await knowledge.ui.screenshot(`${evidenceId}-new-data-restored`));
    }
    assertions.push(
      "Visible exact candidate installer upgraded pinned old generation; actual current four-product health and reviewed commit preserved WAL-only row in both checkpoint and current store",
      withdrawn
        ? "Actual current Knowledge opened withdrawn same-version note and saved new bytes after exact candidate update"
        : "Actual current Knowledge opened the legacy note and saved new bytes; old-generation checkpoint remained blocked; actual current-generation checkpoint restore preserved current data and explicit restore rollback recovered the new saved bytes without reverse-conversion",
    );
    record = {
      id: "DELIVERY-01",
      status: "PASS",
      ...identity,
      fixtureKind: withdrawn ? "withdrawn-same-version-update" : "legacy-upgrade",
      baselineSourceSha: withdrawn ? withdrawnSource : legacySource,
      parentInstallationKey: parent.parentInstallationKey,
      evidenceKind: "packaged-ui",
      assertions,
      screenshotPaths: screenshots,
      failureCode: null,
    };
  } catch (error) {
    originalFailure = error;
    try {
      const ui = knowledge?.ui ?? center?.ui;
      if (ui) screenshots.push(await ui.screenshot(`${evidenceId}-first-failure`));
    } catch {}
    record = {
      id: "DELIVERY-01",
      status: "FAIL",
      ...identity,
      fixtureKind: withdrawn ? "withdrawn-same-version-update" : "legacy-upgrade",
      baselineSourceSha: withdrawn ? withdrawnSource : legacySource,
      parentInstallationKey: parent.parentInstallationKey,
      evidenceKind: "packaged-ui",
      assertions: [...assertions, String(error.message).slice(0, 500)],
      screenshotPaths: screenshots,
      failureCode: "legacy-upgrade-ui-failed",
      stage,
      error: boundedFailure(error),
      operation: error.operation,
      launch: error.launch,
    };
    await mkdir("product-foundation-evidence", { recursive: true });
    await writeFile(
      `product-foundation-evidence/${withdrawn ? "withdrawn" : "legacy"}-upgrade-first-failure.json`,
      JSON.stringify(
        {
          stage,
          ...identity,
          status: "FAIL",
          error: record.error,
          operation: record.operation,
          launch: record.launch,
          screenshotPaths: screenshots,
        },
        null,
        2,
      ),
      { flag: "wx" },
    ).catch(() => {});
  } finally {
    try {
      await finishLegacyCleanup(
        async () => {
          if (center) await center.close().catch(() => {});
          if (knowledge) await knowledge.close();
        },
        async () => {
          try {
            if (registration) {
              try {
                await writeUpgradeEvidence(record, withdrawn, registration.installationKey);
              } finally {
                await execute(
                  "pwsh",
                  ["-NoProfile", "-File", ".github/scripts/windows-user-flow-install.ps1", "-Cleanup"],
                  { operation: "cleanup-owned-installation" },
                );
              }
            } else {
              process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = parent.parentRoot;
              await writeUpgradeEvidence(record, withdrawn, parent.parentInstallationKey);
            }
          } finally {
            process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = parent.parentRoot;
          }
        },
        originalFailure,
      );
    } catch (cleanupError) {
      if (originalFailure) throw originalFailure;
      throw cleanupError;
    } finally {
      process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = parent.parentRoot;
    }
  }
  if (originalFailure) throw originalFailure;
  assert.equal(record.status, "PASS", "Inspect actual legacy-upgrade UI failure evidence");
  return record;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  await runLegacyUpgradeUserFlow({ withdrawn: process.argv.includes("--withdrawn") });

async function writeUpgradeEvidence(record, withdrawn, key) {
  if (!withdrawn) return writeUserFlowResults("legacy-upgrade", [record]);
  const { id, ...proof } = record;
  assert.equal(id, "DELIVERY-01");
  await mkdir("product-foundation-evidence/user-flows/delivery-hooks", { recursive: true });
  await writeFile(
    "product-foundation-evidence/user-flows/delivery-hooks/withdrawn-update.json",
    JSON.stringify({ schemaVersion: 1, ...proof, installationKey: key, candidateSourceSha: record.sourceSha }, null, 2),
    { flag: "wx" },
  );
}
