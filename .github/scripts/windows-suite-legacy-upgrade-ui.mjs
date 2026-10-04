// L4 legacy preparation is separate from the actual current installer/use/restore UI journey.
import assert from "node:assert/strict";
import path from "node:path";
import { readFile, writeFile, mkdir, stat, realpath } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
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
import { windowsLocalAppData, allWindowsProcesses } from "./windows-packaged-smoke.mjs";
const editor = { role: "textbox", name: "Markdown 본문" };
const json = async (file) => JSON.parse((await readFile(file, "utf8")).replace(/^\uFEFF/u, ""));
async function execute(image, args, { timeout = 180000 } = {}) {
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
  const timer = setTimeout(() => child.kill(), timeout);
  try {
    const [code] = await once(child, "exit");
    assert.equal(code, 0, `Owned legacy fixture operation failed: ${error}`);
    return output.trim();
  } finally {
    clearTimeout(timer);
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
async function readWalRow(file) {
  return JSON.parse(
    await execute("python", [
      "-c",
      "import sqlite3,sys,json,pathlib; c=sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True); print(json.dumps(c.execute('SELECT value FROM fixture_legacy_wal WHERE id=1').fetchone()[0]))",
      file,
    ]),
  );
}
async function createSyntheticLegacyWal(store, label) {
  const token = `legacy-wal-${randomUUID()}`;
  const result = JSON.parse(
    await execute("python", [
      "-c",
      `import sqlite3,sys,json,os
c=sqlite3.connect(sys.argv[1]); c.execute('PRAGMA journal_mode=WAL'); c.execute('PRAGMA wal_autocheckpoint=0')
c.execute('CREATE TABLE fixture_legacy_wal(id INTEGER PRIMARY KEY,value TEXT NOT NULL)'); c.commit(); c.execute('PRAGMA wal_checkpoint(TRUNCATE)')
vault=c.execute("SELECT value FROM settings WHERE key='root'").fetchone()[0]
c.execute('INSERT INTO fixture_legacy_wal VALUES(1,?)',(sys.argv[2],));c.commit()
print(json.dumps({'vault':vault,'token':sys.argv[2]}),flush=True);os._exit(0)`,
      store.file,
      token,
    ]),
  );
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
  let center, knowledge, registration, record;
  try {
    const legacy = withdrawn
      ? await prepareWithdrawnSuite(path.join(scratch, "withdrawn-assets"))
      : await prepareLegacySuite(path.join(scratch, "legacy-assets"));
    await execute(legacy.setup, ["/S", `/D=${root}`]);
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
    await execute(process.execPath, [".github/scripts/windows-suite-delivery-native.mjs", root, "import"]);
    await execute(legacy.helper, ["--activate-clean-install", root, legacy.payloadPath]);
    await execute(process.execPath, [".github/scripts/windows-suite-delivery-native.mjs", root, "health"]);
    await execute(legacy.helper, ["--commit-clean-install", root, legacy.payloadPath]);
    assert.equal((await json(path.join(root, "devbox-activation.json"))).phase, "committed");
    assert.equal(
      (await json(path.join(root, "suite-payload.json"))).sourceSha,
      withdrawn ? withdrawnSource : legacySource,
    );
    await closeAutomaticallyOpenedCenter(root);
    const oldStore = await notesStore(registration.installationKey),
      synthetic = await createSyntheticLegacyWal(oldStore, withdrawn ? "withdrawn v0.9.0" : "published v0.8.1"),
      oldManifest = await json(path.join(root, "devbox-installation.json"));
    assertions.push(
      `Pinned ${withdrawn ? "withdrawn v0.9.0" : "published v0.8.1"} hashes verified; silent/native old activation is fixture preparation only; synthetic committed WAL and native default-vault note created in separate owned namespace`,
    );
    const release = await json(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"));
    const updated = await runVisibleSetup(path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS, release.setup.name), root);
    assert.equal(updated.installationKey, registration.installationKey);
    await closeAutomaticallyOpenedCenter(root);
    center = await createInstalledProductContext("control-center");
    await center.ui.click({ role: "button", name: "데이터 및 복구", scope: { role: "navigation", name: "제품 화면" } });
    const inventory = await center.delivery("restore_inventory");
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
    knowledge = await createInstalledKnowledgeContext();
    await knowledge.knowledgeFixture.navigate("notes");
    await knowledge.knowledgeFixture.openNote(synthetic.rel);
    assert.equal(await knowledge.ui.text(editor), synthetic.original);
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
      const review = async (target) => {
        await center.ui.click(target);
        await center.ui.click({ role: "checkbox", name: "선택한 작업과 제품 종료를 확인했습니다." });
        screenshots.push(await center.ui.screenshot(`${evidenceId}-restore-review-${screenshots.length}`));
        await center.ui.click({ role: "button", name: "Control Center를 닫고 실행" });
        await observeUntil(() => center.child.exitCode !== null, "reviewed restore Center shutdown");
        center.dispose();
        const executable = center.executable;
        await observeUntil(
          () => allWindowsProcesses().some((item) => item.Path.toLowerCase() === executable.toLowerCase()),
          "restore reopened Center",
          90000,
        );
        await closeAutomaticallyOpenedCenter(root);
        center = await createInstalledProductContext("control-center");
        await center.ui.click({
          role: "button",
          name: "데이터 및 복구",
          scope: { role: "navigation", name: "제품 화면" },
        });
      };
      await review({ role: "button", name: "이 보존본으로 복원", scope: { role: "listitem", name: checkpointId } });
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
        restored.checkpoints.length > inventory.checkpoints.length,
        "Restore must preserve current data before applying old checkpoint",
      );
      screenshots.push(await center.ui.screenshot(`${evidenceId}-restored-health`));
      await review({ role: "button", name: "원본으로 복귀", scope: { role: "listitem", name: operation.id } });
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
      assert.equal(await knowledge.ui.text(editor), newBody);
      screenshots.push(await knowledge.ui.screenshot(`${evidenceId}-new-data-restored`));
    }
    assertions.push(
      "Visible exact candidate installer upgraded pinned old generation; actual current four-product health and reviewed commit preserved WAL-only row in both checkpoint and current store",
      withdrawn
        ? "Actual current Knowledge opened withdrawn same-version note and saved new bytes after exact candidate update"
        : "Actual current Knowledge opened the legacy note and saved new bytes; actual pre-update checkpoint restore preserved current data and explicit restore rollback recovered the new saved bytes without reverse-conversion",
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
    };
  } finally {
    if (center) await center.close().catch(() => {});
    if (knowledge) await knowledge.close();
    try {
      if (registration) {
        try {
          await writeUpgradeEvidence(record, withdrawn, registration.installationKey);
        } finally {
          await execute("pwsh", ["-NoProfile", "-File", ".github/scripts/windows-user-flow-install.ps1", "-Cleanup"]);
        }
      } else {
        process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = parent.parentRoot;
        await writeUpgradeEvidence(record, withdrawn, parent.parentInstallationKey);
      }
    } finally {
      process.env.DEVBOX_USER_FLOW_INSTALL_ROOT = parent.parentRoot;
    }
  }
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
