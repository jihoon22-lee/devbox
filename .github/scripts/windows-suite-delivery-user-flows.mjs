// Actual Recovery review, native dirty-close cancellation and installed data preservation.
import assert from "node:assert/strict";
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
const editor = { role: "textbox", name: "Markdown 본문" };
export const scenarioIds = ["INSTALL-03", "DELIVERY-02"];
async function exists(file) {
  try {
    await access(file);
    return true;
  } catch {
    return false;
  }
}
export async function run() {
  const identity = await packagedIdentity(),
    results = [],
    screenshots = [];
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
    await center.ui.click({ role: "button", name: "데이터 및 복구" });
    await observeUntil(async () => (await center.body()).includes("현재 데이터 보존"), "recovery ready");
  };
  const nativeHelper = () =>
    allWindowsProcesses().filter(
      (p) =>
        p.Path.toLowerCase().startsWith(`${knowledge.root}${path.sep}`.toLowerCase()) &&
        path.basename(p.Path).toLowerCase() === "devbox-suite-bootstrap.exe",
    );
  const review = async (label) => {
    await center.ui.click({ role: "button", name: label });
    await center.ui.click({ role: "checkbox", name: "선택한 작업과 제품 종료를 확인했습니다." });
    await center.ui.click({ role: "button", name: "Control Center를 닫고 실행" });
    await observeUntil(() => center.child.exitCode !== null, "reviewed Center shutdown");
    center.dispose();
  };
  const reattachCenter = async () => {
    // The reviewed helper launches Center as a normal shortcut; close only this
    // exact resulting image before a new debugging session takes ownership.
    const executable = center.executable;
    await observeUntil(
      () => allWindowsProcesses().some((p) => p.Path.toLowerCase() === executable.toLowerCase()),
      "helper reopened Center",
      90000,
    );
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
    await center.ui.click({ role: "button", name: "현재 데이터 보존" });
    await center.ui.click({ role: "button", name: "취소", scope: { role: "region", name: "복구 작업 검토" } });
    assert.deepEqual(await center.delivery("restore_inventory"), before);
    await review("현재 데이터 보존");
    await knowledge.ui.closeOwnedWindow();
    await knowledge.ui.click({ role: "button", name: "종료 취소" });
    assert.equal(await knowledge.ui.text(editor), "설치 종료 취소 후 남아야 할 합성 초안\n");
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
    assert.equal(await knowledge.ui.text(editor), "설치 종료 취소 후 남아야 할 합성 초안\n");
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
    assert.equal(await knowledge.ui.text(editor), "설치 종료 취소 후 남아야 할 합성 초안\n");
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
    screenshots.push(...(await completeInstalledHealth("업데이트 확정")));
    await launchCenter();
    const marker = path.join(knowledge.root, "owned-user-file-preserved.txt");
    await writeFile(marker, "synthetic unlisted installer preservation\n", { flag: "wx" });
    const hash = await fileDigest(notes.aFile);
    await review("현재 데이터 보존");
    await reattachCenter();
    assert.ok((await center.delivery("restore_inventory")).checkpoints.length > before.checkpoints.length);
    screenshots.push(await center.ui.screenshot("DELIVERY-02-real-data-checkpoint"));
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
    assert.equal(await knowledge.ui.text(editor), "설치 종료 취소 후 남아야 할 합성 초안\n");
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
    if (center) await center.close().catch(() => {});
    if (knowledge) await knowledge.close();
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
