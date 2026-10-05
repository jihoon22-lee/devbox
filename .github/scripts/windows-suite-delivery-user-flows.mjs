import { executeReviewedDeliveryAction } from "./windows-delivery-review.mjs";
// Actual Recovery review, native dirty-close cancellation and installed data preservation.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { preserveReviewedCommitFailure } from "./windows-reviewed-helper-evidence.mjs";
import { withdrawnSource } from "./windows-suite-legacy-upgrade-ui.mjs";
import { observeInstallerFailurePreservation } from "./windows-suite-installer-failures.mjs";
import { readFile, writeFile, access } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createInstalledKnowledgeContext } from "./windows-knowledge-user-flows.mjs";
import { createInstalledProductContext, observeUntil } from "./windows-suite-ui-context.mjs";
import { packagedIdentity, writeUserFlowResults, fileDigest } from "./suite-user-flow-results.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { runVisibleSetup, runVisibleRemoval, rejectBusyVisibleUpdate } from "./windows-suite-installer-actions.mjs";
import { completeInstalledHealth, closeAutomaticallyOpenedCenter } from "./windows-suite-health-actions.mjs";
import { prepareDistinctGeneration } from "./windows-suite-update-fixture.mjs";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
export async function observeDeliveryReopen(
  center,
  observe = observeUntil,
  read = allWindowsProcesses,
  preserve = preserveReviewedCommitFailure,
) {
  try {
    await observe(
      () => read().some((p) => p.Path.toLowerCase() === center.executable.toLowerCase()),
      "helper reopened Center",
      90000,
    );
  } catch (error) {
    try {
      await preserve(center, error, randomUUID());
    } catch {}
    throw error;
  }
}
const editor = { role: "textbox", name: "Markdown 본문" };
export async function assertKnowledgeDraft(context, expected) {
  assert.equal(await context.knowledgeFixture.editorText(), expected);
}
export const scenarioIds = ["INSTALL-03", "DELIVERY-02"];
export async function finalizeDeliveryContexts(center, knowledge, results) {
  const failures = [];
  for (const [product, context] of [
    ["control-center", center],
    ["knowledge", knowledge],
  ]) {
    try {
      if (context) await context.close();
    } catch {
      failures.push(`Owned ${product} cleanup failed`);
    }
  }
  if (!failures.length) return;
  // Preserve the first journey failure and its rows even if normal close also fails.
  const row = results.findLast((item) => item.status === "FAIL") ?? results.at(-1);
  assert.ok(row, "Delivery cleanup failed before any evidence was recorded");
  row.assertions.push(...failures);
  if (row.status !== "FAIL") {
    row.status = "FAIL";
    row.failureCode = "delivery-cleanup-failed";
  }
}
export async function completeHealthAfterKnowledgeClose(knowledge, label, completeHealth = completeInstalledHealth) {
  assert.notEqual(knowledge.child.exitCode, null, "Knowledge normal saved close must precede health");
  // Native X already exited the first owner; release its per-image CDP policy
  // before health opens the same executable under a fresh debugging owner.
  await knowledge.close();
  return completeHealth(label);
}

async function exists(file) {
  try {
    await access(file);
    return true;
  } catch {
    return false;
  }
}
export async function run({ checkpointOnly = false } = {}) {
  const identity = await packagedIdentity(),
    results = [],
    screenshots = [];
  if (checkpointOnly)
    assert.equal(identity.diagnosticOnly, true, "Checkpoint-only execution requires retained diagnostic identity");
  let center, knowledge;
  const record = (id, status, assertions, failureCode = null) => ({
    id,
    status,
    ...identity,
    evidenceKind: "packaged-ui",
    assertions,
    screenshotPaths: [...screenshots],
    failureCode,
  });
  const launchCenter = async () => {
    center = await createInstalledProductContext("control-center");
    await center.ui.click({ role: "button", name: "데이터 및 복구", scope: { role: "navigation", name: "제품 화면" } });
    await observeUntil(async () => (await center.body()).includes("현재 데이터 보존"), "recovery ready");
  };
  const nativeHelper = () =>
    allWindowsProcesses().filter(
      (p) =>
        p.Path.toLowerCase().startsWith(`${knowledge.root}${path.sep}`.toLowerCase()) &&
        path.basename(p.Path).toLowerCase() === "devbox-suite-bootstrap.exe",
    );
  const review = async (label) => {
    await executeReviewedDeliveryAction(center.ui, { role: "button", name: label });
    await observeUntil(() => center.child.exitCode !== null, "reviewed Center shutdown");
    center.dispose();
  };
  const reattachCenter = async () => {
    // The reviewed helper launches Center as a normal shortcut; close only this
    // exact resulting image before a new debugging session takes ownership.
    const executable = center.executable;
    await observeDeliveryReopen(center);
    const item = allWindowsProcesses().find((p) => p.Path.toLowerCase() === executable.toLowerCase());
    nativeWindowAction(captureWindowOwner(item, path.dirname(center.root)), "Close");
    await observeUntil(
      () => !allWindowsProcesses().some((p) => p.Pid === item.Pid && p.Created === item.Created),
      "automatic Center normal close",
    );
    await launchCenter();
  };
  try {
    const baseline = await prepareDistinctGeneration(identity);
    screenshots.push(...baseline.screenshots);
    knowledge = await createInstalledKnowledgeContext();
    const fixture = knowledge.knowledgeFixture,
      notes = await fixture.prepareNotes();
    await fixture.navigate("notes");
    await fixture.openNote(notes.a);
    await fixture.disableAutosave();
    await knowledge.ui.fill(editor, "설치 종료 취소 후 남아야 할 합성 초안\n");
    await fixture.waitJournal(notes.a, "설치 종료 취소 후 남아야 할 합성 초안\n");
    await launchCenter();
    const before = await center.delivery("restore_inventory");
    // Cancel the visible review first: no helper or checkpoint is created.
    await center.ui.waitForTarget({ role: "button", name: "현재 데이터 보존" });
    await center.ui.click({ role: "button", name: "현재 데이터 보존" });
    const cancelReview = { role: "button", name: "취소", scope: { role: "region", name: "복구 작업 검토" } };
    await center.ui.waitForTarget(cancelReview);
    await center.ui.click(cancelReview);
    assert.deepEqual(await center.delivery("restore_inventory"), before);
    await review("현재 데이터 보존");
    await knowledge.ui.closeOwnedWindow();
    await knowledge.ui.waitForTarget({ role: "button", name: "종료 취소" });
    await knowledge.ui.click({ role: "button", name: "종료 취소" });
    await assertKnowledgeDraft(knowledge, "설치 종료 취소 후 남아야 할 합성 초안\n");
    assert.equal(await readFile(notes.aFile, "utf8"), notes.aOriginal);
    assert.ok(knowledge.child.exitCode === null, "Helper must not force a dirty product closed");
    screenshots.push(await knowledge.ui.screenshot("INSTALL-03-dirty-close-cancel"));
    // Observe the helper's real 60-second writer wait and cancel its own dialog.
    let helper;
    await observeUntil(
      () => {
        helper = nativeHelper().find((p) => {
          try {
            return (
              nativeWindowAction(captureWindowOwner(p, path.dirname(knowledge.root)), "Inspect").name ===
              "Devbox 데이터 복구"
            );
          } catch {
            return false;
          }
        });
        return !!helper;
      },
      "writer blocker recovery dialog",
      90000,
    );
    nativeWindowAction(captureWindowOwner(helper, path.dirname(knowledge.root)), "Invoke", { controlId: "2" });
    await reattachCenter();
    assert.deepEqual((await center.delivery("restore_inventory")).checkpoints, before.checkpoints);
    await assertKnowledgeDraft(knowledge, "설치 종료 취소 후 남아야 할 합성 초안\n");
    // This exact public setup differs from the retained fixture generation,
    // so it enters the real update writer gate while the draft is still live.
    const candidate = JSON.parse(
      await readFile(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"), "utf8"),
    );
    const beforeUpdate = await readFile(path.join(knowledge.root, "devbox-installation.json"), "utf8");
    await center.close();
    await rejectBusyVisibleUpdate(
      path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS, candidate.setup.name),
      knowledge.root,
    );
    assert.equal(await readFile(path.join(knowledge.root, "devbox-installation.json"), "utf8"), beforeUpdate);
    await assertKnowledgeDraft(knowledge, "설치 종료 취소 후 남아야 할 합성 초안\n");
    assert.equal(await readFile(notes.aFile, "utf8"), notes.aOriginal);
    assert.equal(knowledge.child.exitCode, null);
    screenshots.push(await knowledge.ui.screenshot("INSTALL-03-update-cancel-preserved"));
    await launchCenter();
    results.push(
      record("INSTALL-03", "PASS", [
        "Recovery review and native dirty-close cancellation preserve draft, original disk and writer blocker",
        "Exact public setup enters update against a distinct retained candidate generation; busy update is cancelled without changing installation selection or terminating dirty Knowledge",
      ]),
    );
    await knowledge.ui.press("Control+s");
    await fixture.wait(
      async () => (await readFile(notes.aFile, "utf8")) === "설치 종료 취소 후 남아야 할 합성 초안\n",
      "reviewed draft saved",
    );
    await knowledge.ui.closeOwnedWindow();
    await fixture.wait(() => knowledge.child.exitCode !== null, "Knowledge normal saved close");
    await center.close();
    await runVisibleSetup(path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS, candidate.setup.name), knowledge.root);
    await closeAutomaticallyOpenedCenter(knowledge.root);
    screenshots.push(...(await completeHealthAfterKnowledgeClose(knowledge, "업데이트 확정")));
    await launchCenter();
    const marker = path.join(knowledge.root, "owned-user-file-preserved.txt");
    await writeFile(marker, "synthetic unlisted installer preservation\n", { flag: "wx" });
    const hash = await fileDigest(notes.aFile);
    await review("현재 데이터 보존");
    await reattachCenter();
    assert.ok((await center.delivery("restore_inventory")).checkpoints.length > before.checkpoints.length);
    screenshots.push(await center.ui.screenshot("DELIVERY-02-real-data-checkpoint"));
    if (checkpointOnly) {
      results.splice(0, results.length, {
        ...record("CHECKPOINT-DIAGNOSTIC", "PASS", ["Updated installation created a checkpoint and reopened Center"]),
        diagnosticOnly: true,
        promotionEvidence: false,
      });
      return results;
    }
    // The interactive reinstall/removal runner appends this same identity after
    // the visible NSIS journey, retaining this checkpoint and exact saved hash.
    const receipt = {
      schemaVersion: 1,
      ...identity,
      root: knowledge.root,
      installationKey: knowledge.installationKey,
      noteFile: notes.aFile,
      noteSha256: hash,
      marker,
      markerSha256: await fileDigest(marker),
      screenshots,
    };
    await writeFile(path.join(path.dirname(knowledge.root), "delivery-ui-checkpoint.json"), JSON.stringify(receipt), {
      flag: "wx",
    });
    assert.equal(await exists(marker), true);
    await center.close();
    const root = knowledge.root;
    await runVisibleRemoval(root, { cancel: true });
    assert.equal(await fileDigest(notes.aFile), hash);
    assert.equal(await fileDigest(marker), receipt.markerSha256);
    await runVisibleRemoval(root);
    assert.equal(await fileDigest(notes.aFile), hash);
    assert.equal(await fileDigest(marker), receipt.markerSha256);
    const release = JSON.parse(
      await readFile(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"), "utf8"),
    );
    const failureProof = await observeInstallerFailurePreservation({
      setup: path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS, release.setup.name),
      root,
      noteFile: notes.aFile,
      noteSha256: hash,
      marker,
      markerSha256: receipt.markerSha256,
    });
    screenshots.push(...failureProof.screenshotPaths);
    const registration = await runVisibleSetup(
      path.resolve(process.env.DEVBOX_USER_FLOW_ASSETS, release.setup.name),
      root,
    );
    assert.equal(registration.installationKey, knowledge.installationKey);
    await closeAutomaticallyOpenedCenter(root);
    screenshots.push(...(await completeInstalledHealth("보존된 데이터로 재설치 확정")));
    assert.equal(await fileDigest(notes.aFile), hash);
    assert.equal(await fileDigest(marker), receipt.markerSha256);
    await knowledge.close();
    knowledge = await createInstalledKnowledgeContext();
    await knowledge.knowledgeFixture.navigate("notes");
    await knowledge.knowledgeFixture.openNote(notes.a);
    await assertKnowledgeDraft(knowledge, "설치 종료 취소 후 남아야 할 합성 초안\n");
    screenshots.push(await knowledge.ui.screenshot("DELIVERY-02-reinstalled-user-data"));
    const withdrawn = JSON.parse(
      await readFile("product-foundation-evidence/user-flows/delivery-hooks/withdrawn-update.json", "utf8"),
    );
    validateWithdrawnUpdateReceipt(withdrawn, identity, knowledge.installationKey);
    screenshots.push(...withdrawn.screenshotPaths);
    results.push(
      record("DELIVERY-02", "PASS", [
        ...failureProof.assertions,
        ...withdrawn.assertions,
        "Interactive uninstall cancel preserves installed data and unlisted file",
        "Visible removal and same-directory setup retain installation key, saved real document, unlisted file and checkpoint",
        "Reinstall completes visible fresh health recording and reviewed commit, then reopened Knowledge displays exact original saved bytes",
      ]),
    );
  } catch (error) {
    for (const id of scenarioIds)
      if (!results.some((row) => row.id === id))
        results.push(record(id, "FAIL", [String(error.message).slice(0, 500)], "delivery-ui-failed"));
  } finally {
    await finalizeDeliveryContexts(center, knowledge, results);
  }
  return results;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const results = await run();
  await writeUserFlowResults("delivery-review", results);
  if (results.some((row) => row.status !== "PASS")) process.exitCode = 1;
}

export function validateWithdrawnUpdateReceipt(proof, identity, parentKey) {
  assert.equal(proof.status, "PASS");
  assert.equal(proof.fixtureKind, "withdrawn-same-version-update");
  assert.equal(proof.baselineSourceSha, withdrawnSource);
  assert.equal(proof.candidateSourceSha, identity.sourceSha);
  assert.equal(proof.sourceSha, identity.sourceSha);
  assert.equal(proof.fixtureSha, identity.fixtureSha);
  assert.deepEqual(proof.artifactDigests, identity.artifactDigests);
  assert.equal(proof.parentInstallationKey, parentKey);
  assert.match(proof.installationKey, /^[a-f0-9]{64}$/u);
  assert.notEqual(proof.installationKey, parentKey);
  assert.ok(proof.screenshotPaths.length > 0);
  assert.ok(proof.assertions.length >= 3);
}
